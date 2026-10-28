//! Drawing decoded video into Android `Surface`s.
//!
//! The core's [`VideoReceiver`](doubleslash_client::video::receiver::VideoReceiver)
//! decodes; this is where its frames land. Kotlin mounts a `TextureView` for
//! every peer the user chose to watch and hands its `Surface` over through
//! `nativeAttachVideoSurface`; each decoded frame is converted to RGBA and
//! posted to the matching `ANativeWindow`.
//!
//! The window is asked for an RGBA buffer the size of the frame, so the
//! compositor can scale it. A TextureView often locks its own view size instead,
//! and posting that buffer unfilled is a black tile, so the picture is scaled
//! into whatever size the lock actually returned. The producer dataspace is left
//! alone unless the consumer default is extended-range, PQ, or HLG: an
//! unconditional sRGB tag makes an 8-bit TextureView buffer composite as black.
//! A size change is still reported to Kotlin as `peer_video_size`.
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
    if !rgba_room(frame, dst, width, height, stride) {
        return;
    }

    let chroma_width = width / 2;
    for row in 0..height {
        let y_row = &frame.y[row * width..][..width];
        let chroma_row = (row / 2) * chroma_width;
        let u_row = &frame.u[chroma_row..][..chroma_width];
        let v_row = &frame.v[chroma_row..][..chroma_width];
        let out = &mut dst[row * stride * 4..][..width * 4];

        for (column, pixel) in out.chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&yuv_to_rgba(
                y_row[column],
                u_row[column / 2],
                v_row[column / 2],
            ));
        }
    }
}

/// Scale I420 into an RGBA buffer of a different size.
///
/// Nearest-neighbour. A TextureView's `Surface` often refuses to take the
/// frame's dimensions and locks a view-sized buffer instead; skipping that
/// buffer and posting it empty is a black tile for the whole call. Sampling
/// into the size we were given still shows the picture, and the view does not
/// have to match the frame.
pub fn scale_i420_to_rgba(
    frame: &RawFrame,
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    stride: usize,
) {
    let src_width = frame.width as usize;
    let src_height = frame.height as usize;
    if src_width == 0 || src_height == 0 || !rgba_room(frame, dst, dst_width, dst_height, stride) {
        return;
    }
    // The same size is the direct copy. Keeping one formula for both means a
    // 1:1 scale cannot drift from [`i420_to_rgba`].
    if (dst_width, dst_height) == (src_width, src_height) {
        i420_to_rgba(frame, dst, stride);
        return;
    }

    let chroma_width = src_width / 2;
    for row in 0..dst_height {
        let src_row = (row as u64 * src_height as u64 / dst_height as u64) as usize;
        let y_row = &frame.y[src_row * src_width..][..src_width];
        let chroma_row = (src_row / 2) * chroma_width;
        let u_row = &frame.u[chroma_row..][..chroma_width];
        let v_row = &frame.v[chroma_row..][..chroma_width];
        let out = &mut dst[row * stride * 4..][..dst_width * 4];

        for (column, pixel) in out.chunks_exact_mut(4).enumerate() {
            let src_col = (column as u64 * src_width as u64 / dst_width as u64) as usize;
            pixel.copy_from_slice(&yuv_to_rgba(
                y_row[src_col],
                u_row[src_col / 2],
                v_row[src_col / 2],
            ));
        }
    }
}

/// Whether `dst` can hold an RGBA image of this size at `stride`.
///
/// Also requires a consistent frame: the plane slices in the converters are
/// only in bounds when the dimensions and the plane lengths agree.
fn rgba_room(frame: &RawFrame, dst: &[u8], width: usize, height: usize, stride: usize) -> bool {
    frame.is_consistent()
        && width > 0
        && height > 0
        && stride >= width
        && dst.len() >= stride * 4 * (height - 1) + width * 4
}

/// One pixel, BT.601 limited range, as RGBA8888.
fn yuv_to_rgba(y: u8, u: u8, v: u8) -> [u8; 4] {
    let c = i32::from(y) - 16;
    let d = i32::from(u) - 128;
    let e = i32::from(v) - 128;
    let luma = 298 * c + 128;
    [
        clamp((luma + 409 * e) >> 8),
        clamp((luma - 100 * d - 208 * e) >> 8),
        clamp((luma + 516 * d) >> 8),
        0xFF,
    ]
}

fn clamp(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

/// Whether 8-bit RGBA would be read as HDR in `dataspace`.
///
/// Extended-range scRGB and the PQ/HLG transfers map those bytes into a
/// fraction of the HDR range. Unknown, sRGB, and the other SDR spaces do not.
/// Forcing sRGB onto an SDR TextureView makes it allocate a color-managed
/// buffer that composites as black, so the override is only for a consumer
/// that already asked for HDR.
///
/// Bit layout matches `<android/data_space.h>`. A negative value is an error
/// from the getter (`-EINVAL`), not a dataspace.
fn consumer_dataspace_is_hdr(dataspace: i32) -> bool {
    if dataspace <= 0 {
        return false;
    }
    const TRANSFER_MASK: i32 = 31 << 22;
    const TRANSFER_ST2084: i32 = 7 << 22;
    const TRANSFER_HLG: i32 = 8 << 22;
    const RANGE_MASK: i32 = 7 << 27;
    const RANGE_EXTENDED: i32 = 3 << 27;
    let transfer = dataspace & TRANSFER_MASK;
    let range = dataspace & RANGE_MASK;
    range == RANGE_EXTENDED || transfer == TRANSFER_ST2084 || transfer == TRANSFER_HLG
}

#[cfg(target_os = "android")]
pub use window::attach_surface;

/// The `ANativeWindow` canvas. Android-only; everything above is host-testable.
#[cfg(target_os = "android")]
mod window {
    use super::{consumer_dataspace_is_hdr, i420_to_rgba, registry, scale_i420_to_rgba, Canvas};
    use doubleslash_client::video::frame::RawFrame;
    use jni::objects::JObject;
    use jni::JNIEnv;
    use std::ffi::c_void;
    use std::sync::OnceLock;
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
    /// Same 4 bytes per pixel, alpha ignored. Some devices lock this even
    /// after RGBA was requested, and it is still safe to write.
    const RGBX_8888: i32 = 2;
    /// `ADATASPACE_SRGB` from `<android/data_space.h>`. API 28.
    const ADATASPACE_SRGB: i32 = 142671872;
    /// `ADATASPACE_UNKNOWN`. Clearing a producer override returns the buffer
    /// to whatever the consumer asked for.
    const ADATASPACE_UNKNOWN: i32 = 0;

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

    // Dataspace calls are API 28 and 34. This crate links at API 26, so a
    // hard reference would fail to load there. `dlopen`/`dlsym` live in libdl.
    #[link(name = "dl")]
    extern "C" {
        fn dlopen(filename: *const std::ffi::c_char, flags: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
        fn dlerror() -> *mut std::ffi::c_char;
    }

    /// `ANativeWindow_setBuffersDataSpace`, present from API 28.
    type SetDataSpace = unsafe extern "C" fn(*mut ANativeWindow, i32) -> i32;
    /// `ANativeWindow_getBuffersDataSpace` (API 28) and
    /// `ANativeWindow_getBuffersDefaultDataSpace` (API 34).
    type GetDataSpace = unsafe extern "C" fn(*mut ANativeWindow) -> i32;

    struct DataSpaceFns {
        set: Option<SetDataSpace>,
        get: Option<GetDataSpace>,
        get_default: Option<GetDataSpace>,
    }

    fn dataspace_fns() -> &'static DataSpaceFns {
        static FNS: OnceLock<DataSpaceFns> = OnceLock::new();
        FNS.get_or_init(|| {
            // RTLD_NOW. libandroid is already loaded; this only looks it up.
            const RTLD_NOW: i32 = 2;
            let opened = unsafe { dlopen(c"libandroid.so".as_ptr(), RTLD_NOW) };
            let handle = if opened.is_null() {
                std::ptr::null_mut()
            } else {
                opened
            };
            let fns = DataSpaceFns {
                set: load(handle, c"ANativeWindow_setBuffersDataSpace"),
                get: load(handle, c"ANativeWindow_getBuffersDataSpace"),
                get_default: load(handle, c"ANativeWindow_getBuffersDefaultDataSpace"),
            };
            if fns.set.is_none() && fns.get.is_none() && fns.get_default.is_none() {
                let err = unsafe { dlerror() };
                let why = if err.is_null() {
                    "unknown".to_owned()
                } else {
                    unsafe { std::ffi::CStr::from_ptr(err) }
                        .to_string_lossy()
                        .into_owned()
                };
                warn!("[video] video dataspace calls are missing from libandroid: {why}");
            }
            fns
        })
    }

    fn load<T: Copy>(handle: *mut c_void, name: &std::ffi::CStr) -> Option<T> {
        let sym = unsafe { dlsym(handle, name.as_ptr()) };
        if sym.is_null() {
            return None;
        }
        // A function pointer is one pointer wide. A mismatch would be a
        // signature we wrote down wrong, and reading it would be unsound.
        assert_eq!(
            std::mem::size_of::<T>(),
            std::mem::size_of::<*mut c_void>(),
            "dataspace symbol {name:?} is not a pointer"
        );
        // SAFETY: `sym` is the address of an ANativeWindow dataspace function
        // of type `T`, and `T` is that function's pointer type.
        Some(unsafe { std::ptr::read_unaligned((&raw const sym).cast::<T>()) })
    }

    struct DataSpace {
        /// Consumer default, or -1 when the API 34 getter is missing.
        consumer: i32,
        /// Producer override after this call, or -1 when the getter is missing.
        producer: i32,
        forced_srgb: bool,
    }

    /// Leave an SDR surface on the consumer's dataspace.
    ///
    /// Tagging every buffer sRGB — including a TextureView whose consumer
    /// default is unknown — makes the 8-bit bytes composite as black. An
    /// extended, PQ, or HLG consumer still needs the sRGB tag, or those same
    /// bytes are read as a fraction of the HDR range. A queue reset drops the
    /// override, so a consumer that is HDR is checked on every frame.
    fn align_dataspace(window: *mut ANativeWindow) -> DataSpace {
        static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let fns = dataspace_fns();
        // SAFETY: `window` is a live reference held from attach until drop.
        let consumer = fns
            .get_default
            .map(|get| unsafe { get(window) })
            .unwrap_or(-1);
        let mut producer = fns.get.map(|get| unsafe { get(window) }).unwrap_or(-1);
        let mut forced_srgb = false;
        if let Some(set) = fns.set {
            // `None` means "do not touch it". An SDR surface whose producer
            // override is already unknown stays that way: setting sRGB here
            // is what turned a decoded picture into a black tile.
            let wanted = if consumer_dataspace_is_hdr(consumer) {
                Some(ADATASPACE_SRGB)
            } else if producer == ADATASPACE_SRGB {
                Some(ADATASPACE_UNKNOWN)
            } else {
                None
            };
            if let Some(wanted) = wanted {
                if producer == wanted {
                    forced_srgb = wanted == ADATASPACE_SRGB;
                } else {
                    let status = unsafe { set(window, wanted) };
                    if status != 0 && !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        warn!(
                            "[video] could not set a video surface dataspace to {wanted}: {status}"
                        );
                    } else if status == 0 {
                        forced_srgb = wanted == ADATASPACE_SRGB;
                        producer = wanted;
                    }
                }
            }
        } else if consumer_dataspace_is_hdr(consumer)
            && !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            warn!(
                "[video] ANativeWindow_setBuffersDataSpace is missing; an HDR surface will show 8-bit video as black"
            );
        }
        DataSpace {
            consumer,
            producer,
            forced_srgb,
        }
    }

    /// Min and max luma, sampled. Enough to tell a black decode from a black
    /// composite without walking a 720p plane on every frame.
    fn luma_span(y: &[u8]) -> (u8, u8) {
        let mut min = 255u8;
        let mut max = 0u8;
        if y.is_empty() {
            return (0, 0);
        }
        for &sample in y.iter().step_by(64) {
            min = min.min(sample);
            max = max.max(sample);
        }
        (min, max)
    }

    struct NativeWindow {
        window: *mut ANativeWindow,
        /// Buffer geometry last requested, so it is only changed when the frame is.
        geometry: (u32, u32),
        /// The frame size could not be locked. Scale into the view's buffer.
        use_view_size: bool,
        /// One warning when the window will not take the frame's size or format.
        noted_mismatch: bool,
        /// The first locked buffer has been described.
        noted_buffer: bool,
        /// A failed lock has been described.
        noted_lock: bool,
    }

    // SAFETY: `ANativeWindow` is reference-counted and its functions are
    // thread-safe; the registry mutex serialises every use of this handle.
    unsafe impl Send for NativeWindow {}

    impl NativeWindow {
        /// Lock the next buffer, let `fill` write it, and post it.
        ///
        /// `fill` receives the format the window actually locked. Only the
        /// 32-bit formats are a `stride * height * 4` byte buffer; the caller
        /// must not write into anything else.
        ///
        /// Returns the lock status. A non-zero status means nothing was posted.
        fn post(&mut self, fill: impl FnOnce(&mut [u8], usize, usize, usize, i32)) -> i32 {
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
            let status =
                unsafe { ANativeWindow_lock(self.window, &mut buffer, std::ptr::null_mut()) };
            if status != 0 {
                // A surface on its way out refuses the lock; the detach that
                // follows is the real cleanup. A frame-sized request the
                // TextureView will not honour fails the same way, and posting
                // nothing is a black tile, so the caller has to hear it.
                return status;
            }
            if !buffer.bits.is_null()
                && buffer.width > 0
                && buffer.height > 0
                && (buffer.format == RGBA_8888 || buffer.format == RGBX_8888)
            {
                let (width, height, stride) = (
                    buffer.width as usize,
                    buffer.height as usize,
                    buffer.stride.max(buffer.width) as usize,
                );
                // SAFETY: a locked RGBA_8888 or RGBX_8888 buffer is
                // `stride * height` pixels of four bytes each, owned by us
                // until `unlockAndPost`.
                let bytes = unsafe {
                    std::slice::from_raw_parts_mut(buffer.bits.cast::<u8>(), stride * height * 4)
                };
                fill(bytes, width, height, stride, buffer.format);
            } else {
                fill(&mut [], 0, 0, 0, buffer.format);
            }
            // SAFETY: paired with the successful lock above.
            unsafe { ANativeWindow_unlockAndPost(self.window) }
        }
    }

    impl Canvas for NativeWindow {
        fn draw(&mut self, frame: &RawFrame) {
            let size = (frame.width, frame.height);
            if !self.use_view_size && self.geometry != size {
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
                    if !self.noted_mismatch {
                        warn!("[video] could not size a video surface to {size:?}: {status}");
                        self.noted_mismatch = true;
                    }
                } else {
                    self.geometry = size;
                }
            }
            let space = align_dataspace(self.window);
            let mut scaled = false;
            let mut bad_format = false;
            let record = !self.noted_buffer;
            let mut locked = None;
            let lock = self.post(|bytes, width, height, stride, format| {
                if record {
                    locked = Some((width, height, stride, format));
                }
                // RGB_565 and the private HDR formats are not 4 bytes a pixel.
                // Writing RGBA into one of those overruns the buffer.
                if format != RGBA_8888 && format != RGBX_8888 {
                    bad_format = true;
                    return;
                }
                if (width, height) == (frame.width as usize, frame.height as usize) {
                    i420_to_rgba(frame, bytes, stride);
                } else if width > 0 && height > 0 {
                    // The window kept its own size. Paint into that, rather
                    // than posting the unfilled buffer, which is a black tile.
                    scaled = true;
                    scale_i420_to_rgba(frame, bytes, width, height, stride);
                }
            });
            if lock != 0 {
                // The frame-sized queue was accepted and then would not lock.
                // Next frame, take the view's own buffer and scale into it.
                if self.geometry == size {
                    self.geometry = (0, 0);
                    self.use_view_size = true;
                }
                if !self.noted_lock {
                    warn!("[video] could not lock a video surface: {lock}");
                    self.noted_lock = true;
                }
                return;
            }
            if record {
                self.noted_buffer = true;
                let (width, height, stride, format) = locked.unwrap_or((0, 0, 0, -1));
                let (ymin, ymax) = luma_span(&frame.y);
                let DataSpace {
                    consumer,
                    producer,
                    forced_srgb,
                } = space;
                warn!(
                    "[video] buffer {width}x{height} stride {stride} format {format} consumer {consumer} producer {producer} srgb {forced_srgb} Y {ymin}-{ymax}"
                );
            }
            if bad_format {
                // Ask again next frame. A resize that has not landed yet locks
                // the previous format, and giving up here leaves the tile black.
                self.geometry = (0, 0);
                self.use_view_size = false;
            }
            if (scaled || bad_format) && !self.noted_mismatch {
                if bad_format {
                    warn!("[video] surface locked a buffer that is not RGBA; will retry");
                } else {
                    warn!(
                        "[video] surface locked a buffer other than {size:?}; scaling the picture into it"
                    );
                }
                self.noted_mismatch = true;
            }
        }

        fn blank(&mut self) {
            let _ = self.post(|bytes, _, _, _, format| {
                if format != RGBA_8888 && format != RGBX_8888 {
                    return;
                }
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
                use_view_size: false,
                noted_mismatch: false,
                noted_buffer: false,
                noted_lock: false,
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

    /// A window that will not take the frame's size must still show it.
    /// Skipping the copy and posting the empty buffer is a black tile.
    #[test]
    fn a_smaller_buffer_is_scaled_rather_than_left_blank() {
        let mut frame = solid(4, 2, 16, 128, 128);
        // Left half black, right half white, so the scale has to sample both.
        for column in 2..4 {
            frame.y[column] = 235;
            frame.y[4 + column] = 235;
        }
        let mut out = vec![7u8; 2 * 2 * 4];
        scale_i420_to_rgba(&frame, &mut out, 2, 2, 2);
        assert_eq!(&out[0..4], &[0, 0, 0, 0xFF], "left pixel");
        assert_eq!(&out[4..8], &[255, 255, 255, 0xFF], "right pixel");
        assert_eq!(&out[8..12], &[0, 0, 0, 0xFF]);
        assert_eq!(&out[12..16], &[255, 255, 255, 0xFF]);
    }

    #[test]
    fn scaling_to_the_frames_own_size_matches_the_direct_copy() {
        let frame = solid(4, 2, 81, 90, 240);
        let mut scaled = vec![0u8; 4 * 2 * 4];
        let mut direct = vec![0u8; 4 * 2 * 4];
        scale_i420_to_rgba(&frame, &mut scaled, 4, 2, 4);
        i420_to_rgba(&frame, &mut direct, 4);
        assert_eq!(scaled, direct);
    }

    #[test]
    fn only_an_hdr_consumer_needs_an_srgb_override() {
        // UNKNOWN and the ordinary sRGB / P3 / BT.709 spaces.
        for dataspace in [0, 142671872, 143261696, 281083904, -22] {
            assert!(
                !consumer_dataspace_is_hdr(dataspace),
                "{dataspace} is not HDR"
            );
        }
        // scRGB, scRGB linear, BT.2020 PQ and HLG, both full and limited.
        for dataspace in [
            411107328, 406913024, 163971072, 298188800, 168165376, 302383104,
        ] {
            assert!(consumer_dataspace_is_hdr(dataspace), "{dataspace} is HDR");
        }
    }
}
