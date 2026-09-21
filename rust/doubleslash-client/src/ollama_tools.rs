//! Client-control tools for the local Ollama assistant.
//!
//! When `ollama_tools_enabled` is on, auto-reply chat requests advertise these
//! functions to Ollama. The model returns `tool_calls`; this host executes them
//! through the same `ConnectionCommand` / `CallCommand` paths the UI uses.
//! Results go back to the model only — they are not posted to peers unless the
//! model then sends chat via a send tool (or its final text auto-reply).
//!
//! Default-off. Restored backups strip the flag. Intended for local test
//! agents (Bobert / `.clientA`), not as a general remote-control surface.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::{Mutex, RwLock};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::call_controller::CallCommand;
use crate::chat_store::{self, ChatStore};
use crate::connection_manager::{ConnectionCommand, ConnectionEvent};
use crate::identity::Identity;
use crate::peer_store::{PeerRecord, PeerStore};
use crate::protocol::{MessageType, SignalingMessage};
use crate::room_store::{RoomEntry, RoomStore};
use crate::session_state::PeerSessionState;

/// Appended to the auto-reply system prompt when tools are enabled.
pub const TOOLS_SYSTEM_ADDON: &str = "You can operate this DoubleSlash client through tools. \
Use them to inspect state and to perform actions when asked to test or control the client. \
Call list_peers or list_rooms to resolve handles and room names to IDs instead of guessing. \
join_voice / start_call / accept_call put this client on the voice path; speak says a short line out loud there \
(text chat replies are silent on voice until you call speak). \
When a user message has an attached image, you can see it — do not claim to be text-only. \
Use view_image only if no image is already attached. Pass message_id (xfer-…) not the file name; \
names with spaces are fine as attachment labels but are not chat ids. With no args, the latest image in this conversation is used. \
After tool calls, always send a short user-facing summary of what you did and what happened. \
Do not dump raw JSON unless the user asks.";

/// Injected when Ollama rejects tools for the selected model (e.g. phi4).
pub const TOOLS_UNAVAILABLE_NOTICE: &str = "This model cannot call client tools. \
Do not invent tool calls, tool names, JSON tool results, or fake room/peer lists. \
If asked to list rooms, join voice, speak, or control the client, say plainly that \
this model has no tools and a tools-capable model is required (qwen3, llama3.1, or gemma3).";

const MAX_RESULT_CHARS: usize = 12_000;
const MAX_CHAT_BODY_CHARS: usize = 4_000;

/// Live transport/call facts updated from `ConnectionEvent`s.
#[derive(Debug, Clone, Default)]
pub struct LiveClientView {
    pub local_handle: String,
    pub public_id: String,
    pub peer_id: String,
    pub connected_peers: BTreeSet<String>,
    pub connected_supernodes: BTreeSet<String>,
    pub peer_sessions: BTreeMap<String, String>,
    pub incoming_call_from: Option<String>,
    pub incoming_call_fallback: Option<(String, String, String, String)>,
    pub active_call_peer: Option<String>,
    pub voice_room: Option<(String, String)>,
    pub subscribed_rooms: BTreeSet<(String, String)>,
    pub room_voice_members: BTreeMap<(String, String), Vec<String>>,
    pub room_chat_members: BTreeMap<(String, String), Vec<String>>,
}

impl LiveClientView {
    pub fn apply_event(&mut self, ev: &ConnectionEvent) {
        match ev {
            ConnectionEvent::PeerConnected(id) => {
                self.connected_peers.insert(id.clone());
            }
            ConnectionEvent::PeerDisconnected(id) => {
                self.connected_peers.remove(id);
                self.peer_sessions.remove(id);
                if self.active_call_peer.as_deref() == Some(id.as_str()) {
                    self.active_call_peer = None;
                }
                if self.incoming_call_from.as_deref() == Some(id.as_str()) {
                    self.incoming_call_from = None;
                    self.incoming_call_fallback = None;
                }
            }
            ConnectionEvent::SupernodeConnected(id) => {
                self.connected_supernodes.insert(id.clone());
            }
            ConnectionEvent::SupernodeDisconnected(id) => {
                self.connected_supernodes.remove(id);
            }
            ConnectionEvent::SessionStateUpdate(state) => {
                apply_session(self, state);
            }
            ConnectionEvent::CallRequest {
                peer_id,
                fallback_supernode_id,
                fallback_room_id,
                fallback_invite_token,
            } => {
                self.incoming_call_from = Some(peer_id.clone());
                if !fallback_room_id.is_empty() {
                    self.incoming_call_fallback = Some((
                        peer_id.clone(),
                        fallback_supernode_id.clone(),
                        fallback_room_id.clone(),
                        fallback_invite_token.clone(),
                    ));
                } else {
                    self.incoming_call_fallback = None;
                }
            }
            ConnectionEvent::CallAccepted { peer_id } => {
                self.active_call_peer = Some(peer_id.clone());
                self.incoming_call_from = None;
                self.incoming_call_fallback = None;
            }
            ConnectionEvent::CallEnded { peer_id } => {
                if self.active_call_peer.as_deref() == Some(peer_id.as_str()) {
                    self.active_call_peer = None;
                }
                if self.incoming_call_from.as_deref() == Some(peer_id.as_str()) {
                    self.incoming_call_from = None;
                    self.incoming_call_fallback = None;
                }
            }
            ConnectionEvent::CallFallbackRoomReady {
                peer_id,
                supernode_id,
                room_id,
            } => {
                self.active_call_peer = Some(peer_id.clone());
                self.voice_room = Some((supernode_id.clone(), room_id.clone()));
            }
            ConnectionEvent::RoomJoinRejected { room_id, .. } => {
                if self.voice_room.as_ref().is_some_and(|(_, r)| r == room_id) {
                    self.voice_room = None;
                }
            }
            ConnectionEvent::RoomMembersChanged {
                supernode_id,
                room_id,
                members,
                chat_members,
            } => {
                let key = (supernode_id.clone(), room_id.clone());
                self.room_voice_members.insert(key.clone(), members.clone());
                self.room_chat_members.insert(key, chat_members.clone());
            }
            _ => {}
        }
    }
}

fn apply_session(live: &mut LiveClientView, state: &PeerSessionState) {
    let mode = format!(
        "{}:{}",
        state.chat_path.as_mode_str(),
        match state.chat_health {
            crate::session_state::ChatHealth::Disconnected => "disconnected",
            crate::session_state::ChatHealth::Connecting => "connecting",
            crate::session_state::ChatHealth::Connected => "connected",
            crate::session_state::ChatHealth::Degraded => "degraded",
        }
    );
    live.peer_sessions.insert(state.peer_id.clone(), mode);
}

/// Result of one tool invocation. `images` are base64 for the next model turn.
pub struct ToolOutcome {
    pub text: String,
    pub images: Vec<String>,
}

impl ToolOutcome {
    fn json(v: Value) -> Self {
        Self {
            text: truncate_result(&v.to_string()),
            images: Vec::new(),
        }
    }
    fn err(e: String) -> Self {
        Self::json(json!({ "ok": false, "error": e }))
    }
}

/// Executes Ollama tool calls against the running client.
pub struct OllamaToolHost {
    identity: Arc<Identity>,
    peer_store: Arc<RwLock<PeerStore>>,
    room_store: Arc<RwLock<RoomStore>>,
    chat_store: Arc<ChatStore>,
    cmd_tx: mpsc::Sender<ConnectionCommand>,
    call_cmd_tx: mpsc::Sender<CallCommand>,
    live: Arc<Mutex<LiveClientView>>,
    /// Conversation the current Ollama Chat belongs to (`direct:` / `room:`).
    active_conversation: Mutex<Option<String>>,
}

impl std::fmt::Debug for OllamaToolHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OllamaToolHost")
    }
}

impl OllamaToolHost {
    pub fn new(
        identity: Arc<Identity>,
        peer_store: Arc<RwLock<PeerStore>>,
        room_store: Arc<RwLock<RoomStore>>,
        chat_store: Arc<ChatStore>,
        cmd_tx: mpsc::Sender<ConnectionCommand>,
        call_cmd_tx: mpsc::Sender<CallCommand>,
        local_handle: String,
    ) -> Arc<Self> {
        let live = LiveClientView {
            local_handle,
            public_id: identity.public_id(),
            peer_id: identity.peer_id(),
            ..LiveClientView::default()
        };
        Arc::new(Self {
            identity,
            peer_store,
            room_store,
            chat_store,
            cmd_tx,
            call_cmd_tx,
            live: Arc::new(Mutex::new(live)),
            active_conversation: Mutex::new(None),
        })
    }

    pub fn set_active_conversation(&self, conversation_id: String) {
        *self.active_conversation.lock() = Some(conversation_id);
    }

    pub fn observe(&self, ev: &ConnectionEvent) {
        self.live.lock().apply_event(ev);
    }

    pub fn current_voice_room(&self) -> Option<(String, String)> {
        self.live.lock().voice_room.clone()
    }

    /// JSON Schema tool list for `POST /api/chat`.
    pub fn ollama_tools() -> Vec<Value> {
        tool_schemas()
    }

    pub fn invoke(&self, name: &str, arguments: &Value) -> ToolOutcome {
        let args = normalize_args(arguments);
        info!("[ollama] tool {name}");
        if name == "view_image" {
            return match self.view_image(&args) {
                Ok((v, images)) => ToolOutcome {
                    text: truncate_result(&v.to_string()),
                    images,
                },
                Err(e) => {
                    warn!("[ollama] view_image failed: {e}");
                    ToolOutcome::err(e)
                }
            };
        }
        let result = match name {
            "get_status" => self.get_status(),
            "list_peers" => self.list_peers(),
            "list_rooms" => self.list_rooms(),
            "get_room_members" => self.get_room_members(&args),
            "get_recent_chat" => self.get_recent_chat(&args),
            "send_direct_chat" => self.send_direct_chat(&args),
            "send_room_chat" => self.send_room_chat(&args),
            "subscribe_room_chat" => self.subscribe_room_chat(&args),
            "unsubscribe_room_chat" => self.unsubscribe_room_chat(&args),
            "join_voice" => self.join_voice(&args),
            "leave_voice" => self.leave_voice(&args),
            "create_room" => self.create_room(&args),
            "start_call" => self.start_call(&args),
            "end_call" => self.end_call(&args),
            "accept_call" => self.accept_call(&args),
            "decline_call" => self.decline_call(&args),
            "accept_invite" => self.accept_invite(&args),
            "refresh_rooms" => self.refresh_rooms(&args),
            "speak" => self.speak(&args),
            other => Err(format!("unknown tool '{other}'")),
        };
        match result {
            Ok(v) => ToolOutcome::json(v),
            Err(e) => ToolOutcome::err(e),
        }
    }

    fn get_status(&self) -> Result<Value, String> {
        let live = self.live.lock().clone();
        let settings = crate::ollama_module::read_assistant_settings();
        Ok(json!({
            "ok": true,
            "handle": if live.local_handle.is_empty() {
                settings_handle()
            } else {
                live.local_handle.clone()
            },
            "public_id": live.public_id,
            "peer_id": live.peer_id,
            "connected_peers": live.connected_peers.iter().cloned().collect::<Vec<_>>(),
            "connected_supernodes": live.connected_supernodes.iter().cloned().collect::<Vec<_>>(),
            "peer_sessions": live.peer_sessions,
            "incoming_call_from": live.incoming_call_from,
            "active_call_peer": live.active_call_peer,
            "voice_room": live.voice_room.as_ref().map(|(sn, rid)| json!({
                "supernode_id": sn,
                "room_id": rid,
            })),
            "subscribed_rooms": live.subscribed_rooms.iter().map(|(sn, rid)| json!({
                "supernode_id": sn,
                "room_id": rid,
            })).collect::<Vec<_>>(),
            "ollama_model": settings.model,
            "tools_enabled": settings.tools_enabled,
            "voice_enabled": settings.voice_enabled,
            "stt_model": settings.stt_model,
        }))
    }

    fn list_peers(&self) -> Result<Value, String> {
        let live = self.live.lock().clone();
        let store = self.peer_store.read();
        let peers: Vec<Value> = store
            .list_non_supernode_peers()
            .into_iter()
            .map(|p| {
                json!({
                    "handle": p.handle,
                    "peer_id": p.peer_id,
                    "public_id": p.identity_pub,
                    "blocked": p.blocked,
                    "connected": live.connected_peers.contains(&p.peer_id)
                        || live.connected_peers.contains(&p.identity_pub),
                    "session": live.peer_sessions.get(&p.peer_id)
                        .or_else(|| live.peer_sessions.get(&p.identity_pub))
                        .cloned(),
                })
            })
            .collect();
        let supernodes: Vec<Value> = store
            .supernodes()
            .into_iter()
            .map(|p| {
                json!({
                    "handle": p.handle,
                    "peer_id": p.peer_id,
                    "public_id": p.identity_pub,
                    "connected": live.connected_supernodes.contains(&p.identity_pub)
                        || live.connected_supernodes.contains(&p.peer_id),
                })
            })
            .collect();
        Ok(json!({ "ok": true, "peers": peers, "supernodes": supernodes }))
    }

    fn list_rooms(&self) -> Result<Value, String> {
        let live = self.live.lock().clone();
        let store = self.room_store.read();
        let rooms: Vec<Value> = store
            .list()
            .into_iter()
            .map(|r| {
                let hidden = store.is_hidden_from_sidebar(&r.supernode_id, &r.room_id);
                let key = (r.supernode_id.clone(), r.room_id.clone());
                json!({
                    "name": r.room_name,
                    "room_id": r.room_id,
                    "supernode_id": r.supernode_id,
                    "type": r.room_type,
                    "parent_id": r.parent_id,
                    "is_creator": r.is_creator,
                    "hidden": hidden,
                    "subscribed": live.subscribed_rooms.contains(&key),
                    "voice": live.voice_room.as_ref() == Some(&key),
                })
            })
            .collect();
        Ok(json!({ "ok": true, "rooms": rooms }))
    }

    fn get_room_members(&self, args: &Value) -> Result<Value, String> {
        let room = self.resolve_room(args)?;
        let live = self.live.lock();
        let key = (room.supernode_id.clone(), room.room_id.clone());
        let voice = live
            .room_voice_members
            .get(&key)
            .cloned()
            .unwrap_or_default();
        let chat = live
            .room_chat_members
            .get(&key)
            .cloned()
            .unwrap_or_default();
        Ok(json!({
            "ok": true,
            "room_id": room.room_id,
            "supernode_id": room.supernode_id,
            "name": room.room_name,
            "voice_members": voice,
            "chat_members": chat,
            "note": if voice.is_empty() && chat.is_empty() {
                "No live roster yet — subscribe or join voice first, then retry."
            } else {
                ""
            },
        }))
    }

    fn get_recent_chat(&self, args: &Value) -> Result<Value, String> {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(20)
            .clamp(1, 50) as usize;
        let key = if let Ok(room) = self.resolve_room(args) {
            chat_store::room_conversation_id(&room.room_id)
        } else if let Some(peer) = arg_str(args, &["peer", "peer_id", "handle"]) {
            self.peer_id_for(&peer)?
        } else {
            return Err("need peer or room".into());
        };
        let mut msgs = self
            .chat_store
            .get_history(&key, 0)
            .map_err(|e| e.to_string())?;
        msgs.reverse();
        if msgs.len() > limit {
            msgs.drain(0..msgs.len() - limit);
        }
        let out: Vec<Value> = msgs
            .into_iter()
            .map(|m| {
                let mut body = m.body;
                if body.chars().count() > 500 {
                    body = format!("{}…", body.chars().take(499).collect::<String>());
                }
                json!({
                    "id": m.id,
                    "from": if m.sender_handle.is_empty() { m.sender } else { m.sender_handle },
                    "mine": m.is_self,
                    "kind": m.kind.as_str(),
                    "body": body,
                    "attachment": m.attachment_name,
                    "has_image": crate::ollama_module::is_vision_filename(&m.attachment_name)
                        && !m.attachment_path.is_empty(),
                    "timestamp": m.timestamp,
                })
            })
            .collect();
        Ok(json!({ "ok": true, "conversation": key, "messages": out }))
    }

    fn send_direct_chat(&self, args: &Value) -> Result<Value, String> {
        let peer_q = arg_str(args, &["peer", "peer_id", "handle"])
            .ok_or_else(|| "missing peer".to_string())?;
        let message = arg_str(args, &["message", "body", "text"])
            .ok_or_else(|| "missing message".to_string())?;
        let message = truncate_chars(&message, MAX_CHAT_BODY_CHARS);
        if message.trim().is_empty() {
            return Err("empty message".into());
        }
        let target = self.peer_id_for(&peer_q)?;
        let handle = settings_handle();
        let message_id = uuid::Uuid::new_v4().to_string();
        let now_ts = now_secs();
        let chat_msg = chat_store::ChatMessage {
            id: message_id.clone(),
            peer_id: target.clone(),
            sender: self.identity.public_id(),
            recipient: target.clone(),
            body: message.clone(),
            timestamp: now_ts,
            is_self: true,
            status: chat_store::MessageStatus::Sending,
            kind: chat_store::MessageKind::Text,
            attachment_name: String::new(),
            attachment_path: String::new(),
            size_str: String::new(),
            status_note: String::new(),
            sender_handle: handle.clone(),
        };
        self.chat_store
            .upsert(&chat_msg)
            .map_err(|e| e.to_string())?;
        let mut msg = SignalingMessage::new(MessageType::ChatMessage, self.identity.public_id());
        msg.target = Some(target.clone());
        msg.payload
            .insert("body".into(), Value::String(message.clone()));
        msg.payload
            .insert("message_id".into(), Value::String(message_id.clone()));
        if !handle.is_empty() {
            msg.payload
                .insert("sender_handle".into(), Value::String(handle));
        }
        self.try_conn(ConnectionCommand::SendMessage(msg))?;
        Ok(json!({
            "ok": true,
            "queued": true,
            "peer_id": target,
            "message_id": message_id,
        }))
    }

    fn send_room_chat(&self, args: &Value) -> Result<Value, String> {
        let room = self.resolve_room(args)?;
        let message = arg_str(args, &["message", "body", "text"])
            .ok_or_else(|| "missing message".to_string())?;
        let message = truncate_chars(&message, MAX_CHAT_BODY_CHARS);
        if message.trim().is_empty() {
            return Err("empty message".into());
        }
        let message_id = uuid::Uuid::new_v4().to_string();
        self.try_conn(ConnectionCommand::SendSfuChat {
            supernode_id: room.supernode_id.clone(),
            room_id: room.room_id.clone(),
            body: message,
            sender_handle: settings_handle(),
            message_id: message_id.clone(),
        })?;
        Ok(json!({
            "ok": true,
            "queued": true,
            "room_id": room.room_id,
            "supernode_id": room.supernode_id,
            "message_id": message_id,
        }))
    }

    fn subscribe_room_chat(&self, args: &Value) -> Result<Value, String> {
        let room = self.resolve_room(args)?;
        self.try_conn(ConnectionCommand::SubscribeRoomChat {
            supernode_id: room.supernode_id.clone(),
            room_id: room.room_id.clone(),
        })?;
        self.live
            .lock()
            .subscribed_rooms
            .insert((room.supernode_id.clone(), room.room_id.clone()));
        Ok(json!({
            "ok": true,
            "queued": true,
            "action": "subscribe_room_chat",
            "room_id": room.room_id,
            "supernode_id": room.supernode_id,
        }))
    }

    fn unsubscribe_room_chat(&self, args: &Value) -> Result<Value, String> {
        let room = self.resolve_room(args)?;
        self.try_conn(ConnectionCommand::UnsubscribeRoomChat {
            supernode_id: room.supernode_id.clone(),
            room_id: room.room_id.clone(),
        })?;
        self.live
            .lock()
            .subscribed_rooms
            .remove(&(room.supernode_id.clone(), room.room_id.clone()));
        Ok(json!({
            "ok": true,
            "queued": true,
            "action": "unsubscribe_room_chat",
            "room_id": room.room_id,
        }))
    }

    fn join_voice(&self, args: &Value) -> Result<Value, String> {
        let room = self.resolve_room(args)?;
        {
            let mut live = self.live.lock();
            if let Some((sn, rid)) = live.voice_room.clone() {
                if sn != room.supernode_id || rid != room.room_id {
                    let _ = self.cmd_tx.try_send(ConnectionCommand::LeaveRoom {
                        supernode_id: sn,
                        room_id: rid,
                    });
                }
            }
            live.voice_room = Some((room.supernode_id.clone(), room.room_id.clone()));
            live.subscribed_rooms
                .insert((room.supernode_id.clone(), room.room_id.clone()));
        }
        let is_private = room.room_type.eq_ignore_ascii_case("private");
        let use_invite = crate::connection_manager::should_use_private_room_invite(
            false,
            is_private,
            room.is_creator,
            !room.invite_token.is_empty(),
        );
        if use_invite {
            self.try_conn(ConnectionCommand::JoinRoomWithInvite {
                supernode_id: room.supernode_id.clone(),
                room_id: room.room_id.clone(),
                invite_token: room.invite_token.clone(),
            })?;
        } else {
            self.try_conn(ConnectionCommand::JoinRoom {
                supernode_id: room.supernode_id.clone(),
                room_id: room.room_id.clone(),
            })?;
        }
        self.try_call(CallCommand::SetRoomMode {
            supernode_id: room.supernode_id.clone(),
            room_id: room.room_id.clone(),
        })?;
        self.try_call(CallCommand::StartAudio {
            voice_activation: voice_activation_setting(),
        })?;
        self.enable_agent_voice_if_configured();
        Ok(json!({
            "ok": true,
            "queued": true,
            "action": "join_voice",
            "room_id": room.room_id,
            "supernode_id": room.supernode_id,
            "used_invite": use_invite,
        }))
    }

    fn leave_voice(&self, args: &Value) -> Result<Value, String> {
        let (sn, rid) = if args_has_room(args) {
            let room = self.resolve_room(args)?;
            (room.supernode_id, room.room_id)
        } else {
            self.live
                .lock()
                .voice_room
                .clone()
                .ok_or_else(|| "not in a voice room".to_string())?
        };
        self.try_conn(ConnectionCommand::LeaveRoom {
            supernode_id: sn.clone(),
            room_id: rid.clone(),
        })?;
        let _ = self.call_cmd_tx.try_send(CallCommand::ClearRoomMode);
        let _ = self.call_cmd_tx.try_send(CallCommand::StopAudio);
        let mut live = self.live.lock();
        if live.voice_room.as_ref() == Some(&(sn.clone(), rid.clone())) {
            live.voice_room = None;
        }
        Ok(json!({
            "ok": true,
            "queued": true,
            "action": "leave_voice",
            "room_id": rid,
            "supernode_id": sn,
        }))
    }

    fn create_room(&self, args: &Value) -> Result<Value, String> {
        let name =
            arg_str(args, &["name", "room_name"]).ok_or_else(|| "missing name".to_string())?;
        let room_type = arg_str(args, &["type", "room_type"]).unwrap_or_else(|| "public".into());
        if room_type != "public" && room_type != "private" {
            return Err("type must be public or private".into());
        }
        let supernode_id = match arg_str(args, &["supernode_id", "supernode"]) {
            Some(s) => resolve_supernode(&self.peer_store.read(), &s)?,
            None => {
                let live = self.live.lock();
                live.connected_supernodes
                    .iter()
                    .next()
                    .cloned()
                    .ok_or_else(|| "no connected supernode; pass supernode_id".to_string())?
            }
        };
        self.try_conn(ConnectionCommand::CreateRoom {
            supernode_id: supernode_id.clone(),
            room_name: name.clone(),
            room_type: room_type.clone(),
            room_id: None,
            creator_id: None,
            materialize_only: false,
            invite_policy: "owner".into(),
            invite_token: String::new(),
        })?;
        Ok(json!({
            "ok": true,
            "queued": true,
            "action": "create_room",
            "name": name,
            "type": room_type,
            "supernode_id": supernode_id,
        }))
    }

    fn start_call(&self, args: &Value) -> Result<Value, String> {
        let peer_q = arg_str(args, &["peer", "peer_id", "handle"])
            .ok_or_else(|| "missing peer".to_string())?;
        let target = self.peer_id_for(&peer_q)?;
        {
            let mut live = self.live.lock();
            if let Some((sn, rid)) = live.voice_room.take() {
                let _ = self.cmd_tx.try_send(ConnectionCommand::LeaveRoom {
                    supernode_id: sn,
                    room_id: rid,
                });
                let _ = self.call_cmd_tx.try_send(CallCommand::ClearRoomMode);
                let _ = self.call_cmd_tx.try_send(CallCommand::StopAudio);
            }
            live.active_call_peer = Some(target.clone());
        }
        let mut msg = SignalingMessage::new(MessageType::CallRequest, self.identity.public_id());
        msg.target = Some(target.clone());
        self.try_conn(ConnectionCommand::SendMessage(msg))?;
        self.try_call(CallCommand::StartAudio {
            voice_activation: voice_activation_setting(),
        })?;
        self.try_call(CallCommand::InitiatePeer {
            peer_id: target.clone(),
            host: None,
            port: None,
        })?;
        self.enable_agent_voice_if_configured();
        Ok(json!({ "ok": true, "queued": true, "action": "start_call", "peer_id": target }))
    }

    fn end_call(&self, args: &Value) -> Result<Value, String> {
        let target = self.resolve_call_peer(args)?;
        let mut msg = SignalingMessage::new(MessageType::CallEnd, self.identity.public_id());
        msg.target = Some(target.clone());
        self.try_conn(ConnectionCommand::SendMessage(msg))?;
        let _ = self.call_cmd_tx.try_send(CallCommand::StopAudio);
        let _ = self.call_cmd_tx.try_send(CallCommand::RemovePeer {
            peer_id: target.clone(),
        });
        let mut live = self.live.lock();
        if live.active_call_peer.as_deref() == Some(target.as_str()) {
            live.active_call_peer = None;
        }
        if live.incoming_call_from.as_deref() == Some(target.as_str()) {
            live.incoming_call_from = None;
            live.incoming_call_fallback = None;
        }
        Ok(json!({ "ok": true, "queued": true, "action": "end_call", "peer_id": target }))
    }

    fn accept_call(&self, args: &Value) -> Result<Value, String> {
        let target = self.resolve_call_peer(args)?;
        let fallback = {
            let mut live = self.live.lock();
            let fb = live.incoming_call_fallback.take();
            live.incoming_call_from = None;
            live.active_call_peer = Some(target.clone());
            fb
        };
        let mut msg = SignalingMessage::new(MessageType::CallAccept, self.identity.public_id());
        msg.target = Some(target.clone());
        self.try_conn(ConnectionCommand::SendMessage(msg))?;
        if let Some((peer, sn, rid, token)) = fallback.filter(|(p, _, _, _)| p == &target) {
            let _ = peer;
            if !token.is_empty() {
                self.try_conn(ConnectionCommand::JoinRoomWithInvite {
                    supernode_id: sn.clone(),
                    room_id: rid.clone(),
                    invite_token: token,
                })?;
            } else {
                self.try_conn(ConnectionCommand::JoinRoom {
                    supernode_id: sn.clone(),
                    room_id: rid.clone(),
                })?;
            }
            self.live.lock().voice_room = Some((sn.clone(), rid.clone()));
            self.try_call(CallCommand::SetRoomMode {
                supernode_id: sn,
                room_id: rid,
            })?;
            self.try_call(CallCommand::StartAudio {
                voice_activation: voice_activation_setting(),
            })?;
        } else {
            self.try_call(CallCommand::StartAudio {
                voice_activation: voice_activation_setting(),
            })?;
            self.try_call(CallCommand::InitiatePeer {
                peer_id: target.clone(),
                host: None,
                port: None,
            })?;
        }
        self.enable_agent_voice_if_configured();
        Ok(json!({ "ok": true, "queued": true, "action": "accept_call", "peer_id": target }))
    }

    fn decline_call(&self, args: &Value) -> Result<Value, String> {
        let target = self.resolve_call_peer(args)?;
        let mut msg = SignalingMessage::new(MessageType::CallEnd, self.identity.public_id());
        msg.target = Some(target.clone());
        self.try_conn(ConnectionCommand::SendMessage(msg))?;
        let mut live = self.live.lock();
        live.incoming_call_from = None;
        live.incoming_call_fallback = None;
        Ok(json!({ "ok": true, "queued": true, "action": "decline_call", "peer_id": target }))
    }

    fn speak(&self, args: &Value) -> Result<Value, String> {
        let text = arg_str(args, &["text", "message", "speech"])
            .ok_or_else(|| "missing text".to_string())?;
        let in_voice = {
            let live = self.live.lock();
            live.voice_room.is_some() || live.active_call_peer.is_some()
        };
        if !in_voice {
            if args_has_room(args) {
                self.join_voice(args)?;
            } else {
                return Err(
                    "not in voice — call join_voice (with room) or start_call, then speak".into(),
                );
            }
        }
        let pcm = crate::agent_voice::synthesize_pcm(&text)?;
        self.try_call(CallCommand::SetAgentVoice(true))?;
        self.try_call(CallCommand::EnqueueSpeakPcm { pcm: pcm.clone() })?;
        Ok(crate::agent_voice::speak_status_json(&pcm))
    }

    fn view_image(&self, args: &Value) -> Result<(Value, Vec<String>), String> {
        let query = arg_str(
            args,
            &[
                "message_id",
                "id",
                "name",
                "filename",
                "path",
                "file",
                "attachment",
            ],
        );
        // A chat id names its image on its own; the scoped candidate list is
        // only needed to match a file name or pick the latest image.
        let mut text_message = false;
        let by_id = query
            .as_deref()
            .and_then(|q| match self.chat_store.get_by_id(q) {
                Ok(Some(msg)) if !msg.attachment_path.is_empty() => {
                    Some((msg.attachment_path, msg.attachment_name))
                }
                Ok(Some(_)) => {
                    text_message = true;
                    None
                }
                _ => None,
            });
        // Set when nothing matched the query and the latest image stands in.
        let mut substituted_for = None;
        let (path, name) = match by_id {
            Some(hit) => hit,
            None => {
                let candidates = self.image_candidates(args)?;
                match query {
                    Some(q) => match find_named_image(&candidates, &q) {
                        Some(hit) => hit,
                        None if text_message => {
                            return Err("that message has no saved attachment".into())
                        }
                        None => {
                            // Models often pass the file name (including
                            // spaces) as message_id. Falling back to the
                            // latest image is better than failing the turn and
                            // claiming we are text-only — but say so, or the
                            // model describes the wrong picture as the one
                            // the user asked about.
                            let hit = candidates
                                .into_iter()
                                .next()
                                .ok_or_else(|| format!("no image matching '{q}'"))?;
                            substituted_for = Some(q);
                            hit
                        }
                    },
                    None => candidates
                        .into_iter()
                        .next()
                        .ok_or_else(|| "no saved image attachment to view".to_string())?,
                }
            }
        };
        let label = if name.is_empty() {
            std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone())
        } else {
            name
        };
        if !crate::ollama_module::is_vision_filename(&label)
            && !crate::ollama_module::is_vision_filename(&path)
        {
            return Err("attachment is not a raster image".into());
        }
        let b64 = crate::ollama_module::encode_image_file(std::path::Path::new(&path))?;
        info!("[ollama] view_image loaded {label}");
        let result = match substituted_for {
            Some(q) => json!({
                "ok": true,
                "name": label,
                "requested": q,
                "note": format!(
                    "no saved image matched '{q}'; attached the latest image ({label}) \
                     instead — tell the user if it is not the one they meant"
                ),
            }),
            None => json!({
                "ok": true,
                "name": label,
                "note": "image attached to the next model turn — look at it",
            }),
        };
        Ok((result, vec![b64]))
    }

    /// Recent saved images: this conversation first, then any chat.
    ///
    /// Does not treat `name` as a room id — gemma4 passes the file name there
    /// when the name has spaces. A room or peer that does not resolve falls
    /// through to the next scope instead of failing: the list is only a
    /// search space, and a mistyped scope must not hide an image the model
    /// can still find by name or id.
    fn image_candidates(&self, args: &Value) -> Result<Vec<(String, String)>, String> {
        let room_key = arg_str(args, &["room", "room_id", "room_name"]).and_then(|q| {
            let store = self.room_store.read();
            let sn = arg_str(args, &["supernode_id", "supernode"]);
            resolve_room(&store, &q, sn.as_deref())
                .ok()
                .map(|room| chat_store::room_conversation_id(&room.room_id))
        });
        let key = room_key
            .or_else(|| {
                arg_str(args, &["peer", "peer_id", "handle"])
                    .and_then(|peer| self.peer_id_for(&peer).ok())
            })
            .or_else(|| {
                self.active_conversation
                    .lock()
                    .as_deref()
                    .and_then(store_key_from_conversation)
            });
        let scoped = self
            .chat_store
            .latest_image_attachments(key.as_deref(), 8)
            .map_err(|e| e.to_string())?;
        if !scoped.is_empty() {
            return Ok(scoped);
        }
        self.chat_store
            .latest_image_attachments(None, 8)
            .map_err(|e| e.to_string())
    }

    fn enable_agent_voice_if_configured(&self) {
        let s = crate::ollama_module::read_assistant_settings();
        if !s.voice_enabled {
            return;
        }
        let _ = self.call_cmd_tx.try_send(CallCommand::SetAgentVoice(true));
        if !s.stt_model.trim().is_empty() {
            let _ = self
                .call_cmd_tx
                .try_send(CallCommand::SetListenUtterances(true));
        }
    }

    fn accept_invite(&self, args: &Value) -> Result<Value, String> {
        let url = arg_str(args, &["url", "invite_url", "invite"])
            .ok_or_else(|| "missing url".to_string())?;
        if url.trim().is_empty() {
            return Err("empty invite url".into());
        }
        self.try_conn(ConnectionCommand::AcceptInvite {
            invite_url: url.trim().to_owned(),
        })?;
        Ok(json!({ "ok": true, "queued": true, "action": "accept_invite" }))
    }

    fn refresh_rooms(&self, args: &Value) -> Result<Value, String> {
        let ids: Vec<String> = if let Some(sn) = arg_str(args, &["supernode_id", "supernode"]) {
            vec![resolve_supernode(&self.peer_store.read(), &sn)?]
        } else {
            let live = self.live.lock();
            if live.connected_supernodes.is_empty() {
                return Err("no connected supernode".into());
            }
            live.connected_supernodes.iter().cloned().collect()
        };
        for id in &ids {
            self.try_conn(ConnectionCommand::RequestRoomList {
                supernode_id: id.clone(),
            })?;
        }
        Ok(json!({ "ok": true, "queued": true, "supernodes": ids }))
    }

    fn resolve_room(&self, args: &Value) -> Result<RoomEntry, String> {
        let store = self.room_store.read();
        let sn = arg_str(args, &["supernode_id", "supernode"]);
        let query = arg_str(args, &["room", "room_id", "room_name", "name"])
            .ok_or_else(|| "missing room".to_string())?;
        resolve_room(&store, &query, sn.as_deref())
    }

    fn resolve_call_peer(&self, args: &Value) -> Result<String, String> {
        if let Some(q) = arg_str(args, &["peer", "peer_id", "handle"]) {
            return self.peer_id_for(&q);
        }
        let live = self.live.lock();
        live.incoming_call_from
            .clone()
            .or_else(|| live.active_call_peer.clone())
            .ok_or_else(|| "no peer given and no active/incoming call".into())
    }

    fn peer_id_for(&self, query: &str) -> Result<String, String> {
        let store = self.peer_store.read();
        Ok(resolve_peer(&store, query)?.peer_id.clone())
    }

    fn try_conn(&self, cmd: ConnectionCommand) -> Result<(), String> {
        self.cmd_tx
            .try_send(cmd)
            .map_err(|e| format!("connection command not queued: {e}"))
    }

    fn try_call(&self, cmd: CallCommand) -> Result<(), String> {
        self.call_cmd_tx
            .try_send(cmd)
            .map_err(|e| format!("call command not queued: {e}"))
    }
}

fn tool_schemas() -> Vec<Value> {
    vec![
        fn_tool(
            "get_status",
            "Get this client's identity, connected peers/supernodes, call and voice-room state.",
            json!({ "type": "object", "properties": {} }),
        ),
        fn_tool(
            "list_peers",
            "List trusted peers (handle, ids, connected) and supernodes.",
            json!({ "type": "object", "properties": {} }),
        ),
        fn_tool(
            "list_rooms",
            "List saved rooms (name, ids, type, whether subscribed or in voice).",
            json!({ "type": "object", "properties": {} }),
        ),
        fn_tool(
            "get_room_members",
            "Live voice and chat member lists for a room, if a roster has arrived.",
            json!({
                "type": "object",
                "properties": {
                    "room": {"type": "string", "description": "Room name or room_id"},
                    "supernode_id": {"type": "string"}
                },
                "required": ["room"]
            }),
        ),
        fn_tool(
            "get_recent_chat",
            "Recent messages in a direct chat or room.",
            json!({
                "type": "object",
                "properties": {
                    "peer": {"type": "string", "description": "Peer handle, peer_id, or public_id"},
                    "room": {"type": "string", "description": "Room name or room_id"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50}
                }
            }),
        ),
        fn_tool(
            "send_direct_chat",
            "Send a direct-message to a trusted peer.",
            json!({
                "type": "object",
                "properties": {
                    "peer": {"type": "string"},
                    "message": {"type": "string"}
                },
                "required": ["peer", "message"]
            }),
        ),
        fn_tool(
            "send_room_chat",
            "Send a text message to a room. Subscribe first if you are not already in it.",
            json!({
                "type": "object",
                "properties": {
                    "room": {"type": "string"},
                    "supernode_id": {"type": "string"},
                    "message": {"type": "string"}
                },
                "required": ["room", "message"]
            }),
        ),
        fn_tool(
            "subscribe_room_chat",
            "Subscribe to a room's text chat without joining voice.",
            json!({
                "type": "object",
                "properties": {
                    "room": {"type": "string"},
                    "supernode_id": {"type": "string"}
                },
                "required": ["room"]
            }),
        ),
        fn_tool(
            "unsubscribe_room_chat",
            "Stop receiving a room's text chat.",
            json!({
                "type": "object",
                "properties": {
                    "room": {"type": "string"},
                    "supernode_id": {"type": "string"}
                },
                "required": ["room"]
            }),
        ),
        fn_tool(
            "join_voice",
            "Join a room's voice session (and its chat). Leaves any other voice room first.",
            json!({
                "type": "object",
                "properties": {
                    "room": {"type": "string"},
                    "supernode_id": {"type": "string"}
                },
                "required": ["room"]
            }),
        ),
        fn_tool(
            "leave_voice",
            "Leave the current voice room, or a named room.",
            json!({
                "type": "object",
                "properties": {
                    "room": {"type": "string"},
                    "supernode_id": {"type": "string"}
                }
            }),
        ),
        fn_tool(
            "create_room",
            "Create a public or private room on a connected supernode.",
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "type": {"type": "string", "enum": ["public", "private"]},
                    "supernode_id": {"type": "string"}
                },
                "required": ["name"]
            }),
        ),
        fn_tool(
            "start_call",
            "Start a 1:1 voice call with a peer. Leaves room voice first.",
            json!({
                "type": "object",
                "properties": { "peer": {"type": "string"} },
                "required": ["peer"]
            }),
        ),
        fn_tool(
            "end_call",
            "Hang up the active 1:1 call, or a named peer.",
            json!({
                "type": "object",
                "properties": { "peer": {"type": "string"} }
            }),
        ),
        fn_tool(
            "accept_call",
            "Accept an incoming 1:1 call. Peer optional when only one call is ringing.",
            json!({
                "type": "object",
                "properties": { "peer": {"type": "string"} }
            }),
        ),
        fn_tool(
            "decline_call",
            "Decline an incoming 1:1 call.",
            json!({
                "type": "object",
                "properties": { "peer": {"type": "string"} }
            }),
        ),
        fn_tool(
            "accept_invite",
            "Accept a DoubleSlash invite URL (peer or supernode).",
            json!({
                "type": "object",
                "properties": { "url": {"type": "string"} },
                "required": ["url"]
            }),
        ),
        fn_tool(
            "refresh_rooms",
            "Ask connected supernodes to send their current room lists.",
            json!({
                "type": "object",
                "properties": { "supernode_id": {"type": "string"} }
            }),
        ),
        fn_tool(
            "speak",
            "Speak a short line into the current voice room or 1:1 call (Windows TTS). Text chat is silent on voice until you call this. Pass room to join_voice first if you are not already in voice.",
            json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "room": {"type": "string", "description": "Join this room's voice before speaking, if not already in voice"}
                },
                "required": ["text"]
            }),
        ),
        fn_tool(
            "view_image",
            "Load a saved chat image so you can see it. Prefer no args (latest in this conversation) or message_id from get_recent_chat (xfer-…). File names, including those with spaces, are also accepted as name/filename.",
            json!({
                "type": "object",
                "properties": {
                    "message_id": {"type": "string", "description": "Chat message id, usually xfer-…"},
                    "name": {"type": "string", "description": "Attachment file name; spaces are allowed"},
                    "filename": {"type": "string"},
                    "peer": {"type": "string"},
                    "room": {"type": "string"}
                }
            }),
        ),
    ]
}

/// Map an Ollama conversation id (`direct:` / `room:`) to a ChatStore key.
pub(crate) fn store_key_from_conversation(conv: &str) -> Option<String> {
    if let Some(rest) = conv.strip_prefix("room:") {
        Some(chat_store::room_conversation_id(rest))
    } else {
        conv.strip_prefix("direct:").map(|s| s.to_owned())
    }
}

fn fn_tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters,
        }
    })
}

fn normalize_args(arguments: &Value) -> Value {
    match arguments {
        Value::String(s) => serde_json::from_str(s).unwrap_or_else(|_| json!({"_raw": s})),
        Value::Null => json!({}),
        other => other.clone(),
    }
}

/// Match a model-supplied name or id against a saved image.
///
/// Gemma4 often puts the file name in `message_id`. Names with spaces/commas
/// (e.g. ChatGPT exports) must still match the stored attachment_name.
fn find_named_image(candidates: &[(String, String)], query: &str) -> Option<(String, String)> {
    let q = image_name_key(query);
    if q.is_empty() {
        return None;
    }
    candidates
        .iter()
        .find(|(path, name)| image_name_key(name) == q || image_name_key(path) == q)
        .cloned()
        .or_else(|| {
            candidates
                .iter()
                .find(|(path, name)| {
                    let n = image_name_key(name);
                    let p = image_name_key(path);
                    (!n.is_empty() && n.contains(&q)) || (!p.is_empty() && p.contains(&q))
                })
                .cloned()
        })
}

fn image_name_key(s: &str) -> String {
    let base = std::path::Path::new(s)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| s.to_owned());
    base.trim().to_ascii_lowercase()
}

fn arg_str(args: &Value, keys: &[&str]) -> Option<String> {
    let obj = args.as_object()?;
    for k in keys {
        if let Some(v) = obj.get(*k) {
            if let Some(s) = v.as_str() {
                let t = s.trim();
                if !t.is_empty() {
                    return Some(t.to_owned());
                }
            }
        }
    }
    None
}

fn args_has_room(args: &Value) -> bool {
    arg_str(args, &["room", "room_id", "room_name", "name"]).is_some()
}

pub(crate) fn resolve_peer<'a>(
    store: &'a PeerStore,
    query: &str,
) -> Result<&'a PeerRecord, String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("empty peer".into());
    }
    if let Some(p) = store.get(q) {
        return Ok(p);
    }
    if let Some(p) = store.get_by_identity(q) {
        return Ok(p);
    }
    let mut matches: Vec<&PeerRecord> = store
        .list_peers()
        .into_iter()
        .filter(|p| {
            p.handle.eq_ignore_ascii_case(q)
                || (q.len() >= 8 && (p.peer_id.starts_with(q) || p.identity_pub.starts_with(q)))
        })
        .collect();
    matches.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
    matches.dedup_by(|a, b| a.peer_id == b.peer_id);
    match matches.len() {
        1 => Ok(matches[0]),
        0 => Err(format!("no peer matching '{q}'")),
        n => Err(format!(
            "{n} peers match '{q}'; use a full peer_id or public_id"
        )),
    }
}

pub(crate) fn resolve_room(
    store: &RoomStore,
    query: &str,
    supernode: Option<&str>,
) -> Result<RoomEntry, String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("empty room".into());
    }
    let rooms: Vec<RoomEntry> = store
        .list()
        .into_iter()
        .filter(|r| {
            supernode.is_none_or(|sn| r.supernode_id == sn || r.supernode_id.starts_with(sn))
        })
        .cloned()
        .collect();
    if let Some(r) = rooms.iter().find(|r| r.room_id == q) {
        return Ok(r.clone());
    }
    let named: Vec<&RoomEntry> = rooms
        .iter()
        .filter(|r| r.room_name.eq_ignore_ascii_case(q))
        .collect();
    match named.len() {
        1 => Ok(named[0].clone()),
        0 => {
            let prefix: Vec<&RoomEntry> = rooms
                .iter()
                .filter(|r| q.len() >= 8 && r.room_id.starts_with(q))
                .collect();
            match prefix.len() {
                1 => Ok(prefix[0].clone()),
                0 => Err(format!("no room matching '{q}'")),
                n => Err(format!("{n} rooms match id prefix '{q}'")),
            }
        }
        n => Err(format!(
            "{n} rooms named '{q}'; pass supernode_id or room_id"
        )),
    }
}

fn resolve_supernode(store: &PeerStore, query: &str) -> Result<String, String> {
    let q = query.trim();
    if let Some(p) = store.get(q).or_else(|| store.get_by_identity(q)) {
        if p.is_supernode {
            return Ok(if p.identity_pub.is_empty() {
                p.peer_id.clone()
            } else {
                p.identity_pub.clone()
            });
        }
    }
    let hits: Vec<&PeerRecord> = store
        .supernodes()
        .into_iter()
        .filter(|p| {
            p.handle.eq_ignore_ascii_case(q)
                || p.peer_id == q
                || p.identity_pub == q
                || (q.len() >= 8 && (p.peer_id.starts_with(q) || p.identity_pub.starts_with(q)))
        })
        .collect();
    match hits.len() {
        1 => Ok(if hits[0].identity_pub.is_empty() {
            hits[0].peer_id.clone()
        } else {
            hits[0].identity_pub.clone()
        }),
        0 => Err(format!("no supernode matching '{q}'")),
        n => Err(format!("{n} supernodes match '{q}'")),
    }
}

fn truncate_result(s: &str) -> String {
    truncate_chars(s, MAX_RESULT_CHARS)
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    format!(
        "{}…",
        s.chars().take(max.saturating_sub(1)).collect::<String>()
    )
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn settings_handle() -> String {
    let path = crate::identity::Identity::default_key_dir().join("settings.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| {
            v.get("local_handle")
                .and_then(|x| x.as_str())
                .map(|s| s.to_owned())
        })
        .unwrap_or_default()
}

fn voice_activation_setting() -> bool {
    let path = crate::identity::Identity::default_key_dir().join("settings.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("voice_activation").and_then(|x| x.as_bool()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer_store::PeerRecord;

    #[test]
    fn store_key_from_conversation_maps_direct_and_room() {
        assert_eq!(
            store_key_from_conversation("room:default").as_deref(),
            Some("room:default")
        );
        assert_eq!(
            store_key_from_conversation("direct:abc123").as_deref(),
            Some("abc123")
        );
        assert!(store_key_from_conversation("other").is_none());
    }

    #[test]
    fn tool_names_are_unique_and_nonempty() {
        let tools = tool_schemas();
        assert!(tools.len() >= 10);
        let mut names = Vec::new();
        for t in &tools {
            let name = t["function"]["name"].as_str().expect("name");
            assert!(!name.is_empty());
            names.push(name.to_owned());
        }
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len());
    }

    #[test]
    fn normalize_parses_string_arguments() {
        let v = normalize_args(&Value::String(r#"{"peer":"Bob"}"#.into()));
        assert_eq!(v["peer"], "Bob");
        assert_eq!(normalize_args(&Value::Null), json!({}));
    }

    #[test]
    fn live_view_tracks_calls_and_supernodes() {
        let mut live = LiveClientView::default();
        live.apply_event(&ConnectionEvent::SupernodeConnected("sn1".into()));
        live.apply_event(&ConnectionEvent::CallRequest {
            peer_id: "p1".into(),
            fallback_supernode_id: String::new(),
            fallback_room_id: String::new(),
            fallback_invite_token: String::new(),
        });
        assert!(live.connected_supernodes.contains("sn1"));
        assert_eq!(live.incoming_call_from.as_deref(), Some("p1"));
        live.apply_event(&ConnectionEvent::CallAccepted {
            peer_id: "p1".into(),
        });
        assert_eq!(live.active_call_peer.as_deref(), Some("p1"));
        assert!(live.incoming_call_from.is_none());
        live.apply_event(&ConnectionEvent::CallEnded {
            peer_id: "p1".into(),
        });
        assert!(live.active_call_peer.is_none());
    }

    #[test]
    fn arg_str_reads_aliases() {
        let args = json!({"handle": "Ada", "body": "hi"});
        assert_eq!(arg_str(&args, &["peer", "handle"]).as_deref(), Some("Ada"));
        assert_eq!(arg_str(&args, &["message", "body"]).as_deref(), Some("hi"));
    }

    /// A tool host over empty stores in a temp dir that also holds the images.
    fn host_with_stores() -> (Arc<OllamaToolHost>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let id = Arc::new(Identity::generate());
        let peers = PeerStore::open(&id, Some(&dir.path().join("peers.dat"))).unwrap();
        let rooms = RoomStore::open(&id, Some(&dir.path().join("rooms.dat"))).unwrap();
        let chats = ChatStore::open(&id, Some(&dir.path().join("chat.db"))).unwrap();
        let (cmd_tx, _) = mpsc::channel(1);
        let (call_tx, _) = mpsc::channel(1);
        let host = OllamaToolHost::new(
            id,
            Arc::new(RwLock::new(peers)),
            Arc::new(RwLock::new(rooms)),
            Arc::new(chats),
            cmd_tx,
            call_tx,
            "me".to_owned(),
        );
        (host, dir)
    }

    /// Save a chat message in `room:default`; `name` non-empty makes it an image.
    fn save_message(host: &OllamaToolHost, dir: &std::path::Path, id: &str, name: &str) {
        let (kind, path) = if name.is_empty() {
            (chat_store::MessageKind::Text, String::new())
        } else {
            let path = dir.join(name);
            std::fs::write(&path, b"\x89PNG not really").unwrap();
            (
                chat_store::MessageKind::Image,
                path.to_string_lossy().into_owned(),
            )
        };
        host.chat_store
            .insert(&chat_store::ChatMessage {
                id: id.to_owned(),
                peer_id: "room:default".to_owned(),
                sender: "peer".to_owned(),
                recipient: "me".to_owned(),
                body: "attachment".to_owned(),
                timestamp: now_secs(),
                is_self: false,
                status: chat_store::MessageStatus::Sent,
                kind,
                attachment_name: name.to_owned(),
                attachment_path: path,
                size_str: String::new(),
                status_note: String::new(),
                sender_handle: "Peer".to_owned(),
            })
            .unwrap();
    }

    /// A room or peer the model got wrong narrows nothing; it must not fail a
    /// call whose message id or file name already names the image.
    #[test]
    fn view_image_survives_an_unresolvable_scope() {
        let (host, dir) = host_with_stores();
        save_message(&host, dir.path(), "xfer-1", "shot one.png");

        let (v, images) = host
            .view_image(&json!({"message_id": "xfer-1", "room": "no such room"}))
            .unwrap();
        assert_eq!(v["name"], "shot one.png");
        assert_eq!(images.len(), 1);

        let (v, _) = host
            .view_image(&json!({"name": "shot one.png", "peer": "nobody"}))
            .unwrap();
        assert_eq!(v["name"], "shot one.png");
    }

    /// Falling back to the latest image must say so, or the model describes
    /// it as the picture the user asked about.
    #[test]
    fn view_image_reports_a_substituted_image() {
        let (host, dir) = host_with_stores();
        save_message(&host, dir.path(), "xfer-a", "older.png");
        save_message(&host, dir.path(), "xfer-b", "newer.png");
        save_message(&host, dir.path(), "text-1", "");

        let (v, _) = host.view_image(&json!({"name": "older.png"})).unwrap();
        assert_eq!(v["name"], "older.png");
        assert!(v.get("requested").is_none());

        let (v, _) = host.view_image(&json!({"name": "missing.png"})).unwrap();
        assert_eq!(v["name"], "newer.png");
        assert_eq!(v["requested"], "missing.png");

        assert_eq!(
            host.view_image(&json!({"message_id": "text-1"}))
                .unwrap_err(),
            "that message has no saved attachment"
        );
    }

    #[test]
    fn named_image_matches_names_with_spaces() {
        let saved = "C:\\Users\\AWOL\\Downloads\\ChatGPT Image Apr 21, 2026, 10_24_40 PM.png";
        let name = "ChatGPT Image Apr 21, 2026, 10_24_40 PM.png";
        let candidates = vec![(saved.to_owned(), name.to_owned())];
        assert_eq!(
            find_named_image(&candidates, name).unwrap().1,
            name,
            "exact file name"
        );
        assert_eq!(
            find_named_image(&candidates, saved).unwrap().1,
            name,
            "full Windows path"
        );
        assert_eq!(
            find_named_image(&candidates, "ChatGPT Image Apr 21")
                .unwrap()
                .1,
            name,
            "truncated at comma still matches"
        );
        assert!(find_named_image(&candidates, "xfer-nope").is_none());
    }

    #[test]
    fn peer_record_handle_match_is_case_insensitive() {
        let a = PeerRecord {
            peer_id: "aaa111".into(),
            handle: "Bobert".into(),
            identity_pub: "pubA".into(),
            ..PeerRecord::default()
        };
        let b = PeerRecord {
            peer_id: "bbb222".into(),
            handle: "Other".into(),
            ..PeerRecord::default()
        };
        assert!(a.handle.eq_ignore_ascii_case("bobert"));
        assert!(!b.handle.eq_ignore_ascii_case("bobert"));
    }
}
