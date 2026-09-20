//! x.ollama.v1 — local Ollama AI integration as a FeatureModule.
//!
//! Local-only: queries go to the user's own Ollama instance over HTTP streaming.
//! The capability is announced to peers as a presence signal (auth=public) so
//! they can see AI assistance is available; no data is sent to/from peers.
//!
//! `on_invoke` / `on_message` are intentional no-ops.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use doubleslash_features::{AuthTier, CapabilityDescriptor, ChannelKind};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};
use tracing::debug;

/// Default Ollama base URL. Uses `127.0.0.1` (not `localhost`) so Windows
/// never resolves to IPv6 `::1` while the daemon is IPv4-only.
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434";
pub const DEFAULT_MODEL: &str = "llama3";

/// Capability id advertised by this module.
pub const CAPABILITY_ID: &str = "x.ollama.v1";

/// Full assistant settings snapshot (shared by Qt bridge + headless).
#[derive(Debug, Clone)]
pub struct OllamaAssistantSettings {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    pub system_prompt: String,
    pub auto_respond_direct: bool,
    pub auto_respond_room: bool,
}

impl Default for OllamaAssistantSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: DEFAULT_BASE_URL.to_owned(),
            model: DEFAULT_MODEL.to_owned(),
            system_prompt: "You are a helpful assistant.".to_owned(),
            auto_respond_direct: false,
            auto_respond_room: false,
        }
    }
}

impl OllamaAssistantSettings {
    /// Config consumed by the background Ollama task.
    pub fn to_config(&self) -> OllamaConfig {
        OllamaConfig {
            base_url: normalize_ollama_base_url(&self.base_url),
            model: self.model.clone(),
        }
    }
}

/// Read Ollama assistant fields from `$DOUBLESLASH_HOME/settings.json`.
pub fn read_assistant_settings() -> OllamaAssistantSettings {
    let path = crate::identity::Identity::default_key_dir().join("settings.json");
    let Ok(txt) = std::fs::read_to_string(&path) else {
        return OllamaAssistantSettings::default();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) else {
        return OllamaAssistantSettings::default();
    };
    OllamaAssistantSettings {
        enabled: v
            .get("ollama_enabled")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        base_url: v
            .get("ollama_base_url")
            .and_then(|x| x.as_str())
            .unwrap_or(DEFAULT_BASE_URL)
            .to_owned(),
        model: v
            .get("ollama_model")
            .and_then(|x| x.as_str())
            .unwrap_or(DEFAULT_MODEL)
            .to_owned(),
        system_prompt: v
            .get("ollama_system_prompt")
            .and_then(|x| x.as_str())
            .unwrap_or("You are a helpful assistant.")
            .to_owned(),
        auto_respond_direct: v
            .get("ollama_auto_respond_direct")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        auto_respond_room: v
            .get("ollama_auto_respond_room")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
    }
}

/// Build the `x.ollama.v1` capability descriptor.
///
/// `OllamaModule.descriptor()`:
/// `auth=public`, zero per-peer byte/datagram quota (local-only).
pub fn descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor::new(CAPABILITY_ID, "1.0", ChannelKind::Stream)
        .with_auth(AuthTier::Public)
        .with_params(json!({
            "quota_bytes_per_sec": 0,
            "quota_datagrams_per_sec": 0,
        }))
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A streamed token chunk from the Ollama API.
#[derive(Debug, Clone)]
pub struct OllamaChunk {
    pub request_id: String,
    pub text: String,
    pub done: bool,
}

/// Configuration consumed on every query (read-on-query for hot-reload).
#[derive(Debug, Clone)]
pub struct OllamaConfig {
    pub base_url: String,
    pub model: String,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            model: DEFAULT_MODEL.to_owned(),
        }
    }
}

/// Events emitted by the Ollama module task.
#[derive(Debug, Clone)]
pub enum OllamaEvent {
    /// Streamed token chunk.
    Chunk(OllamaChunk),
    /// HTTP or stream error.
    Error { request_id: String, message: String },
    /// Result of a `ListModels` command.
    /// `models` is the catalog (grouped/sorted); `error` is empty on success.
    Models {
        models: Vec<OllamaModelInfo>,
        error: String,
    },
}

/// One turn in a multi-message Ollama chat (`/api/chat`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

impl ChatTurn {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_owned(),
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_owned(),
            content: content.into(),
        }
    }
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_owned(),
            content: content.into(),
        }
    }
}

/// Stable conversation keys for auto-reply memory.
pub fn conversation_id_direct(peer_id: &str) -> String {
    format!("direct:{peer_id}")
}

/// Room history is keyed by `room_id` only.
///
/// Multi-homed clients receive the same logical room via different supernode
/// WS sessions (`inbound_supernode_id` rotates across the cluster). Including
/// the supernode in the key used to split Ollama memory and cancel scopes so
/// consecutive messages in one room looked like independent conversations.
pub fn conversation_id_room(room_id: &str) -> String {
    format!("room:{room_id}")
}

/// Max user+assistant turns kept per conversation (system is not counted).
pub const MAX_HISTORY_TURNS: usize = 12;
/// Soft cap per stored message body (chars) to bound memory / context size.
pub const MAX_TURN_CHARS: usize = 4_000;

/// Commands sent to the Ollama module task.
#[derive(Debug)]
pub enum OllamaCommand {
    /// Submit a streaming single-shot query (`/api/generate`) — no history.
    Query {
        request_id: String,
        prompt: String,
        system_prompt: String,
    },
    /// Multi-turn chat (`/api/chat`): appends `user_message` to `conversation_id`
    /// history, streams a reply, then stores the assistant response.
    Chat {
        request_id: String,
        /// e.g. `direct:<peer>` or `room:<room_id>`.
        conversation_id: String,
        user_message: String,
        system_prompt: String,
    },
    /// Drop history for one conversation (or all if empty).
    ClearConversation {
        conversation_id: String,
    },
    /// Cancel an in-flight query by request_id.
    Cancel {
        request_id: String,
    },
    /// Fetch the list of installed models from `GET <base_url>/api/tags`.
    /// The result is returned as `OllamaEvent::Models`.
    ListModels {
        base_url: String,
    },
    /// Update configuration (takes effect on next query).
    SetConfig(OllamaConfig),
    Shutdown,
}

// ---------------------------------------------------------------------------
// OllamaModule
// ---------------------------------------------------------------------------

/// Local-only Ollama streaming query manager.
pub struct OllamaModule {
    config: OllamaConfig,
    client: Client,
    /// Map of `request_id` → cancel sender.
    in_flight: HashMap<String, oneshot::Sender<()>>,
    /// Per-conversation multi-turn history (user/assistant only).
    conversations: Arc<std::sync::Mutex<HashMap<String, Vec<ChatTurn>>>>,
    event_tx: mpsc::Sender<OllamaEvent>,
    cmd_rx: mpsc::Receiver<OllamaCommand>,
}

impl OllamaModule {
    /// Create and split into `(cmd_tx, event_rx, task_future)`.
    pub fn split(
        config: OllamaConfig,
    ) -> (
        mpsc::Sender<OllamaCommand>,
        mpsc::Receiver<OllamaEvent>,
        impl std::future::Future<Output = ()> + Send,
    ) {
        let (event_tx, event_rx) = mpsc::channel::<OllamaEvent>(64);
        let (cmd_tx, cmd_rx) = mpsc::channel::<OllamaCommand>(32);
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            // Local Ollama must not be routed through HTTP_PROXY/HTTPS_PROXY.
            .no_proxy()
            .build()
            .unwrap_or_default();
        let m = Self {
            config,
            client,
            in_flight: HashMap::new(),
            conversations: Arc::new(std::sync::Mutex::new(HashMap::new())),
            event_tx,
            cmd_rx,
        };
        (cmd_tx, event_rx, m.run())
    }

    // -----------------------------------------------------------------------
    // Streaming query
    // -----------------------------------------------------------------------

    async fn start_query(&mut self, request_id: String, prompt: String, system_prompt: String) {
        // Cancel any existing task for this request_id
        if let Some(tx) = self.in_flight.remove(&request_id) {
            let _ = tx.send(());
        }

        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        self.in_flight.insert(request_id.clone(), cancel_tx);

        let url = format!(
            "{}/api/generate",
            normalize_ollama_base_url(&self.config.base_url)
        );
        let model = self.config.model.clone();
        let event_tx = self.event_tx.clone();
        let client = self.client.clone();

        tokio::spawn(async move {
            run_generate_stream(
                client,
                url,
                model,
                request_id,
                prompt,
                system_prompt,
                cancel_rx,
                event_tx,
            )
            .await;
        });
    }

    /// Multi-turn chat with retained history for `conversation_id`.
    async fn start_chat(
        &mut self,
        request_id: String,
        conversation_id: String,
        user_message: String,
        system_prompt: String,
    ) {
        if let Some(tx) = self.in_flight.remove(&request_id) {
            let _ = tx.send(());
        }
        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        self.in_flight.insert(request_id.clone(), cancel_tx);

        let user_text = truncate_turn(&user_message);
        // Append user turn + snapshot history for the request.
        let history_snapshot = {
            let mut guard = self.conversations.lock().unwrap_or_else(|e| e.into_inner());
            let turns = guard.entry(conversation_id.clone()).or_default();
            turns.push(ChatTurn::user(user_text));
            trim_history(turns);
            turns.clone()
        };

        let mut messages = Vec::with_capacity(history_snapshot.len() + 1);
        let sys = system_prompt.trim();
        if !sys.is_empty() {
            messages.push(ChatTurn::system(sys));
        }
        messages.extend(history_snapshot);

        let url = format!(
            "{}/api/chat",
            normalize_ollama_base_url(&self.config.base_url)
        );
        let model = self.config.model.clone();
        let event_tx = self.event_tx.clone();
        let client = self.client.clone();
        let conversations = Arc::clone(&self.conversations);
        let conv_id = conversation_id.clone();

        debug!(
            "[ollama] chat conv={} model={} history_msgs={}",
            conv_id,
            model,
            messages.len()
        );

        tokio::spawn(async move {
            let assistant = run_chat_stream(
                client, url, model, request_id, messages, cancel_rx, event_tx,
            )
            .await;
            if let Some(text) = assistant {
                let text = truncate_turn(&text);
                if text.is_empty() {
                    return;
                }
                if let Ok(mut guard) = conversations.lock() {
                    let turns = guard.entry(conv_id).or_default();
                    turns.push(ChatTurn::assistant(text));
                    trim_history(turns);
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Event loop
    // -----------------------------------------------------------------------

    async fn run(mut self) {
        while let Some(cmd) = self.cmd_rx.recv().await {
            match cmd {
                OllamaCommand::Shutdown => {
                    // Cancel all in-flight queries
                    for (_, tx) in self.in_flight.drain() {
                        let _ = tx.send(());
                    }
                    break;
                }
                OllamaCommand::Cancel { request_id } => {
                    if let Some(tx) = self.in_flight.remove(&request_id) {
                        let _ = tx.send(());
                    }
                }
                OllamaCommand::SetConfig(cfg) => {
                    self.config = cfg;
                }
                OllamaCommand::Query {
                    request_id,
                    prompt,
                    system_prompt,
                } => {
                    self.start_query(request_id, prompt, system_prompt).await;
                }
                OllamaCommand::Chat {
                    request_id,
                    conversation_id,
                    user_message,
                    system_prompt,
                } => {
                    self.start_chat(request_id, conversation_id, user_message, system_prompt)
                        .await;
                }
                OllamaCommand::ClearConversation { conversation_id } => {
                    if let Ok(mut guard) = self.conversations.lock() {
                        if conversation_id.is_empty() {
                            guard.clear();
                        } else {
                            guard.remove(&conversation_id);
                        }
                    }
                }
                OllamaCommand::ListModels { base_url } => {
                    let client = self.client.clone();
                    let event_tx = self.event_tx.clone();
                    tokio::spawn(async move {
                        let ev = match fetch_model_catalog(&client, &base_url).await {
                            Ok(models) => OllamaEvent::Models {
                                models,
                                error: String::new(),
                            },
                            Err(e) => OllamaEvent::Models {
                                models: vec![],
                                error: e,
                            },
                        };
                        let _ = event_tx.send(ev).await;
                    });
                }
            }
        }
    }
}

fn truncate_turn(s: &str) -> String {
    let t = s.trim();
    if t.chars().count() <= MAX_TURN_CHARS {
        return t.to_owned();
    }
    let truncated: String = t.chars().take(MAX_TURN_CHARS.saturating_sub(1)).collect();
    format!("{truncated}…")
}

/// Keep the last `MAX_HISTORY_TURNS * 2` user/assistant messages.
fn trim_history(turns: &mut Vec<ChatTurn>) {
    let max = MAX_HISTORY_TURNS.saturating_mul(2);
    if turns.len() > max {
        let drain = turns.len() - max;
        turns.drain(0..drain);
    }
}

// ---------------------------------------------------------------------------
// Streaming implementation
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    system: &'a str,
    stream: bool,
}

#[derive(Deserialize)]
struct GenerateChunk {
    response: Option<String>,
    #[serde(default)]
    done: bool,
}

/// One installed Ollama model, enriched for the settings picker.
///
/// Built from `GET /api/tags` (name, size, family, params, quant, context,
/// capabilities) plus `GET /api/ps` when the model is currently loaded
/// (`tier` is GPU vs CPU-split only for loaded models).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OllamaModelInfo {
    pub name: String,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub size_label: String,
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub parameter_size: String,
    #[serde(default)]
    pub quantization: String,
    #[serde(default)]
    pub num_ctx: u64,
    #[serde(default)]
    pub ctx_label: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// `"both"` | `"chat"` | `"vision"` | `"general"`
    pub recommended_for: String,
    pub recommended_label: String,
    /// `"full_gpu"` | `"cpu_split"` | `"unknown"` — unknown unless `/api/ps`
    /// reports the model loaded.
    pub tier: String,
    pub group: String,
    pub detail: String,
}

#[derive(Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<TagsModel>,
}

#[derive(Deserialize, Default)]
struct TagsDetails {
    #[serde(default)]
    family: String,
    #[serde(default)]
    parameter_size: String,
    #[serde(default)]
    quantization_level: String,
    #[serde(default)]
    context_length: u64,
}

#[derive(Deserialize)]
struct TagsModel {
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    details: TagsDetails,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(Deserialize, Default)]
struct PsResponse {
    #[serde(default)]
    models: Vec<PsModel>,
}

#[derive(Deserialize)]
struct PsModel {
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    size_vram: u64,
}

/// Fetch sorted model names from `GET <base_url>/api/tags` (8 s timeout).
pub async fn fetch_model_list(client: &Client, base_url: &str) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = fetch_model_catalog(client, base_url)
        .await?
        .into_iter()
        .map(|m| m.name)
        .collect();
    names.sort();
    Ok(names)
}

/// Fetch installed models with size, context, capabilities, and live GPU tier.
pub async fn fetch_model_catalog(
    client: &Client,
    base_url: &str,
) -> Result<Vec<OllamaModelInfo>, String> {
    let base = normalize_ollama_base_url(base_url);
    let tags_url = format!("{base}/api/tags");
    let resp = client
        .get(&tags_url)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .map_err(|e| format!("HTTP error talking to {tags_url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Ollama returned {} for {tags_url}", resp.status()));
    }
    let body: TagsResponse = resp
        .json()
        .await
        .map_err(|e| format!("Parse error from {tags_url}: {e}"))?;

    let ps = fetch_loaded_models(client, &base).await;
    let mut models: Vec<OllamaModelInfo> = body
        .models
        .into_iter()
        .filter(|m| !m.name.is_empty())
        .map(|m| model_info_from_tags(m, &ps))
        .collect();
    sort_model_catalog(&mut models);
    Ok(models)
}

async fn fetch_loaded_models(client: &Client, base: &str) -> Vec<PsModel> {
    let url = format!("{base}/api/ps");
    let Ok(resp) = client
        .get(&url)
        .timeout(Duration::from_secs(4))
        .send()
        .await
    else {
        return Vec::new();
    };
    if !resp.status().is_success() {
        return Vec::new();
    }
    resp.json::<PsResponse>()
        .await
        .map(|body| body.models)
        .unwrap_or_default()
}

fn model_info_from_tags(model: TagsModel, loaded: &[PsModel]) -> OllamaModelInfo {
    let capabilities = if model.capabilities.is_empty() {
        heuristic_capabilities(&model.name)
    } else {
        model.capabilities
    };
    let recommended_for = recommend_for(&capabilities);
    let recommended_label = recommended_label(recommended_for);
    let tier = gpu_tier(&model.name, model.size, loaded);
    let group = group_for(tier, recommended_for);
    let num_ctx = model.details.context_length;
    let size_label = format_bytes(model.size);
    let ctx_label = format_ctx(num_ctx);
    let family = model.details.family;
    let parameter_size = model.details.parameter_size;
    let quantization = model.details.quantization_level;
    let detail = format_detail(
        &family,
        &parameter_size,
        &quantization,
        &ctx_label,
        &size_label,
        &capabilities,
    );
    OllamaModelInfo {
        name: model.name,
        size_bytes: model.size,
        size_label,
        family,
        parameter_size,
        quantization,
        num_ctx,
        ctx_label,
        capabilities,
        recommended_for: recommended_for.to_owned(),
        recommended_label: recommended_label.to_owned(),
        tier: tier.to_owned(),
        group: group.to_owned(),
        detail,
    }
}

fn heuristic_capabilities(name: &str) -> Vec<String> {
    let nl = name.to_ascii_lowercase();
    let mut caps = vec!["completion".to_owned()];
    if [
        "vision",
        "-vl",
        ":vl",
        "llava",
        "moondream",
        "bakllava",
        "minicpm-v",
    ]
    .iter()
    .any(|k| nl.contains(k))
    {
        caps.push("vision".to_owned());
    }
    if ["qwen", "mistral", "llama", "gemma", "phi", "gpt-oss"]
        .iter()
        .any(|k| nl.contains(k))
    {
        caps.push("tools".to_owned());
    }
    if nl.contains("think") || nl.contains("reason") {
        caps.push("thinking".to_owned());
    }
    caps
}

fn recommend_for(capabilities: &[String]) -> &'static str {
    let has = |cap: &str| capabilities.iter().any(|c| c == cap);
    let vision = has("vision");
    let tools = has("tools");
    if vision && tools {
        "both"
    } else if tools {
        "chat"
    } else if vision {
        "vision"
    } else {
        "general"
    }
}

fn recommended_label(recommended_for: &str) -> &'static str {
    match recommended_for {
        "both" => "All features",
        "chat" => "Best for chat",
        "vision" => "Vision",
        _ => "General",
    }
}

fn gpu_tier(name: &str, tags_size: u64, loaded: &[PsModel]) -> &'static str {
    let Some(ps) = loaded.iter().find(|m| m.name == name) else {
        return "unknown";
    };
    let total = if ps.size > 0 { ps.size } else { tags_size };
    if total > 0 && ps.size_vram >= total.saturating_mul(9) / 10 {
        "full_gpu"
    } else if ps.size_vram > 0 {
        "cpu_split"
    } else {
        "unknown"
    }
}

fn group_for(tier: &str, recommended_for: &str) -> &'static str {
    if tier == "cpu_split" {
        return "CPU split (slower)";
    }
    recommended_label(recommended_for)
}

fn sort_model_catalog(models: &mut [OllamaModelInfo]) {
    fn rec_rank(s: &str) -> u8 {
        match s {
            "both" => 0,
            "chat" => 1,
            "vision" => 2,
            _ => 3,
        }
    }
    fn tier_rank(s: &str) -> u8 {
        match s {
            "full_gpu" => 0,
            "unknown" => 1,
            "cpu_split" => 2,
            _ => 3,
        }
    }
    models.sort_by(|a, b| {
        tier_rank(&a.tier)
            .cmp(&tier_rank(&b.tier))
            .then_with(|| rec_rank(&a.recommended_for).cmp(&rec_rank(&b.recommended_for)))
            .then_with(|| a.name.cmp(&b.name))
    });
}

fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return String::new();
    }
    const GB: f64 = 1_000_000_000.0;
    const MB: f64 = 1_000_000.0;
    let n = bytes as f64;
    if n >= GB {
        format!("{:.1} GB", n / GB)
    } else {
        format!("{:.0} MB", n / MB)
    }
}

fn format_ctx(tokens: u64) -> String {
    if tokens == 0 {
        return String::new();
    }
    if tokens.is_multiple_of(1024) {
        format!("{}k", tokens / 1024)
    } else if tokens >= 1000 {
        format!("{}k", (tokens + 500) / 1000)
    } else {
        tokens.to_string()
    }
}

fn format_detail(
    family: &str,
    parameter_size: &str,
    quantization: &str,
    ctx_label: &str,
    size_label: &str,
    capabilities: &[String],
) -> String {
    let ctx_part = if ctx_label.is_empty() {
        String::new()
    } else {
        format!("{ctx_label} ctx")
    };
    let extra_joined = capabilities
        .iter()
        .filter(|c| c.as_str() != "completion")
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let mut parts: Vec<&str> = Vec::new();
    if !family.is_empty() {
        parts.push(family);
    }
    if !parameter_size.is_empty() {
        parts.push(parameter_size);
    }
    if !quantization.is_empty() {
        parts.push(quantization);
    }
    if !ctx_part.is_empty() {
        parts.push(&ctx_part);
    }
    if !size_label.is_empty() {
        parts.push(size_label);
    }
    if !extra_joined.is_empty() {
        parts.push(&extra_joined);
    }
    parts.join(" · ")
}

/// Normalize an Ollama base URL for local use.
///
/// Maps `http://localhost:…` → `http://127.0.0.1:…` so we never depend on
/// IPv6 `::1` resolution when the daemon only bound IPv4.
pub fn normalize_ollama_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if let Some(rest) = trimmed.strip_prefix("http://localhost") {
        return format!("http://127.0.0.1{rest}");
    }
    if let Some(rest) = trimmed.strip_prefix("https://localhost") {
        return format!("https://127.0.0.1{rest}");
    }
    if let Some(rest) = trimmed.strip_prefix("http://[::1]") {
        return format!("http://127.0.0.1{rest}");
    }
    trimmed.to_owned()
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatTurn],
    stream: bool,
}

#[derive(Deserialize)]
struct ChatStreamChunk {
    message: Option<ChatStreamMessage>,
    #[serde(default)]
    done: bool,
}

#[derive(Deserialize)]
struct ChatStreamMessage {
    #[serde(default)]
    content: String,
}

#[allow(clippy::too_many_arguments)]
async fn run_generate_stream(
    client: Client,
    url: String,
    model: String,
    request_id: String,
    prompt: String,
    system_prompt: String,
    mut cancel_rx: oneshot::Receiver<()>,
    event_tx: mpsc::Sender<OllamaEvent>,
) {
    let body = GenerateRequest {
        model: &model,
        prompt: &prompt,
        system: &system_prompt,
        stream: true,
    };

    let resp = tokio::select! {
        r = client.post(&url).json(&body).send() => r,
        _ = &mut cancel_rx => return,
    };

    let resp = match resp {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            let _ = event_tx.try_send(OllamaEvent::Error {
                request_id,
                message: format!("Ollama API returned {}", r.status()),
            });
            return;
        }
        Err(e) => {
            let _ = event_tx.try_send(OllamaEvent::Error {
                request_id,
                message: format!("HTTP error: {e}"),
            });
            return;
        }
    };

    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut buf = Vec::<u8>::new();

    loop {
        let item = tokio::select! {
            item = stream.next() => item,
            _ = &mut cancel_rx => break,
        };

        let chunk = match item {
            Some(Ok(b)) => b,
            Some(Err(e)) => {
                let _ = event_tx.try_send(OllamaEvent::Error {
                    request_id,
                    message: format!("Stream error: {e}"),
                });
                return;
            }
            None => break,
        };

        buf.extend_from_slice(chunk.as_ref());

        // Each newline-delimited JSON object is one chunk
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line = buf.drain(..=pos).collect::<Vec<_>>();
            let s = match std::str::from_utf8(&line) {
                Ok(s) => s.trim().to_owned(),
                Err(_) => continue,
            };
            if s.is_empty() {
                continue;
            }
            match serde_json::from_str::<GenerateChunk>(&s) {
                Ok(gc) => {
                    let text = gc.response.unwrap_or_default();
                    let done = gc.done;
                    if !text.is_empty() || done {
                        let _ = event_tx.try_send(OllamaEvent::Chunk(OllamaChunk {
                            request_id: request_id.clone(),
                            text,
                            done,
                        }));
                    }
                    if done {
                        return;
                    }
                }
                Err(e) => {
                    debug!("Ollama JSON parse error: {e} | line: {s}");
                }
            }
        }
    }

    // Stream ended without explicit done=true
    let _ = event_tx.try_send(OllamaEvent::Chunk(OllamaChunk {
        request_id,
        text: String::new(),
        done: true,
    }));
}

/// Stream `/api/chat`. Returns the full assistant text when the stream completes
/// successfully (for history retention). `None` on cancel/error.
#[allow(clippy::too_many_arguments)]
async fn run_chat_stream(
    client: Client,
    url: String,
    model: String,
    request_id: String,
    messages: Vec<ChatTurn>,
    mut cancel_rx: oneshot::Receiver<()>,
    event_tx: mpsc::Sender<OllamaEvent>,
) -> Option<String> {
    let body = ChatRequest {
        model: &model,
        messages: &messages,
        stream: true,
    };

    let resp = tokio::select! {
        r = client.post(&url).json(&body).send() => r,
        _ = &mut cancel_rx => return None,
    };

    let resp = match resp {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            let _ = event_tx.try_send(OllamaEvent::Error {
                request_id,
                message: format!("Ollama chat API returned {}", r.status()),
            });
            return None;
        }
        Err(e) => {
            let _ = event_tx.try_send(OllamaEvent::Error {
                request_id,
                message: format!("HTTP error: {e}"),
            });
            return None;
        }
    };

    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut buf = Vec::<u8>::new();
    let mut assistant = String::new();

    loop {
        let item = tokio::select! {
            item = stream.next() => item,
            _ = &mut cancel_rx => return None,
        };

        let chunk = match item {
            Some(Ok(b)) => b,
            Some(Err(e)) => {
                let _ = event_tx.try_send(OllamaEvent::Error {
                    request_id,
                    message: format!("Stream error: {e}"),
                });
                return None;
            }
            None => break,
        };

        buf.extend_from_slice(chunk.as_ref());

        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line = buf.drain(..=pos).collect::<Vec<_>>();
            let s = match std::str::from_utf8(&line) {
                Ok(s) => s.trim().to_owned(),
                Err(_) => continue,
            };
            if s.is_empty() {
                continue;
            }
            match serde_json::from_str::<ChatStreamChunk>(&s) {
                Ok(gc) => {
                    let text = gc.message.map(|m| m.content).unwrap_or_default();
                    let done = gc.done;
                    if !text.is_empty() {
                        assistant.push_str(&text);
                        let _ = event_tx.try_send(OllamaEvent::Chunk(OllamaChunk {
                            request_id: request_id.clone(),
                            text,
                            done: false,
                        }));
                    }
                    if done {
                        let _ = event_tx.try_send(OllamaEvent::Chunk(OllamaChunk {
                            request_id: request_id.clone(),
                            text: String::new(),
                            done: true,
                        }));
                        return Some(assistant);
                    }
                }
                Err(e) => {
                    debug!("Ollama chat JSON parse error: {e} | line: {s}");
                }
            }
        }
    }

    let _ = event_tx.try_send(OllamaEvent::Chunk(OllamaChunk {
        request_id,
        text: String::new(),
        done: true,
    }));
    if assistant.is_empty() {
        None
    } else {
        Some(assistant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_url_trims_trailing_slash() {
        let base = normalize_ollama_base_url("http://127.0.0.1:11434/");
        let url = format!("{base}/api/tags");
        assert_eq!(url, "http://127.0.0.1:11434/api/tags");
    }

    #[test]
    fn normalize_maps_localhost_to_ipv4_loopback() {
        assert_eq!(
            normalize_ollama_base_url("http://localhost:11434"),
            "http://127.0.0.1:11434"
        );
        assert_eq!(
            normalize_ollama_base_url("http://localhost:11434/"),
            "http://127.0.0.1:11434"
        );
        assert_eq!(
            normalize_ollama_base_url("http://127.0.0.1:11434"),
            "http://127.0.0.1:11434"
        );
    }

    #[test]
    fn trim_history_keeps_last_turns() {
        let mut turns = Vec::new();
        for i in 0..30 {
            turns.push(ChatTurn::user(format!("u{i}")));
            turns.push(ChatTurn::assistant(format!("a{i}")));
        }
        trim_history(&mut turns);
        assert_eq!(turns.len(), MAX_HISTORY_TURNS * 2);
        assert_eq!(turns[0].content, format!("u{}", 30 - MAX_HISTORY_TURNS));
    }

    #[test]
    fn conversation_ids_are_stable() {
        assert_eq!(conversation_id_direct("abc"), "direct:abc");
        assert_eq!(conversation_id_room("room1"), "room:room1");
        // Multi-home must not split history by supernode path.
        assert_eq!(
            conversation_id_room("5919ee78b42b260c"),
            conversation_id_room("5919ee78b42b260c")
        );
    }

    #[test]
    fn tags_response_parses_model_names() {
        let json = r#"{
            "models": [
                {"name": "llama3.2:latest", "size": 1},
                {"name": "mistral:7b", "size": 2},
                {"name": "", "size": 3}
            ]
        }"#;
        let body: TagsResponse = serde_json::from_str(json).expect("parse tags");
        let mut names: Vec<String> = body
            .models
            .into_iter()
            .map(|m| m.name)
            .filter(|n| !n.is_empty())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["llama3.2:latest".to_owned(), "mistral:7b".to_owned()]
        );
    }

    #[test]
    fn tags_response_parses_capabilities_and_context() {
        let json = r#"{
            "models": [{
                "name": "qwen3-vl:2b",
                "size": 1889519687,
                "details": {
                    "family": "qwen3vl",
                    "parameter_size": "2.1B",
                    "quantization_level": "Q4_K_M",
                    "context_length": 262144
                },
                "capabilities": ["completion", "vision", "tools", "thinking"]
            }]
        }"#;
        let body: TagsResponse = serde_json::from_str(json).expect("parse tags");
        let info = model_info_from_tags(body.models.into_iter().next().unwrap(), &[]);
        assert_eq!(info.name, "qwen3-vl:2b");
        assert_eq!(info.num_ctx, 262144);
        assert_eq!(info.ctx_label, "256k");
        assert_eq!(info.size_label, "1.9 GB");
        assert_eq!(info.parameter_size, "2.1B");
        assert_eq!(info.quantization, "Q4_K_M");
        assert_eq!(info.recommended_for, "both");
        assert_eq!(info.recommended_label, "All features");
        assert_eq!(info.group, "All features");
        assert_eq!(info.tier, "unknown");
        assert!(info.detail.contains("vision"));
        assert!(info.detail.contains("tools"));
        assert!(info.detail.contains("thinking"));
        assert!(info.detail.contains("256k"));
    }

    #[test]
    fn completion_only_model_is_general() {
        let model = TagsModel {
            name: "phi4:latest".into(),
            size: 9053116391,
            details: TagsDetails {
                family: "phi3".into(),
                parameter_size: "14.7B".into(),
                quantization_level: "Q4_K_M".into(),
                context_length: 16384,
            },
            capabilities: vec!["completion".into()],
        };
        let info = model_info_from_tags(model, &[]);
        assert_eq!(info.recommended_for, "general");
        assert_eq!(info.ctx_label, "16k");
        assert_eq!(info.size_label, "9.1 GB");
        assert!(!info.detail.contains("tools"));
    }

    #[test]
    fn tools_without_vision_is_best_for_chat() {
        assert_eq!(
            recommend_for(&["completion".into(), "tools".into()]),
            "chat"
        );
        assert_eq!(
            recommend_for(&["completion".into(), "vision".into()]),
            "vision"
        );
    }

    #[test]
    fn heuristic_capabilities_cover_vision_and_tools() {
        let caps = heuristic_capabilities("qwen3-vl:2b");
        assert!(caps.contains(&"vision".to_owned()));
        assert!(caps.contains(&"tools".to_owned()));
        let phi = heuristic_capabilities("phi4:latest");
        assert!(phi.contains(&"tools".to_owned()));
        assert!(!phi.contains(&"vision".to_owned()));
    }

    #[test]
    fn gpu_tier_from_loaded_process_list() {
        let loaded = vec![
            PsModel {
                name: "phi4:latest".into(),
                size: 9_000_000_000,
                size_vram: 9_000_000_000,
            },
            PsModel {
                name: "gemma2:latest".into(),
                size: 5_000_000_000,
                size_vram: 1_000_000_000,
            },
        ];
        assert_eq!(gpu_tier("phi4:latest", 9_000_000_000, &loaded), "full_gpu");
        assert_eq!(
            gpu_tier("gemma2:latest", 5_000_000_000, &loaded),
            "cpu_split"
        );
        assert_eq!(gpu_tier("mistral:7b", 4_000_000_000, &loaded), "unknown");
        let split = model_info_from_tags(
            TagsModel {
                name: "gemma2:latest".into(),
                size: 5_000_000_000,
                details: TagsDetails::default(),
                capabilities: vec!["completion".into(), "tools".into()],
            },
            &loaded,
        );
        assert_eq!(split.tier, "cpu_split");
        assert_eq!(split.group, "CPU split (slower)");
    }

    #[test]
    fn catalog_sorts_chat_models_ahead_of_general() {
        let mut models = vec![
            model_info_from_tags(
                TagsModel {
                    name: "phi4:latest".into(),
                    size: 1,
                    details: TagsDetails::default(),
                    capabilities: vec!["completion".into()],
                },
                &[],
            ),
            model_info_from_tags(
                TagsModel {
                    name: "qwen2.5:7b".into(),
                    size: 1,
                    details: TagsDetails::default(),
                    capabilities: vec!["completion".into(), "tools".into()],
                },
                &[],
            ),
            model_info_from_tags(
                TagsModel {
                    name: "qwen3-vl:8b".into(),
                    size: 1,
                    details: TagsDetails::default(),
                    capabilities: vec!["completion".into(), "vision".into(), "tools".into()],
                },
                &[],
            ),
        ];
        sort_model_catalog(&mut models);
        assert_eq!(models[0].name, "qwen3-vl:8b");
        assert_eq!(models[1].name, "qwen2.5:7b");
        assert_eq!(models[2].name, "phi4:latest");
    }

    #[tokio::test]
    async fn list_models_command_emits_error_on_unreachable_host() {
        let (cmd_tx, mut event_rx, task) = OllamaModule::split(OllamaConfig {
            base_url: "http://127.0.0.1:9".to_owned(), // closed port
            model: DEFAULT_MODEL.to_owned(),
        });
        tokio::spawn(task);
        cmd_tx
            .send(OllamaCommand::ListModels {
                base_url: "http://127.0.0.1:9".to_owned(),
            })
            .await
            .expect("send ListModels");
        let ev = tokio::time::timeout(Duration::from_secs(6), event_rx.recv())
            .await
            .expect("timeout waiting for Models event")
            .expect("channel closed");
        match ev {
            OllamaEvent::Models { models, error } => {
                assert!(models.is_empty());
                assert!(!error.is_empty(), "expected HTTP error message");
            }
            other => panic!("unexpected event: {other:?}"),
        }
        let _ = cmd_tx.send(OllamaCommand::Shutdown).await;
    }
}
