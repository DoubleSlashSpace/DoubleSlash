//! Audio device listing and OS-default follow.
//!
//! Settings "Default" is a UI sentinel, not a CPAL device name. The real
//! endpoint list must not include that label, or the combo box shows it twice.
//! An empty (or `"Default"`) selection means follow the host default, including
//! when Windows changes it while a call is already open.

use std::collections::HashSet;

use cpal::traits::{DeviceTrait, HostTrait};

/// Combo-box label for "use the OS default". Not a device name.
pub const DEFAULT_DEVICE_LABEL: &str = "Default";

/// Stable identity of the host's current console default endpoints.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultEndpointIds {
    pub input: Option<String>,
    pub output: Option<String>,
}

/// Notification that the OS console default input and/or output changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OsDefaultChange {
    pub input: bool,
    pub output: bool,
}

/// Treat empty / `"Default"` as "follow the OS default".
pub fn owned_device_selection(name: Option<String>) -> Option<String> {
    let trimmed = name?.trim().to_string();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(DEFAULT_DEVICE_LABEL) {
        None
    } else {
        Some(trimmed)
    }
}

/// Drop empty names, the `"Default"` sentinel, and duplicates (first wins).
pub fn sanitize_device_names(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for name in names {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(DEFAULT_DEVICE_LABEL) {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

/// Real input/output device names for the settings combo boxes.
///
/// Does not prepend [`DEFAULT_DEVICE_LABEL`]; QML adds that row itself.
pub fn list_host_device_names() -> (Vec<String>, Vec<String>) {
    let host = cpal::default_host();
    let inputs = host
        .input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect::<Vec<_>>())
        .unwrap_or_default();
    let outputs = host
        .output_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect::<Vec<_>>())
        .unwrap_or_default();
    (
        sanitize_device_names(inputs),
        sanitize_device_names(outputs),
    )
}

/// Whether a live pipeline that may be following OS defaults should reopen.
pub fn should_reopen_for_os_default(
    follow_input: bool,
    follow_output: bool,
    input_changed: bool,
    output_changed: bool,
) -> bool {
    (follow_input && input_changed) || (follow_output && output_changed)
}

/// Current console-default endpoint identities.
///
/// On Windows this is the WASAPI device id (`IMMDevice::GetId`) for the
/// `eConsole` role, which stays stable when the friendly name does not. Other
/// platforms fall back to the CPAL default device name.
pub fn query_default_endpoint_ids() -> DefaultEndpointIds {
    #[cfg(target_os = "windows")]
    {
        windows_impl::query_console_defaults()
    }
    #[cfg(not(target_os = "windows"))]
    {
        cpal_name_defaults()
    }
}

#[cfg(not(target_os = "windows"))]
fn cpal_name_defaults() -> DefaultEndpointIds {
    let host = cpal::default_host();
    DefaultEndpointIds {
        input: host.default_input_device().and_then(|d| d.name().ok()),
        output: host.default_output_device().and_then(|d| d.name().ok()),
    }
}

/// Watch the OS default device. `None` on platforms without a notification API;
/// the call controller still polls in that case.
pub fn spawn_default_device_watch(
    tx: tokio::sync::mpsc::UnboundedSender<OsDefaultChange>,
) -> Option<DefaultDeviceWatch> {
    #[cfg(target_os = "windows")]
    {
        windows_impl::spawn_watch(tx)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = tx;
        None
    }
}

/// Handle that unregisters the Windows endpoint callback on drop.
pub struct DefaultDeviceWatch {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for DefaultDeviceWatch {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::{DefaultDeviceWatch, DefaultEndpointIds, OsDefaultChange};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::sync::Arc;
    use std::time::Duration;

    use windows::core::implement;
    use windows::Win32::Media::Audio::{
        eCapture, eConsole, eRender, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
        IMMNotificationClient_Impl, MMDeviceEnumerator, DEVICE_STATE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

    pub fn query_console_defaults() -> DefaultEndpointIds {
        // Already-initialised is not an error: Qt or Media Foundation may own
        // the apartment on this thread.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        DefaultEndpointIds {
            input: default_endpoint_id(eCapture),
            output: default_endpoint_id(eRender),
        }
    }

    fn default_endpoint_id(flow: EDataFlow) -> Option<String> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let device = enumerator.GetDefaultAudioEndpoint(flow, eConsole).ok()?;
            let id = device.GetId().ok()?;
            pwstr_to_string(id)
        }
    }

    fn pwstr_to_string(id: windows::core::PWSTR) -> Option<String> {
        if id.is_null() {
            return None;
        }
        let text = unsafe { id.to_string() }.ok();
        unsafe {
            CoTaskMemFree(Some(id.0 as *const std::ffi::c_void));
        }
        text.filter(|s| !s.is_empty())
    }

    pub fn spawn_watch(
        tx: tokio::sync::mpsc::UnboundedSender<OsDefaultChange>,
    ) -> Option<DefaultDeviceWatch> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = Arc::clone(&stop);
        let (wake_tx, wake_rx) = mpsc::sync_channel::<()>(4);
        let thread = std::thread::Builder::new()
            .name("doubleslash-audio-defaults".into())
            .spawn(move || watch_thread(tx, wake_tx, wake_rx, stop_t))
            .ok()?;
        Some(DefaultDeviceWatch {
            stop,
            thread: Some(thread),
        })
    }

    fn watch_thread(
        out: tokio::sync::mpsc::UnboundedSender<OsDefaultChange>,
        wake_tx: mpsc::SyncSender<()>,
        wake_rx: mpsc::Receiver<()>,
        stop: Arc<AtomicBool>,
    ) {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let enumerator: IMMDeviceEnumerator =
            match unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) } {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("audio default watch: enumerator unavailable: {e}");
                    unsafe {
                        CoUninitialize();
                    }
                    return;
                }
            };

        let client: IMMNotificationClient = DefaultDeviceClient { wake: wake_tx }.into();
        if let Err(e) = unsafe { enumerator.RegisterEndpointNotificationCallback(&client) } {
            tracing::warn!("audio default watch: register failed: {e}");
            unsafe {
                CoUninitialize();
            }
            return;
        }

        let mut last = query_console_defaults();
        tracing::debug!(
            "audio default watch started (input={:?}, output={:?})",
            last.input,
            last.output
        );

        while !stop.load(Ordering::Relaxed) {
            match wake_rx.recv_timeout(Duration::from_millis(200)) {
                Ok(()) => {}
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
            // Console + multimedia roles often fire together for one user click.
            std::thread::sleep(Duration::from_millis(80));
            while wake_rx.try_recv().is_ok() {}

            let now = query_console_defaults();
            let input = now.input != last.input;
            let output = now.output != last.output;
            if input || output {
                last = now;
                let _ = out.send(OsDefaultChange { input, output });
            }
        }

        let _ = unsafe { enumerator.UnregisterEndpointNotificationCallback(&client) };
        unsafe {
            CoUninitialize();
        }
    }

    #[implement(IMMNotificationClient)]
    struct DefaultDeviceClient {
        wake: mpsc::SyncSender<()>,
    }

    #[allow(non_snake_case)]
    impl IMMNotificationClient_Impl for DefaultDeviceClient_Impl {
        fn OnDeviceStateChanged(
            &self,
            _pwstrdeviceid: &windows::core::PCWSTR,
            _dwnewstate: DEVICE_STATE,
        ) -> windows::core::Result<()> {
            Ok(())
        }

        fn OnDeviceAdded(
            &self,
            _pwstrdeviceid: &windows::core::PCWSTR,
        ) -> windows::core::Result<()> {
            Ok(())
        }

        fn OnDeviceRemoved(
            &self,
            _pwstrdeviceid: &windows::core::PCWSTR,
        ) -> windows::core::Result<()> {
            Ok(())
        }

        fn OnDefaultDeviceChanged(
            &self,
            flow: EDataFlow,
            role: ERole,
            _pwstrdefaultdeviceid: &windows::core::PCWSTR,
        ) -> windows::core::Result<()> {
            // Voice and loopback open the console role, not communications.
            if role != eConsole {
                return Ok(());
            }
            if flow != eCapture && flow != eRender {
                return Ok(());
            }
            let _ = self.wake.try_send(());
            Ok(())
        }

        fn OnPropertyValueChanged(
            &self,
            _pwstrdeviceid: &windows::core::PCWSTR,
            _key: &PROPERTYKEY,
        ) -> windows::core::Result<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_default_sentinel_and_duplicates() {
        let names = sanitize_device_names([
            "Default".into(),
            "Mic".into(),
            "  ".into(),
            "Mic".into(),
            "default".into(),
            "Headset".into(),
        ]);
        assert_eq!(names, vec!["Mic".to_string(), "Headset".to_string()]);
    }

    #[test]
    fn owned_selection_treats_default_as_follow_os() {
        assert_eq!(owned_device_selection(None), None);
        assert_eq!(owned_device_selection(Some(String::new())), None);
        assert_eq!(owned_device_selection(Some("  ".into())), None);
        assert_eq!(owned_device_selection(Some("Default".into())), None);
        assert_eq!(owned_device_selection(Some("default".into())), None);
        assert_eq!(
            owned_device_selection(Some(" Headset ".into())),
            Some("Headset".into())
        );
    }

    #[test]
    fn reopen_only_when_the_followed_side_changes() {
        assert!(!should_reopen_for_os_default(true, true, false, false));
        assert!(should_reopen_for_os_default(true, true, true, false));
        assert!(should_reopen_for_os_default(true, true, false, true));
        assert!(!should_reopen_for_os_default(false, true, true, false));
        assert!(should_reopen_for_os_default(false, true, false, true));
        assert!(!should_reopen_for_os_default(true, false, false, true));
        assert!(!should_reopen_for_os_default(false, false, true, true));
    }
}
