//! Live acceptance driver for hosting a supernode: runs the same
//! `AdminService` requests the desktop UI sends, against a real server.
//!
//! Uses its own store in `DS_ADMIN_DIR` (keyed by a random `store.key` there),
//! never a real profile. The server password comes from `DS_ADMIN_PASSWORD`
//! only, and is never printed.
//!
//! ```text
//! DS_ADMIN_DIR=/tmp/admin-test DS_ADMIN_HOST=203.0.113.10 DS_ADMIN_PASSWORD=… \
//!   cargo run --example supernode_admin_live --features supernode-admin -- deploy
//! cargo run --example supernode_admin_live --features supernode-admin -- status
//! ```
//!
//! Commands: `fingerprint`, `deploy`, `state`, `status`, `check_update`,
//! `update_now`, `restart`, `logs`, `set_update_mode <server-timer|client>`,
//! `uninstall [purge]`, `forget`. Node commands act on the first stored node.
//! `deploy` reads `DS_ADMIN_ACCESS` (default `client-key-only`) and
//! `DS_ADMIN_UPDATE` (default `server-timer`), and confirms the host key it
//! prints without asking: compare it with the provider's fingerprint.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use doubleslash_client::supernode_admin::{AdminService, ManagedNodeStore, Request};
use serde_json::{json, Value};

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn required(name: &str) -> Result<String, String> {
    env(name).ok_or_else(|| format!("set {name}"))
}

fn store_key(dir: &Path) -> Result<[u8; 32], String> {
    let path = dir.join("store.key");
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(key) = <[u8; 32]>::try_from(bytes.as_slice()) {
            return Ok(key);
        }
    }
    let key: [u8; 32] = rand::random();
    std::fs::write(&path, key).map_err(|e| format!("write store.key: {e}"))?;
    Ok(key)
}

fn print_event(event: Value) {
    match event["kind"].as_str() {
        Some("plan") => {
            println!("plan:");
            for title in event["steps"].as_array().into_iter().flatten() {
                println!("  - {}", title.as_str().unwrap_or_default());
            }
        }
        Some("step") => {
            let state = event["state"].as_str().unwrap_or_default();
            println!(
                "[{state:>7}] {}",
                event["title"].as_str().unwrap_or_default()
            );
            for line in event["lines"].as_array().into_iter().flatten() {
                println!("           {}", line.as_str().unwrap_or_default());
            }
            if let Some(error) = event["error"].as_str() {
                println!("           error: {error}");
            }
        }
        _ => println!("{event}"),
    }
}

async fn run(service: &AdminService, request: Value) -> Result<Value, String> {
    let request: Request = serde_json::from_value(request).map_err(|e| e.to_string())?;
    service.handle(request, &print_event).await
}

async fn command(service: &AdminService, args: &[String]) -> Result<Value, String> {
    let node_id = || -> Result<String, String> {
        service.state()["nodes"][0]["id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "no managed node in this store".to_string())
    };
    let port: u16 = env("DS_ADMIN_PORT")
        .and_then(|p| p.parse().ok())
        .unwrap_or(22);
    match args.first().map(String::as_str).unwrap_or("state") {
        "fingerprint" => {
            let host = required("DS_ADMIN_HOST")?;
            run(
                service,
                json!({"op": "fetch_host_key", "host": host, "port": port}),
            )
            .await
        }
        "deploy" => {
            let host = required("DS_ADMIN_HOST")?;
            let key = run(
                service,
                json!({"op": "fetch_host_key", "host": host, "port": port}),
            )
            .await?;
            println!(
                "host key {}",
                key["fingerprint"].as_str().unwrap_or_default()
            );
            run(service, json!({"op": "accept_notice"})).await?;
            run(
                service,
                json!({
                    "op": "deploy",
                    "name": env("DS_ADMIN_NAME").unwrap_or_else(|| "Live test".into()),
                    "host": host,
                    "port": port,
                    "user": env("DS_ADMIN_USER").unwrap_or_else(|| "root".into()),
                    "public_host": env("DS_ADMIN_PUBLIC").unwrap_or_default(),
                    "host_key": key["host_key"],
                    "password": env("DS_ADMIN_PASSWORD"),
                    "access": env("DS_ADMIN_ACCESS").unwrap_or_else(|| "client-key-only".into()),
                    "update_mode": env("DS_ADMIN_UPDATE").unwrap_or_else(|| "server-timer".into()),
                }),
            )
            .await
        }
        "state" => Ok(service.state()),
        "set_update_mode" => {
            let mode = args.get(1).cloned().unwrap_or_else(|| "client".into());
            run(
                service,
                json!({"op": "set_update_mode", "node_id": node_id()?, "mode": mode}),
            )
            .await
        }
        "uninstall" => {
            let purge = args.get(1).map(String::as_str) == Some("purge");
            run(
                service,
                json!({"op": "uninstall", "node_id": node_id()?, "purge": purge}),
            )
            .await
        }
        "logs" => {
            run(
                service,
                json!({"op": "logs", "node_id": node_id()?, "lines": 80}),
            )
            .await
        }
        op @ ("status" | "check_update" | "update_now" | "restart" | "forget") => {
            run(service, json!({"op": op, "node_id": node_id()?})).await
        }
        other => Err(format!("unknown command {other}")),
    }
}

async fn main_result() -> Result<Value, String> {
    let dir = PathBuf::from(required("DS_ADMIN_DIR")?);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create DS_ADMIN_DIR: {e}"))?;
    let key = store_key(&dir)?;
    let store = ManagedNodeStore::open_with_key(&key, &dir.join("managed_supernodes.dat"))
        .map_err(|e| format!("open store: {e}"))?;
    let service = AdminService::new(store, dir);
    let args: Vec<String> = std::env::args().skip(1).collect();
    command(&service, &args).await
}

#[tokio::main]
async fn main() -> ExitCode {
    match main_result().await {
        Ok(value) => {
            match value["text"].as_str() {
                Some(text) => println!("{text}"),
                None => println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                ),
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
