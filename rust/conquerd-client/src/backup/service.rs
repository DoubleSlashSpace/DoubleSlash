//! Shared desktop/JNI command implementation. Restore and profile selection
//! are available only before starting a session.

use super::*;
use parking_lot::Mutex;
use serde_json::{json, Value};

#[derive(Default)]
pub struct BackupService {
    pending: Mutex<Option<(String, PreparedRestore)>>,
}

#[derive(Deserialize)]
#[serde(tag = "cmd")]
enum Request {
    #[serde(rename = "backup.export")]
    Export {
        path: PathBuf,
        password: String,
        #[serde(default = "yes")]
        attachments: bool,
    },
    #[serde(rename = "backup.inspect")]
    Inspect { path: PathBuf, password: String },
    #[serde(rename = "backup.restore")]
    Restore {
        token: String,
        local_password: String,
    },
    #[serde(rename = "backup.cancel")]
    Cancel,
    #[serde(rename = "profile.list")]
    List,
    #[serde(rename = "profile.select")]
    Select { profile: String },
}

fn yes() -> bool {
    true
}

pub fn selected_profile(root: &Path) -> Result<PathBuf> {
    let selector = root.join("active-profile");
    if !selector.exists() {
        return Ok(root.to_path_buf());
    }
    let name = String::from_utf8(read_small(&selector)?)
        .map_err(|_| invalid("Invalid profile selector"))?;
    profile_path(root, &name)
}

fn profile_path(root: &Path, name: &str) -> Result<PathBuf> {
    if name == "original" {
        return Ok(root.to_path_buf());
    }
    if uuid::Uuid::parse_str(name).is_err() || name.len() != 36 {
        return Err(invalid("Invalid profile identifier"));
    }
    let path = root.join("profiles").join(name);
    let canonical = path.canonicalize()?;
    let parent = root.join("profiles").canonicalize()?;
    if canonical.parent() != Some(parent.as_path()) || !canonical.join("identity.dat").is_file() {
        return Err(invalid("Invalid profile directory"));
    }
    Ok(path)
}

fn select_profile(root: &Path, name: &str) -> Result<()> {
    let path = profile_path(root, name)?;
    if !path.join("identity.dat").is_file() {
        return Err(invalid("Profile has no identity"));
    }
    let mut temp = tempfile::NamedTempFile::new_in(root)?;
    temp.write_all(name.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(root.join("active-profile"))
        .map_err(|e| ClientError::Io(e.error))?;
    Ok(())
}

impl BackupService {
    /// Called on a worker thread. The snapshot callback acquires store locks
    /// only during capture; expensive encryption runs after those locks drop.
    pub fn run(
        &self,
        request: &str,
        root: &Path,
        session_running: bool,
        snapshot: impl FnOnce(bool) -> Result<BackupSnapshot>,
    ) -> Value {
        let result = self.execute(request, root, session_running, snapshot);
        match result {
            Ok(value) => value,
            Err(error) => json!({"ok": false, "error": error.to_string()}),
        }
    }

    fn execute(
        &self,
        request: &str,
        root: &Path,
        session_running: bool,
        snapshot: impl FnOnce(bool) -> Result<BackupSnapshot>,
    ) -> Result<Value> {
        let request: Request = serde_json::from_str(request)?;
        match request {
            Request::Export {
                path,
                password,
                attachments,
            } => {
                let password = Zeroizing::new(password);
                check_password(password.as_bytes())?;
                let summary = snapshot(attachments)?.write(&path, password.as_bytes())?;
                Ok(json!({"ok": true, "summary": summary}))
            }
            Request::Cancel => {
                self.pending.lock().take();
                Ok(json!({"ok": true}))
            }
            Request::Inspect { path, password } => {
                if session_running {
                    return Err(invalid("Lock the identity before restoring a backup"));
                }
                self.pending.lock().take();
                let password = Zeroizing::new(password);
                let prepared =
                    PreparedRestore::inspect(&path, password.as_bytes(), &root.join("profiles"))?;
                let token = uuid::Uuid::new_v4().to_string();
                let reply = json!({"ok": true, "summary": prepared.summary, "token": token});
                *self.pending.lock() = Some((token, prepared));
                Ok(reply)
            }
            Request::Restore {
                token,
                local_password,
            } => {
                if session_running {
                    return Err(invalid("Lock the identity before restoring a backup"));
                }
                let password = Zeroizing::new(local_password);
                check_password(password.as_bytes())?;
                let mut pending = self.pending.lock();
                if pending.as_ref().map(|(t, _)| t) != Some(&token) {
                    return Err(invalid("Preview the backup again before restoring"));
                }
                let (_, prepared) = pending
                    .take()
                    .ok_or_else(|| invalid("No prepared restore"))?;
                let profile = uuid::Uuid::new_v4().to_string();
                let summary =
                    prepared.commit(&root.join("profiles").join(&profile), password.as_bytes())?;
                select_profile(root, &profile)?;
                let settings = android_settings(&root.join("profiles").join(&profile));
                Ok(
                    json!({"ok": true, "summary": summary, "profile": profile, "android_settings": settings}),
                )
            }
            Request::List => {
                let mut profiles = Vec::new();
                let mut names = vec!["original".to_owned()];
                if root.join("profiles").is_dir() {
                    for entry in fs::read_dir(root.join("profiles"))? {
                        let entry = entry?;
                        if entry.file_type()?.is_dir() {
                            names.push(entry.file_name().to_string_lossy().into_owned());
                        }
                    }
                }
                for name in names {
                    let Ok(path) = profile_path(root, &name) else {
                        continue;
                    };
                    let Ok(bytes) = read_small(&path.join("identity.dat")) else {
                        continue;
                    };
                    let Ok(identity) = serde_json::from_slice::<Value>(&bytes) else {
                        continue;
                    };
                    profiles.push(json!({"profile": name, "public_id": identity["identity_pub"]}));
                }
                Ok(json!({"ok": true, "profiles": profiles}))
            }
            Request::Select { profile } => {
                if session_running {
                    return Err(invalid("Lock the identity before switching profiles"));
                }
                self.pending.lock().take();
                select_profile(root, &profile)?;
                let settings = android_settings(&profile_path(root, &profile)?);
                Ok(json!({"ok": true, "android_settings": settings}))
            }
        }
    }
}

fn android_settings(directory: &Path) -> Value {
    read_small(&directory.join("android-settings.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null)
}
