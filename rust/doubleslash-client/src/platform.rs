//! Platform integration helpers — uri_scheme, taskbar badge, ringtone, UPnP.

use tracing::{info, warn};

// Ringtone playback state (one global OutputStream keeps the audio device open)
use std::sync::Mutex;

struct RingtoneState {
    _stream: rodio::OutputStream,
    sink: rodio::Sink,
}

// SAFETY: rodio's OutputStream contains a raw pointer to a CPAL thread;
// we only access it from the calling thread and hold the mutex.
unsafe impl Send for RingtoneState {}

static RINGTONE: Mutex<Option<RingtoneState>> = Mutex::new(None);

/// Embedded ringtone OGG bytes — compile-time bundle from `assets/ringtone.ogg`.
///
/// If the asset is absent, `RINGTONE_BYTES` will be a zero-length slice and
/// `play_ringtone` will silently skip playback.
///
/// To avoid breaking the build on machines without the asset, we use a
/// build.rs `DEP_` trick; here we simply try to load at runtime.
fn load_ringtone_bytes() -> Option<Vec<u8>> {
    // Try exe-adjacent `assets/ringtone.ogg`, then cwd fallback.
    let candidates: Vec<std::path::PathBuf> = [
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("assets").join("ringtone.ogg"))),
        Some(std::path::PathBuf::from("assets/ringtone.ogg")),
    ]
    .into_iter()
    .flatten()
    .collect();
    for path in &candidates {
        if let Ok(data) = std::fs::read(path) {
            return Some(data);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// URI scheme
// ---------------------------------------------------------------------------

/// Registered custom URI schemes (`d://` and `doubleslash://`).
pub const URI_SCHEME: &str = doubleslash_features::URI_SCHEME;
pub const URI_SCHEME_ALT: &str = doubleslash_features::URI_SCHEME_ALT;

// ---------------------------------------------------------------------------
// Single-instance guard
// ---------------------------------------------------------------------------

/// Shown when a second process tries to open a profile that is already running.
pub const IDENTITY_ALREADY_RUNNING: &str = "DoubleSlash is already running for this identity.";

/// Another process already holds this profile.
#[derive(Debug)]
pub struct ProfileAlreadyRunning;

impl std::fmt::Display for ProfileAlreadyRunning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(IDENTITY_ALREADY_RUNNING)
    }
}

/// One live process per profile directory.
///
/// The lock is the profile `Identity::default_key_dir()` resolves: the
/// selected `active-profile`, or the profile root when none is selected.
/// Two directories (two identities, or `.clientA` and `.clientB`) can run
/// together. A second process for the same directory is refused.
///
/// Switching profiles in this process moves the lock. The previous profile
/// stays claimed until the new one is held, so a failed switch does not
/// leave this process with no claim.
struct HeldProfile {
    key: String,
    // Kept so the OS lock lives as long as this claim. Nothing reads it.
    _guard: ProfileGuard,
}

static HELD_PROFILE: std::sync::Mutex<Option<HeldProfile>> = std::sync::Mutex::new(None);

/// Claim `profile_dir` for this process. `Ok` means this process is the only
/// one for that directory and now holds it until exit or a later claim.
pub fn claim_profile(profile_dir: &std::path::Path) -> Result<(), ProfileAlreadyRunning> {
    let key = identity_instance_key(profile_dir);
    let mut held = HELD_PROFILE.lock().unwrap_or_else(|err| err.into_inner());
    if held.as_ref().is_some_and(|current| current.key == key) {
        return Ok(());
    }
    let guard = match acquire_profile_guard(profile_dir, &key) {
        Ok(guard) => guard,
        Err(AcquireError::Busy) => return Err(ProfileAlreadyRunning),
        Err(AcquireError::Unavailable(message)) => {
            warn!("[single-instance] could not claim this profile, continuing: {message}");
            return Ok(());
        }
    };
    *held = Some(HeldProfile { key, _guard: guard });
    Ok(())
}

/// Return `true` when this process should exit because this profile is already
/// open.
///
/// A `d://` or `doubleslash://` launch stays quiet: the shell (and Chromium's
/// external-protocol fallback inside the embedded portal) would otherwise open
/// a second window at the unlock prompt. Any other second launch is told.
pub fn should_exit_as_duplicate_instance() -> bool {
    if claim_profile(&crate::identity::Identity::default_key_dir()).is_ok() {
        return false;
    }
    if argv_has_app_url() {
        info!(
            "[single-instance] this identity is already running; \
             exiting quietly instead of opening another window for a link"
        );
    } else {
        warn!("[single-instance] {IDENTITY_ALREADY_RUNNING}");
        tell_user_identity_is_running();
    }
    true
}

fn argv_has_app_url() -> bool {
    std::env::args().any(|arg| {
        doubleslash_features::looks_like_app_url(&arg)
            || doubleslash_features::strip_scheme(&arg).is_some()
    })
}

fn tell_user_identity_is_running() {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            MessageBoxW, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MB_TOPMOST,
        };
        let text = wide(IDENTITY_ALREADY_RUNNING);
        let title = wide("DoubleSlash");
        unsafe {
            let _ = MessageBoxW(
                HWND::default(),
                PCWSTR(text.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST,
            );
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("{IDENTITY_ALREADY_RUNNING}");
    }
}

/// Stable id for a profile directory. Windows paths are compared
/// case-insensitively so `C:\Profiles\A` and `c:\profiles\a` are one identity.
pub fn identity_instance_key(profile_dir: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(
        normalize_profile_path(profile_dir).as_bytes(),
    ))
}

fn normalize_profile_path(profile_dir: &std::path::Path) -> String {
    let absolute = std::path::absolute(profile_dir).unwrap_or_else(|_| profile_dir.to_path_buf());
    let resolved = absolute.canonicalize().unwrap_or(absolute);
    let text = resolved.to_string_lossy();
    #[cfg(windows)]
    {
        let mut text = text.replace('/', "\\");
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            text = rest.to_owned();
        }
        text.trim_end_matches('\\').to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        text.into_owned()
    }
}

enum AcquireError {
    Busy,
    Unavailable(String),
}

#[cfg(windows)]
struct ProfileGuard {
    handle: isize,
}

#[cfg(windows)]
impl Drop for ProfileGuard {
    fn drop(&mut self) {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        unsafe {
            let _ = CloseHandle(HANDLE(self.handle as *mut std::ffi::c_void));
        }
    }
}

#[cfg(windows)]
fn acquire_profile_guard(
    _profile_dir: &std::path::Path,
    key: &str,
) -> Result<ProfileGuard, AcquireError> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    // Local\ keeps the name in this logon session. The hash is the profile
    // directory, so a second identity gets its own mutex.
    let name = wide(&format!("Local\\DoubleSlash-Identity-{key}"));
    unsafe {
        match CreateMutexW(None, false, PCWSTR(name.as_ptr())) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    let _ = windows::Win32::Foundation::CloseHandle(handle);
                    Err(AcquireError::Busy)
                } else {
                    Ok(ProfileGuard {
                        handle: handle.0 as isize,
                    })
                }
            }
            Err(error) => Err(AcquireError::Unavailable(error.to_string())),
        }
    }
}

#[cfg(unix)]
struct ProfileGuard {
    // Held open so the exclusive lock survives until this claim is replaced.
    _file: std::fs::File,
}

#[cfg(unix)]
fn acquire_profile_guard(
    profile_dir: &std::path::Path,
    _key: &str,
) -> Result<ProfileGuard, AcquireError> {
    use std::io::ErrorKind;
    use std::os::unix::io::AsRawFd;

    if let Err(error) = std::fs::create_dir_all(profile_dir) {
        return Err(AcquireError::Unavailable(error.to_string()));
    }
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(profile_dir.join("instance.lock"))
    {
        Ok(file) => file,
        Err(error) => return Err(AcquireError::Unavailable(error.to_string())),
    };
    // LOCK_EX | LOCK_NB. The kernel drops the lock when this file is closed.
    let rc = unsafe { flock(file.as_raw_fd(), 2 | 4) };
    if rc == 0 {
        return Ok(ProfileGuard { _file: file });
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == ErrorKind::WouldBlock {
        Err(AcquireError::Busy)
    } else {
        Err(AcquireError::Unavailable(error.to_string()))
    }
}

#[cfg(unix)]
extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

#[cfg(not(any(windows, unix)))]
struct ProfileGuard;

#[cfg(not(any(windows, unix)))]
fn acquire_profile_guard(
    _profile_dir: &std::path::Path,
    _key: &str,
) -> Result<ProfileGuard, AcquireError> {
    Ok(ProfileGuard)
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod instance_tests {
    use super::identity_instance_key;
    use std::path::Path;

    #[test]
    fn identity_instance_key_follows_the_profile_directory() {
        let root = std::env::temp_dir();
        let first = root.join("doubleslash-instance-a");
        let second = root.join("doubleslash-instance-b");
        assert_eq!(
            identity_instance_key(&first),
            identity_instance_key(Path::new(&first))
        );
        assert_ne!(
            identity_instance_key(&first),
            identity_instance_key(&second)
        );
    }

    #[cfg(windows)]
    #[test]
    fn identity_instance_key_ignores_windows_case_and_slash_style() {
        let lower = Path::new(r"C:\Users\Example\AppData\Local\DoubleSlash");
        let upper = Path::new("c:/users/example/appdata/local/doubleslash");
        assert_eq!(identity_instance_key(lower), identity_instance_key(upper));
    }
}

/// Parse a `d://` or `doubleslash://` URI into its components.
///
/// Expected format: `d://action/payload`
pub fn parse_uri(uri: &str) -> Option<(String, String)> {
    let stripped = doubleslash_features::strip_scheme(uri)?;
    let (action, payload) = stripped.split_once('/').unwrap_or((stripped, ""));
    Some((action.to_string(), payload.to_string()))
}

/// Register the `d://` / `doubleslash://` URI scheme handlers with the OS.
///
/// - **Windows**: writes to `HKCU\Software\Classes\d\...` and `...\doubleslash\...`
/// - **macOS**: handled via `Info.plist` `CFBundleURLTypes` at build time
/// - **Linux**: installs a `.desktop` file (see `packaging/install_uri_scheme.sh`)
///
/// Returns `true` if successful. Failures are non-fatal (logged only).
pub fn register_uri_scheme() -> bool {
    #[cfg(target_os = "windows")]
    {
        register_uri_scheme_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        // macOS: done via Info.plist; Linux: done at install time
        info!("URI scheme registration skipped on this platform");
        false
    }
}

#[cfg(target_os = "windows")]
fn register_uri_scheme_windows() -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    // Use reg.exe to add to HKCU\Software\Classes
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut any_ok = false;
    for scheme in [URI_SCHEME_ALT, URI_SCHEME] {
        let base = format!("HKCU\\Software\\Classes\\{scheme}");
        let ok = Command::new("reg")
            .args(["add", &base, "/ve", "/d", "URL:DoubleSlash Protocol", "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            continue;
        }
        let cmd_key = format!("{base}\\shell\\open\\command");
        Command::new("reg")
            .args([
                "add",
                &cmd_key,
                "/ve",
                "/d",
                &format!("\"{exe}\" \"%1\""),
                "/f",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        Command::new("reg")
            .args(["add", &base, "/v", "URL Protocol", "/d", "", "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map(|s| s.success())
            .unwrap_or(ok);
        any_ok = true;
    }
    any_ok
}

// ---------------------------------------------------------------------------
// Taskbar badge
// ---------------------------------------------------------------------------

/// Set the taskbar unread-message badge count.
///
/// - **Windows**: Win32 `ITaskbarList3::SetOverlayIcon` via windows-rs
/// - **macOS**: `NSApp.dockTile.badgeLabel` (not yet implemented)
/// - **Linux**: `UnityLauncherEntry.count` via D-Bus (not yet implemented)
pub fn set_taskbar_badge(count: u32) {
    #[cfg(target_os = "windows")]
    {
        set_taskbar_badge_windows(count);
    }
    #[cfg(target_os = "macos")]
    {
        // Simple implementation via osascript (no extra deps)
        let script = format!(
            r#"tell application "System Events" to set the dock tile of process "DoubleSlash" to {{label:"{}"}}"#,
            count
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        // Basic D-Bus signal for Unity/GNOME taskbar count via dbus-send
        let _ = std::process::Command::new("dbus-send")
            .args([
                "--session",
                "--dest=com.canonical.Unity",
                "--type=method_call",
                "/com/canonical/unity/launcherentry/1",
                "com.canonical.Unity.LauncherEntry.Update",
                "string:application://doubleslash.desktop",
                &format!("variant:{{'count': <int64:{}>}}", count as i64),
            ])
            .spawn();
    }
    // Android has no desktop taskbar — the unread count reaches the user
    // through the notification channel instead, which the Kotlin layer owns.
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = count;
    }
}

/// Clear the taskbar badge.
pub fn clear_taskbar_badge() {
    set_taskbar_badge(0);
}

/// Reveal `path` in the system file manager, selecting the file when possible.
///
/// `path` is a local filesystem path (not a `file://` URL). An empty path is
/// refused; a missing file still opens its parent directory.
pub fn open_containing_folder(path: &str) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("empty path".into());
    }
    let p = std::path::Path::new(path);
    #[cfg(target_os = "windows")]
    {
        let mut cmd = std::process::Command::new(system_file_manager());
        if p.is_file() {
            cmd.arg(explorer_select_arg(p));
        } else if p.is_dir() {
            cmd.arg(p.as_os_str());
        } else if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
            cmd.arg(dir.as_os_str());
        } else {
            cmd.arg(p.as_os_str());
        }
        cmd.spawn()
            .map(|_| ())
            .map_err(|e| format!("could not open folder: {e}"))
    }
    #[cfg(target_os = "macos")]
    {
        // Absolute: CreateProcess-style PATH lookup is not a concern here, but
        // pinning the system binary costs nothing and removes the question.
        let mut cmd = std::process::Command::new("/usr/bin/open");
        if p.exists() {
            cmd.args(["-R", path]);
        } else if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
            cmd.arg(dir.as_os_str());
        } else {
            cmd.arg(path);
        }
        cmd.spawn()
            .map(|_| ())
            .map_err(|e| format!("could not open folder: {e}"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let dir = if p.is_dir() {
            p
        } else {
            p.parent()
                .filter(|d| !d.as_os_str().is_empty())
                .unwrap_or(p)
        };
        std::process::Command::new("xdg-open")
            .arg(dir.as_os_str())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not open folder: {e}"))
    }
}

/// `explorer /select,PATH` argument — no space after the comma.
#[cfg(any(test, target_os = "windows"))]
fn explorer_select_arg(path: &std::path::Path) -> String {
    format!("/select,{}", path.display())
}

/// Absolute path to Explorer.
///
/// `Command::new("explorer")` goes through `CreateProcessW`, whose search
/// order includes the current directory — so an `explorer.exe` dropped in
/// whatever directory the app happens to be running from would win. Resolve it
/// under `%SystemRoot%` instead, falling back to the bare name only if the
/// environment has no usable `SystemRoot`.
#[cfg(any(test, target_os = "windows"))]
fn system_file_manager() -> std::path::PathBuf {
    match std::env::var_os("SystemRoot").filter(|r| !r.is_empty()) {
        Some(root) => std::path::Path::new(&root).join("explorer.exe"),
        None => std::path::PathBuf::from("explorer.exe"),
    }
}

#[cfg(target_os = "windows")]
fn set_taskbar_badge_windows(count: u32) {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList};
    use windows::Win32::UI::WindowsAndMessaging::HICON;

    unsafe {
        // COM must be initialized on the calling thread.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let taskbar: ITaskbarList3 = match CoCreateInstance(&TaskbarList, None, CLSCTX_ALL) {
            Ok(t) => t,
            Err(e) => {
                warn!("Taskbar badge: CoCreateInstance failed: {e}");
                return;
            }
        };

        // Null HWND: targets the foreground window of the calling process
        let hwnd = HWND(0isize as _);
        if count == 0 {
            let _ = taskbar.SetOverlayIcon(hwnd, HICON(0isize as _), PCWSTR::null());
        } else {
            let hicon = create_badge_icon(count);
            let tip: Vec<u16> = "Unread messages\0".encode_utf16().collect();
            let _ = taskbar.SetOverlayIcon(hwnd, hicon, PCWSTR(tip.as_ptr()));
            if hicon.0 as usize != 0 {
                let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(hicon);
            }
        }
    }
}

#[cfg(target_os = "windows")]
unsafe fn create_badge_icon(count: u32) -> windows::Win32::UI::WindowsAndMessaging::HICON {
    use windows::Win32::Foundation::{COLORREF, HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush, DeleteDC, DeleteObject,
        FillRect, GetDC, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TextOutW, TRANSPARENT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

    const SIZE: i32 = 16;
    let hdc_screen = GetDC(HWND(0isize as _));
    let hdc = CreateCompatibleDC(hdc_screen);
    let hbm = CreateCompatibleBitmap(hdc_screen, SIZE, SIZE);
    let _ = SelectObject(hdc, hbm);
    let _ = ReleaseDC(HWND(0isize as _), hdc_screen);

    // Red background (BGR)
    let brush = CreateSolidBrush(COLORREF(0x0000CC));
    let rect = RECT {
        left: 0,
        top: 0,
        right: SIZE,
        bottom: SIZE,
    };
    let _ = FillRect(hdc, &rect, brush);
    let _ = DeleteObject(brush);

    // White count text
    let _ = SetBkMode(hdc, TRANSPARENT);
    let _ = SetTextColor(hdc, COLORREF(0x00FFFFFF));
    let label: Vec<u16> = if count < 100 {
        format!("{count}\0").encode_utf16().collect()
    } else {
        "99+\0".encode_utf16().collect()
    };
    let _ = TextOutW(hdc, 2, 2, &label);

    let mask = CreateCompatibleBitmap(hdc_screen, SIZE, SIZE);
    let info = ICONINFO {
        fIcon: windows::Win32::Foundation::BOOL(1),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask,
        hbmColor: hbm,
    };
    let hicon = CreateIconIndirect(&info).unwrap_or(HICON(0isize as _));
    let _ = DeleteObject(mask);
    let _ = DeleteObject(hbm);
    let _ = DeleteDC(hdc);
    hicon
}

// ---------------------------------------------------------------------------
// Ringtone
// ---------------------------------------------------------------------------

/// Play the incoming-call ringtone.
///
/// Returns immediately; playback runs on a background thread managed by rodio.
/// Looks for `assets/ringtone.ogg` next to the executable (or in cwd).
pub fn play_ringtone() {
    let Some(bytes) = load_ringtone_bytes() else {
        info!("Ringtone: asset not found, skipping playback");
        return;
    };
    let Ok((stream, handle)) = rodio::OutputStream::try_default() else {
        warn!("Ringtone: could not open audio output");
        return;
    };
    let Ok(sink) = rodio::Sink::try_new(&handle) else {
        warn!("Ringtone: could not create sink");
        return;
    };
    let cursor = std::io::Cursor::new(bytes);
    match rodio::Decoder::new(cursor) {
        Ok(decoder) => {
            sink.append(decoder);
            sink.set_volume(0.8);
        }
        Err(e) => {
            warn!("Ringtone: decode error: {e}");
            return;
        }
    }
    let state = RingtoneState {
        _stream: stream,
        sink,
    };
    if let Ok(mut guard) = RINGTONE.lock() {
        *guard = Some(state);
    }
    info!("Ringtone: playing");
}

/// Stop the currently playing ringtone.
pub fn stop_ringtone() {
    if let Ok(mut guard) = RINGTONE.lock() {
        if let Some(state) = guard.take() {
            state.sink.stop();
        }
    }
    info!("Ringtone: stopped");
}

// ---------------------------------------------------------------------------
// Push-to-talk
// ---------------------------------------------------------------------------

/// Start a PTT polling thread. Returns a stop flag — set it to `true` to halt.
///
/// The `muted_tx` channel receives `true` (muted) when the key is NOT held
/// and `false` (unmuted) when the key IS held. Polls at ~60 Hz.
pub fn start_ptt_polling(
    key_name: String,
    muted_tx: std::sync::mpsc::SyncSender<bool>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<()> {
    match std::thread::Builder::new()
        .name("doubleslash-ptt".into())
        .spawn(move || {
            #[cfg(target_os = "windows")]
            let vk = key_name_to_vk_windows(&key_name);
            #[cfg(not(target_os = "windows"))]
            let _ = key_name;

            let mut prev_muted = true;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                #[cfg(target_os = "windows")]
                let pressed = if vk != 0 {
                    unsafe {
                        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
                        (GetAsyncKeyState(vk) as u16 & 0x8000) != 0
                    }
                } else {
                    false
                };
                #[cfg(not(target_os = "windows"))]
                let pressed = false;

                let muted = !pressed;
                if muted != prev_muted {
                    prev_muted = muted;
                    let _ = muted_tx.try_send(muted);
                }
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
        }) {
        Ok(handle) => handle,
        Err(e) => {
            tracing::error!("failed to spawn PTT thread: {e}");
            std::thread::Builder::new()
                .spawn(|| {})
                .unwrap_or_else(|_| std::process::abort())
        }
    }
}

#[cfg(target_os = "windows")]
fn key_name_to_vk_windows(name: &str) -> i32 {
    match name.to_lowercase().trim() {
        "space" => 0x20,
        "f1" => 0x70,
        "f2" => 0x71,
        "f3" => 0x72,
        "f4" => 0x73,
        "f5" => 0x74,
        "f6" => 0x75,
        "f7" => 0x76,
        "f8" => 0x77,
        "f9" => 0x78,
        "f10" => 0x79,
        "f11" => 0x7A,
        "f12" => 0x7B,
        "ctrl" | "control" | "lctrl" => 0x11,
        "alt" | "lalt" => 0x12,
        "shift" | "lshift" => 0x10,
        "capslock" => 0x14,
        "tab" => 0x09,
        "enter" | "return" => 0x0D,
        "backspace" => 0x08,
        "delete" | "del" => 0x2E,
        "insert" | "ins" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" | "pgup" => 0x21,
        "pagedown" | "pgdn" => 0x22,
        "left" => 0x25,
        "right" => 0x27,
        "up" => 0x26,
        "down" => 0x28,
        "esc" | "escape" => 0x1B,
        "numlock" => 0x90,
        "scrolllock" => 0x91,
        // Mouse buttons (GetAsyncKeyState works with VK_*BUTTON codes)
        "mouse1" | "lmb" => 0x01, // VK_LBUTTON
        "mouse2" | "rmb" => 0x02, // VK_RBUTTON
        "mouse3" | "mmb" => 0x04, // VK_MBUTTON
        "mouse4" | "xb1" => 0x05, // VK_XBUTTON1 (Back)
        "mouse5" | "xb2" => 0x06, // VK_XBUTTON2 (Forward)
        s if s.len() == 1 => s
            .chars()
            .next()
            .and_then(|ch| ch.to_uppercase().next())
            .map(|c| c as i32)
            .unwrap_or(0),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Desktop notifications
// ---------------------------------------------------------------------------

/// Show a transient desktop notification (non-blocking, best-effort).
pub fn show_notification(title: &str, body: &str) {
    #[cfg(target_os = "windows")]
    show_notification_windows(title, body);
    #[cfg(target_os = "linux")]
    show_notification_linux(title, body);
    #[cfg(target_os = "macos")]
    show_notification_macos(title, body);
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = (title, body);
    }
}

#[cfg(target_os = "windows")]
fn show_notification_windows(title: &str, body: &str) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    // Use Windows Forms NotifyIcon balloon tip via PowerShell — always works
    // on Windows 7+ without app registration. Spawned async so never blocks.
    let title = title.replace('"', "");
    let body = body.replace('"', "");
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms;\
         $n=New-Object System.Windows.Forms.NotifyIcon;\
         $n.Icon=[System.Drawing.SystemIcons]::Information;\
         $n.Visible=$true;\
         $n.ShowBalloonTip(4000,\"{title}\",\"{body}\",[System.Windows.Forms.ToolTipIcon]::None);\
         Start-Sleep -Seconds 5;$n.Visible=$false;$n.Dispose()"
    );
    let _ = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

#[cfg(target_os = "linux")]
fn show_notification_linux(title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "DoubleSlash", "-t", "4000", title, body])
        .spawn();
}

#[cfg(target_os = "macos")]
fn show_notification_macos(title: &str, body: &str) {
    let script = format!(
        "display notification {} with title {} subtitle \"DoubleSlash\"",
        serde_json::to_string(body).unwrap_or_default(),
        serde_json::to_string(title).unwrap_or_default()
    );
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .spawn();
}

// ---------------------------------------------------------------------------
// UPnP port mapping
// ---------------------------------------------------------------------------

/// Request a UPnP port mapping for the local QUIC listener.
///
/// Returns `Some(external_port)` on success or `None` if UPnP is unavailable.
pub async fn request_upnp_mapping(local_port: u16) -> Option<u16> {
    use futures_util::{pin_mut, StreamExt};
    use rupnp::ssdp::{SearchTarget, URN};

    info!("UPnP: searching for IGD gateway...");

    let search_target = SearchTarget::RootDevice;
    let devices = match rupnp::discover(&search_target, std::time::Duration::from_secs(3)).await {
        Ok(d) => d,
        Err(e) => {
            info!("UPnP: SSDP search failed: {}", e);
            return None;
        }
    };
    pin_mut!(devices);

    let wan_ip_urn = URN::service("schemas-upnp-org", "WANIPConnection", 1);
    let wan_ppp_urn = URN::service("schemas-upnp-org", "WANPPPConnection", 1);

    while let Some(device) = devices.next().await {
        match device {
            Ok(dev) => {
                if let Some(external) =
                    try_add_port_mapping(&dev, &wan_ip_urn, &wan_ppp_urn, local_port).await
                {
                    info!("UPnP: successfully mapped {} -> {}", external, local_port);
                    return Some(external);
                }
            }
            Err(e) => warn!("UPnP: device discovery error: {}", e),
        }
    }

    info!("UPnP: no suitable IGD found");
    None
}

async fn try_add_port_mapping(
    device: &rupnp::Device,
    wan_ip_urn: &rupnp::ssdp::URN,
    wan_ppp_urn: &rupnp::ssdp::URN,
    local_port: u16,
) -> Option<u16> {
    let service = device
        .find_service(wan_ip_urn)
        .or_else(|| device.find_service(wan_ppp_urn))?;

    let description = "DoubleSlash QUIC";
    let internal_client = local_ip().unwrap_or_default();

    match add_port_mapping(
        service,
        device.url(),
        local_port,
        local_port,
        &internal_client,
        description,
    )
    .await
    {
        Ok(_) => Some(local_port),
        Err(e) => {
            warn!("UPnP: AddPortMapping failed on first try: {}", e);
            for offset in 1..10 {
                let Some(try_port) = local_port.checked_add(offset) else {
                    continue;
                };
                if add_port_mapping(
                    service,
                    device.url(),
                    local_port,
                    try_port,
                    &internal_client,
                    description,
                )
                .await
                .is_ok()
                {
                    return Some(try_port);
                }
            }
            None
        }
    }
}

async fn add_port_mapping(
    service: &rupnp::Service,
    device_url: &rupnp::http::Uri,
    local_port: u16,
    external_port: u16,
    internal_client: &str,
    description: &str,
) -> Result<std::collections::HashMap<String, String>, rupnp::Error> {
    let payload = format!(
        "<NewRemoteHost></NewRemoteHost>\
         <NewExternalPort>{external_port}</NewExternalPort>\
         <NewProtocol>UDP</NewProtocol>\
         <NewInternalPort>{local_port}</NewInternalPort>\
         <NewInternalClient>{internal_client}</NewInternalClient>\
         <NewEnabled>1</NewEnabled>\
         <NewPortMappingDescription>{description}</NewPortMappingDescription>\
         <NewLeaseDuration>0</NewLeaseDuration>"
    );
    service.action(device_url, "AddPortMapping", &payload).await
}

pub(crate) fn local_ip() -> Option<String> {
    // Simple best-effort local IP (for UPnP internal client)
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip().to_string())
}

/// Release a UPnP port mapping previously obtained with [`request_upnp_mapping`].
pub async fn release_upnp_mapping(external_port: u16) {
    info!("UPnP: releasing mapping for port {} (stub)", external_port);
}

// ---------------------------------------------------------------------------
// Desktop shortcuts
// ---------------------------------------------------------------------------

/// Create Start Menu and Desktop shortcuts for the DoubleSlash executable.
///
/// **Windows only** — no-op on other platforms.  Uses PowerShell
/// `WScript.Shell` COM to write `.lnk` files into the user's Desktop and
/// `%APPDATA%\Microsoft\Windows\Start Menu\Programs`; no elevation is
/// required.
pub fn create_desktop_shortcuts() {
    #[cfg(target_os = "windows")]
    create_shortcuts_windows();
    #[cfg(not(target_os = "windows"))]
    info!("Desktop shortcuts creation skipped — not supported on this platform");
}

/// Remove the Start Menu and Desktop shortcuts created by
/// [`create_desktop_shortcuts`].
pub fn remove_desktop_shortcuts() {
    #[cfg(target_os = "windows")]
    remove_shortcuts_windows();
    #[cfg(not(target_os = "windows"))]
    info!("Desktop shortcuts removal skipped — not supported on this platform");
}

/// Return `true` if at least one DoubleSlash shortcut (Desktop or Start Menu)
/// already exists.  **Windows only** — always returns `false` on other
/// platforms.
pub fn has_desktop_shortcuts() -> bool {
    #[cfg(target_os = "windows")]
    {
        has_shortcuts_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

#[cfg(target_os = "windows")]
fn has_shortcuts_windows() -> bool {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let start_menu = std::path::Path::new(&appdata)
        .join(r"Microsoft\Windows\Start Menu\Programs\DoubleSlash.lnk");

    let userprofile = std::env::var("USERPROFILE").unwrap_or_default();
    let desktop = std::path::Path::new(&userprofile).join(r"Desktop\DoubleSlash.lnk");

    start_menu.exists() || desktop.exists()
}

#[cfg(target_os = "windows")]
fn create_shortcuts_windows() {
    use std::os::windows::process::CommandExt;
    let exe = match std::env::current_exe() {
        Ok(p) => p.to_string_lossy().replace('/', "\\"),
        Err(e) => {
            warn!("Shortcuts: could not determine exe path: {e}");
            return;
        }
    };
    // PowerShell WScript.Shell — targets user-local paths, no elevation needed.
    let script = format!(
        r#"$ws = New-Object -ComObject WScript.Shell; \
$paths = @( \
    [Environment]::GetFolderPath('Desktop'), \
    [Environment]::GetFolderPath('Programs') \
); \
foreach ($dir in $paths) {{ \
    if (-not (Test-Path $dir)) {{ continue }}; \
    $lnk = $ws.CreateShortcut("$dir\DoubleSlash.lnk"); \
    $lnk.TargetPath = "{exe}"; \
    $lnk.Description = "DoubleSlash — Privacy-first peer connectivity"; \
    $lnk.Save(); \
}}"#
    );
    let _ = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .creation_flags(0x08000000u32)
        .spawn();
    info!("Shortcuts: Start Menu and Desktop shortcuts created");
}

#[cfg(target_os = "windows")]
fn remove_shortcuts_windows() {
    use std::os::windows::process::CommandExt;
    let script = r#"$paths = @( \
    [Environment]::GetFolderPath('Desktop'), \
    [Environment]::GetFolderPath('Programs') \
); \
foreach ($dir in $paths) { \
    $lnk = "$dir\DoubleSlash.lnk"; \
    if (Test-Path $lnk) { Remove-Item $lnk -Force } \
}"#;
    let _ = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            script,
        ])
        .creation_flags(0x08000000u32)
        .spawn();
    info!("Shortcuts: Start Menu and Desktop shortcuts removed");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uri_invite() {
        let (action, payload) = parse_uri("d://invite/abc123def456").unwrap();
        assert_eq!(action, "invite");
        assert_eq!(payload, "abc123def456");
        let (action, payload) = parse_uri("doubleslash://invite/abc123def456").unwrap();
        assert_eq!(action, "invite");
        assert_eq!(payload, "abc123def456");
    }

    #[test]
    fn parse_uri_no_payload() {
        let (action, payload) = parse_uri("d://open").unwrap();
        assert_eq!(action, "open");
        assert_eq!(payload, "");
    }

    #[test]
    fn parse_uri_invalid_scheme() {
        assert!(parse_uri("https://example.com").is_none());
    }

    #[test]
    fn open_containing_folder_rejects_empty() {
        assert!(open_containing_folder("").is_err());
        assert!(open_containing_folder("   ").is_err());
    }

    /// Explorer must be resolved absolutely: `CreateProcessW` searches the
    /// current directory, so a bare name is a binary-planting target.
    #[test]
    fn system_file_manager_is_absolute_under_system_root() {
        let mgr = system_file_manager();
        if std::env::var_os("SystemRoot").is_some_and(|r| !r.is_empty()) {
            assert!(mgr.is_absolute(), "{}", mgr.display());
        }
        assert!(mgr.ends_with("explorer.exe"), "{}", mgr.display());
    }

    #[test]
    fn explorer_select_arg_has_no_space_after_comma() {
        let p = std::path::Path::new("C:\\Downloads\\clip.bin");
        let arg = explorer_select_arg(p);
        assert!(arg.starts_with("/select,"), "{arg}");
        assert!(!arg.starts_with("/select, "), "{arg}");
        assert!(arg.ends_with("clip.bin"), "{arg}");
    }
}
