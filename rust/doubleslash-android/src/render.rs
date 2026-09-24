//! Drawing decoded video into Android `Surface`s.
//!
//! The core's [`VideoReceiver`](doubleslash_client::video::receiver::VideoReceiver)
//! decodes; this is where its frames land. Kotlin mounts a `SurfaceView` for
//! every peer the user chose to watch and hands its `Surface` over through
//! `nativeAttachVideoSurface`; each decoded frame is converted to RGBA and
//! posted to the matching `ANativeWindow`.
//!
//! The buffer is sized to the *frame*, not the view, so the compositor does the
//! scaling — in hardware, filtered, for free. That leaves the aspect ratio to
//! the view, which is why a size change is reported to Kotlin as
//! `peer_video_size`.
//!
//! Bindings are keyed by an attach token rather than the peer id. Compose can
//! create the replacement view for a peer before it destroys the old one, and a
//! detach keyed by peer would then tear down the surface that just arrived.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use doubleslash_client::video::frame::RawFrame;
use doubleslash_client::video::sink::RenderSink;
use parking_lot::Mutex;
use tracing::warn;

use crate::sink::EventSink;

/// One place frames can be drawn.
pub trait Canvas: Send {
    /// Draw `frame`, resizing the buffer to it first when needed.
    fn draw(&mut self, frame: &RawFrame);
    /// Fill with black, so a peer who stopped does not leave a frozen picture.
    fn blank(&mut self);
}

struct Binding {
    token: i64,
    /// Normalised peer id — see [`normalise`].
    peer: String,
    canvas: Box<dyn Canvas>,
}

/// Every attached surface, and the last frame size reported for each peer.
#[derive(Default)]
pub struct Registry {
    bindings: Vec<Binding>,
    sizes: HashMap<String, (u32, u32)>,
    next_token: i64,
}

/// Peer ids arrive in two spellings of one base64 key — the supernode's padded
/// `public_id` on some paths and the relay's unpadded form on others — so the
/// registry compares on the unpadded one. Hex peer-store keys carry no padding
/// and pass through unchanged.
fn normalise(peer_id: &str) -> &str {
    peer_id.trim_end_matches('=')
}

impl Registry {
    /// Bind a canvas to a peer. Returns the token that later detaches it.
    pub fn attach(&mut self, peer_id: &str, canvas: Box<dyn Canvas>) -> i64 {
        self.next_token += 1;
        let token = self.next_token;
        self.bindings.push(Binding {
            token,
            peer: normalise(peer_id).to_owned(),
            canvas,
        });
        token
    }

    /// Unbind one surface. The canvas is dropped — releasing its window —
    /// before this returns, which is what `surfaceDestroyed` requires.
    pub fn detach(&mut self, token: i64) -> bool {
        let before = self.bindings.len();
        self.bindings.retain(|b| b.token != token);
        self.bindings.len() != before
    }

    pub fn watching(&self, peer_id: &str) -> bool {
        let peer = normalise(peer_id);
        self.bindings.iter().any(|b| b.peer == peer)
    }

    /// Draw on every surface bound to `peer_id`.
    ///
    /// Returns the frame size when it differs from the last one reported for
    /// this peer, so the caller can tell the UI without a JNI call per frame.
    pub fn draw(&mut self, peer_id: &str, frame: &RawFrame) -> Option<(u32, u32)> {
        let peer = normalise(peer_id);
        let mut drawn = false;
        for binding in self.bindings.iter_mut().filter(|b| b.peer == peer) {
            binding.canvas.draw(frame);
            drawn = true;
        }
        if !drawn {
            return None;
        }
        let size = (frame.width, frame.height);
        match self.sizes.insert(peer.to_owned(), size) {
            Some(previous) if previous == size => None,
            _ => Some(size),
        }
    }

    pub fn blank(&mut self, peer_id: &str) {
        let peer = normalise(peer_id);
        for binding in self.bindings.iter_mut().filter(|b| b.peer == peer) {
            binding.canvas.blank();
        }
    }

    pub fn blank_all(&mut self) {
        for binding in &mut self.bindings {
            binding.canvas.blank();
        }
    }
}

/// The process-wide registry.
///
/// Global rather than per session: surfaces belong to views, which outlive a
/// core restart, and the JNI attach call has no session handle to hand.
pub fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

/// The receiver's render target on Android.
pub struct SurfaceSinks {
    events: EventSink,
}

impl SurfaceSinks {
    pub fn new(events: EventSink) -> Arc<Self> {
        Arc::new(Self { events })
    }

    /// Report a peer's new frame size so the view can match its aspect ratio.
    ///
    /// Called from the decode thread, outside the registry lock: the listener
    /// is Kotlin code, and a `surfaceDestroyed` waiting on that lock from the
    /// UI thread must never end up waiting on the UI thread in turn.
    fn report_size(&self, peer_id: &str, (width, height): (u32, u32)) {
        let payload = serde_json::json!({
            "event": "peer_video_size",
            "peer_id": normalise(peer_id),
            "width": width,
            "height": height,
        });
        match self.events.attach() {
            Ok(mut env) => self.events.emit(&mut env, &payload.to_string()),
            Err(e) => warn!("[video] could not report frame size: {e}"),
        }
    }
}

impl RenderSink for SurfaceSinks {
    fn has_sink(&self, peer_id: &str) -> bool {
        registry().lock().watching(peer_id)
    }

    fn push_frame(&self, peer_id: &str, frame: &RawFrame) {
        if !frame.is_consistent() {
            return;
        }
        let resized = registry().lock().draw(peer_id, frame);
        if let Some(size) = resized {
            self.report_size(peer_id, size);
        }
    }

    fn clear_peer(&self, peer_id: &str) {
        registry().lock().blank(peer_id);
    }

    fn clear_all(&self) {
        registry().lock().blank_all();
    }
}

/// Convert I420 to RGBA8888 into a buffer whose rows are `stride` pixels apart.
///
/// BT.601 limited range, which is what VP8 carries. Integer arithmetic: this
/// runs per pixel for every displayed frame, on a phone.
pub fn i420_to_rgba(frame: &RawFrame, dst: &mut [u8], stride: usize) {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let chroma_width = width / 2;
    // Consistency is what makes the plane slicing below in bounds.
    if !frame.is_consistent()
        || width == 0
        || height == 0
        || stride < width
        || dst.len() < stride * 4 * (height - 1) + width * 4
    {
        return;
    }

    for row in 0..height {
        let y_row = &frame.y[row * width..][..width];
        let chroma_row = (row / 2) * chroma_width;
        let u_row = &frame.u[chroma_row..][..chroma_width];
        let v_row = &frame.v[chroma_row..][..chroma_width];
        let out = &mut dst[row * stride * 4..][..width * 4];

        for (column, pixel) in out.chunks_exact_mut(4).enumerate() {
            let c = i32::from(y_row[column]) - 16;
            let d = i32::from(u_row[column / 2]) - 128;
            let e = i32::from(v_row[column / 2]) - 128;
            let luma = 298 * c + 128;
            pixel[0] = clamp((luma + 409 * e) >> 8);
            pixel[1] = clamp((luma - 100 * d - 208 * e) >> 8);
            pixel[2] = clamp((luma + 516 * d) >> 8);
            pixel[3] = 0xFF;
        }
    }
}

fn clamp(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

#[cfg(target_os = "android")]
pub use window::attach_surface;

/// The `ANativeWindow` canvas. Android-only; everything above is host-testable.
#[cfg(target_os = "android")]
mod window {
    use super::{i420_to_rgba, registry, Canvas};
    use doubleslash_client::video::frame::RawFrame;
    use jni::objects::JObject;
    use jni::JNIEnv;
    use std::ffi::c_void;
    use tracing::warn;

    #[repr(C)]
    struct ANativeWindow {
        _private: [u8; 0],
    }

    /// `ANativeWindow_Buffer` from `<android/native_window.h>`.
    #[repr(C)]
    struct Buffer {
        width: i32,
        height: i32,
        /// In pixels, not bytes.
        stride: i32,
        format: i32,
        bits: *mut c_void,
        reserved: [u32; 6],
    }

    /// `WINDOW_FORMAT_RGBA_8888`: the one CPU-lockable format every device
    /// supports. YV12 would skip the conversion but is not guaranteed.
    const RGBA_8888: i32 = 1;

    #[link(name = "android")]
    extern "C" {
        fn ANativeWindow_fromSurface(
            env: *mut jni::sys::JNIEnv,
            surface: jni::sys::jobject,
        ) -> *mut ANativeWindow;
        fn ANativeWindow_release(window: *mut ANativeWindow);
        fn ANativeWindow_setBuffersGeometry(
            window: *mut ANativeWindow,
            width: i32,
            height: i32,
            format: i32,
        ) -> i32;
        fn ANativeWindow_lock(
            window: *mut ANativeWindow,
            buffer: *mut Buffer,
            dirty: *mut c_void,
        ) -> i32;
        fn ANativeWindow_unlockAndPost(window: *mut ANativeWindow) -> i32;
    }

    struct NativeWindow {
        window: *mut ANativeWindow,
        /// Buffer geometry last set, so it is only changed when the frame is.
        geometry: (u32, u32),
    }

    // SAFETY: `ANativeWindow` is reference-counted and its functions are
    // thread-safe; the registry mutex serialises every use of this handle.
    unsafe impl Send for NativeWindow {}

    impl NativeWindow {
        /// Lock the next buffer, let `fill` write it, and post it.
        fn post(&mut self, fill: impl FnOnce(&mut [u8], usize, usize, usize)) {
            let mut buffer = Buffer {
                width: 0,
                height: 0,
                stride: 0,
                format: 0,
                bits: std::ptr::null_mut(),
                reserved: [0; 6],
            };
            // SAFETY: `window` is a live reference held since attach; `buffer`
            // is a valid out-parameter.
            if unsafe { ANativeWindow_lock(self.window, &mut buffer, std::ptr::null_mut()) } != 0 {
                // A surface on its way out refuses the lock; the detach that
                // follows is the real cleanup.
                return;
            }
            if !buffer.bits.is_null() && buffer.width > 0 && buffer.height > 0 {
                let (width, height, stride) = (
                    buffer.width as usize,
                    buffer.height as usize,
                    buffer.stride.max(buffer.width) as usize,
                );
                // SAFETY: a locked RGBA_8888 buffer is `stride * height` pixels
                // of four bytes each, owned by us until `unlockAndPost`.
                let bytes = unsafe {
                    std::slice::from_raw_parts_mut(buffer.bits.cast::<u8>(), stride * height * 4)
                };
                fill(bytes, width, height, stride);
            }
            // SAFETY: paired with the successful lock above.
            unsafe { ANativeWindow_unlockAndPost(self.window) };
        }
    }

    impl Canvas for NativeWindow {
        fn draw(&mut self, frame: &RawFrame) {
            let size = (frame.width, frame.height);
            if self.geometry != size {
                // SAFETY: live window; dimensions are positive and even.
                let status = unsafe {
                    ANativeWindow_setBuffersGeometry(
                        self.window,
                        frame.width as i32,
                        frame.height as i32,
                        RGBA_8888,
                    )
                };
                if status != 0 {
                    warn!("[video] could not size a video surface to {size:?}: {status}");
                    return;
                }
                self.geometry = size;
            }
            self.post(|bytes, width, height, stride| {
                // The buffer is the frame's size unless a resize is still in
                // flight; skip that one frame rather than draw it skewed.
                if (width, height) == (frame.width as usize, frame.height as usize) {
                    i420_to_rgba(frame, bytes, stride);
                }
            });
        }

        fn blank(&mut self) {
            self.post(|bytes, _, _, _| {
                for pixel in bytes.chunks_exact_mut(4) {
                    pixel.copy_from_slice(&[0, 0, 0, 0xFF]);
                }
            });
        }
    }

    impl Drop for NativeWindow {
        fn drop(&mut self) {
            // SAFETY: releases the reference `fromSurface` acquired.
            unsafe { ANativeWindow_release(self.window) };
        }
    }

    /// Bind a Java `Surface` to `peer_id`. Returns the detach token, or 0.
    pub fn attach_surface(env: &JNIEnv<'_>, surface: &JObject<'_>, peer_id: &str) -> i64 {
        // SAFETY: `surface` is a live `android.view.Surface` local reference for
        // the duration of this JNI call; the returned window holds its own
        // reference, so it outlives that local.
        let window = unsafe { ANativeWindow_fromSurface(env.get_raw(), surface.as_raw()) };
        if window.is_null() {
            warn!("[video] the view handed over a surface with no native window");
            return 0;
        }
        registry().lock().attach(
            peer_id,
            Box::new(NativeWindow {
                window,
                geometry: (0, 0),
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Counts draws and blanks, and reports its drop.
    struct Probe {
        draws: Arc<AtomicUsize>,
        blanks: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
    }

    #[derive(Default, Clone)]
    struct Counts {
        draws: Arc<AtomicUsize>,
        blanks: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
    }

    impl Counts {
        fn probe(&self) -> Box<dyn Canvas> {
            Box::new(Probe {
                draws: Arc::clone(&self.draws),
                blanks: Arc::clone(&self.blanks),
                dropped: Arc::clone(&self.dropped),
            })
        }
    }

    impl Canvas for Probe {
        fn draw(&mut self, _frame: &RawFrame) {
            self.draws.fetch_add(1, Ordering::SeqCst);
        }
        fn blank(&mut self) {
            self.blanks.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn padded_and_unpadded_ids_are_one_peer() {
        let mut registry = Registry::default();
        let counts = Counts::default();
        registry.attach("AbCd==", counts.probe());
        assert!(registry.watching("AbCd"));
        registry.draw("AbCd", &RawFrame::black(4, 4));
        assert_eq!(counts.draws.load(Ordering::SeqCst), 1);
    }

    /// Compose may attach the replacement view before the old one is
    /// destroyed. Detaching the old token must leave the new surface alone.
    #[test]
    fn a_late_detach_does_not_remove_the_replacement() {
        let mut registry = Registry::default();
        let old = Counts::default();
        let new = Counts::default();
        let old_token = registry.attach("peer", old.probe());
        registry.attach("peer", new.probe());

        assert!(registry.detach(old_token));
        assert_eq!(old.dropped.load(Ordering::SeqCst), 1, "window released");
        assert!(registry.watching("peer"), "the new surface is still bound");

        registry.draw("peer", &RawFrame::black(4, 4));
        assert_eq!(new.draws.load(Ordering::SeqCst), 1);
        assert_eq!(old.draws.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_size_is_reported_once_per_change() {
        let mut registry = Registry::default();
        registry.attach("peer", Counts::default().probe());
        assert_eq!(registry.draw("peer", &RawFrame::black(4, 4)), Some((4, 4)));
        assert_eq!(registry.draw("peer", &RawFrame::black(4, 4)), None);
        assert_eq!(registry.draw("peer", &RawFrame::black(8, 4)), Some((8, 4)));
    }

    #[test]
    fn an_unwatched_peer_draws_nothing_and_reports_nothing() {
        let mut registry = Registry::default();
        let counts = Counts::default();
        registry.attach("alice", counts.probe());
        assert!(!registry.watching("bob"));
        assert_eq!(registry.draw("bob", &RawFrame::black(4, 4)), None);
        assert_eq!(counts.draws.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn blanking_one_peer_leaves_the_others() {
        let mut registry = Registry::default();
        let alice = Counts::default();
        let bob = Counts::default();
        registry.attach("alice", alice.probe());
        registry.attach("bob", bob.probe());
        registry.blank("alice");
        assert_eq!(alice.blanks.load(Ordering::SeqCst), 1);
        assert_eq!(bob.blanks.load(Ordering::SeqCst), 0);
        registry.blank_all();
        assert_eq!(bob.blanks.load(Ordering::SeqCst), 1);
    }

    fn solid(width: u32, height: u32, y: u8, u: u8, v: u8) -> RawFrame {
        let (w, h) = (width as usize, height as usize);
        let chroma = (w / 2) * (h / 2);
        RawFrame {
            width,
            height,
            y: vec![y; w * h],
            u: vec![u; chroma],
            v: vec![v; chroma],
        }
    }

    #[test]
    fn video_black_and_white_map_to_full_range() {
        let mut out = vec![0u8; 2 * 2 * 4];
        i420_to_rgba(&solid(2, 2, 16, 128, 128), &mut out, 2);
        assert!(out.chunks(4).all(|p| p == [0, 0, 0, 0xFF]));
        i420_to_rgba(&solid(2, 2, 235, 128, 128), &mut out, 2);
        assert!(out.chunks(4).all(|p| p == [255, 255, 255, 0xFF]));
    }

    #[test]
    fn strong_red_chroma_is_red() {
        let mut out = vec![0u8; 2 * 2 * 4];
        // BT.601 pure red: Y 81, Cb 90, Cr 240.
        i420_to_rgba(&solid(2, 2, 81, 90, 240), &mut out, 2);
        assert!(out[0] > 250 && out[1] < 5 && out[2] < 5, "got {out:?}");
    }

    /// Window buffers are usually wider than the image. Padding must be left
    /// alone and each row must start at its stride, not at `width`.
    #[test]
    fn rows_land_on_the_stride() {
        let stride = 4;
        let mut out = vec![7u8; stride * 2 * 4];
        i420_to_rgba(&solid(2, 2, 235, 128, 128), &mut out, stride);
        let row = stride * 4;
        assert_eq!(&out[0..8], &[255, 255, 255, 0xFF, 255, 255, 255, 0xFF]);
        assert_eq!(&out[8..16], &[7; 8], "padding was overwritten");
        assert_eq!(&out[row..row + 4], &[255, 255, 255, 0xFF]);
    }

    #[test]
    fn a_short_buffer_is_refused_not_overrun() {
        let mut out = vec![0u8; 4];
        i420_to_rgba(&solid(2, 2, 235, 128, 128), &mut out, 2);
        assert_eq!(out, vec![0; 4]);
    }
}
