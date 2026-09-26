//! GPU-side scale and colour conversion for screen capture.
//!
//! # Why this exists
//!
//! Two problems share one fix.
//!
//! **HDR desktops came out washed out.** On a display running in HDR, the
//! desktop is composed in linear scRGB. Asking Windows.Graphics.Capture for
//! 8-bit BGRA hands back a conversion with no tone mapping and SDR white in the
//! wrong place — the familiar grey, low-contrast picture. The fix every capture
//! tool converged on is to take the frame as 16-bit float scRGB and convert it
//! ourselves: scale so SDR white lands where the user's "SDR content
//! brightness" slider puts it, bring brighter highlights into range without shifting their hue, and
//! encode to sRGB.
//!
//! **Full-resolution frames were crossing to the CPU.** The capture copied
//! every frame at source size into system memory and scaled it there. A 4K
//! monitor is 33 MB per frame of readback plus a CPU box filter, before the
//! encoder has seen a pixel — at 60 fps that alone overruns the frame budget.
//! Scaling on the GPU first means only the encoder-sized frame is read back.
//!
//! # Pipeline
//!
//! ```text
//! WGC texture (BGRA8, or FP16 scRGB on an HDR display)
//!   -> pixel shader: box-filtered downscale into the letterboxed rectangle,
//!      and for HDR: SDR-white scaling, hue-preserving highlight fit, sRGB encode
//!   -> BGRA8 render target at the encoder's size
//!   -> staging texture (CPU-readable, encoder size only)
//! ```
//!
//! The caller converts the result to I420 exactly as before; nothing past this
//! point knows the source was HDR.

use windows::core::{s, Interface, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput6};
use windows::Win32::Graphics::Gdi::HMONITOR;

/// Luminance of scRGB 1.0, in nits. scRGB is defined against an 80-nit white.
const SCRGB_WHITE_NITS: f32 = 80.0;

/// SDR white to assume when the display will not say. Windows' own default
/// for the "SDR content brightness" slider sits near here on most panels.
const DEFAULT_SDR_WHITE_NITS: f32 = 200.0;

/// Taps per axis the box filter may take. Past 4 (a 4x reduction, e.g. 4K to
/// 960x540) the remaining aliasing is invisible next to what the encoder does.
const MAX_TAPS: u32 = 4;

const SHADER: &str = r#"
Texture2D src : register(t0);
SamplerState samp : register(s0);

cbuffer Params : register(b0) {
    float2 src_texel;   // 1 / source size
    float2 footprint;   // source texels per output pixel, >= 1
    float  sdr_scale;   // multiplies scRGB so SDR white reaches 1.0
    uint   hdr;         // 1: source is linear scRGB
    uint   taps;        // box filter taps per axis
    uint   pad;
};

struct VSOut { float4 pos : SV_Position; float2 uv : TEXCOORD0; };

// One triangle covering the viewport; no vertex buffer needed.
VSOut vs_main(uint id : SV_VertexID) {
    VSOut o;
    float2 uv = float2((id << 1) & 2, id & 2);
    o.pos = float4(uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    o.uv = uv;
    return o;
}

// Everything up to SDR white passes through untouched: a shared desktop is
// mostly SDR text and UI, and any curve that starts below 1.0 greys out
// white. Above it, the colour is scaled down by its largest channel rather
// than clipped per channel, which keeps hue — a bright red stays red instead
// of washing toward white.
float3 fit_sdr(float3 c) {
    c = max(c, 0.0);
    float m = max(c.r, max(c.g, c.b));
    if (m > 1.0) {
        c /= m;
    }
    return c;
}

float3 srgb_encode(float3 c) {
    float3 lo = c * 12.92;
    float3 hi = 1.055 * pow(c, 1.0 / 2.4) - 0.055;
    return lerp(hi, lo, step(c, 0.0031308));
}

float4 ps_main(VSOut i) : SV_Target {
    float3 acc = 0.0;
    [loop] for (uint y = 0; y < taps; y++) {
        [loop] for (uint x = 0; x < taps; x++) {
            float2 off = ((float2(x, y) + 0.5) / taps - 0.5) * footprint * src_texel;
            acc += src.SampleLevel(samp, i.uv + off, 0).rgb;
        }
    }
    acc /= (float)(taps * taps);
    if (hdr != 0) {
        acc = srgb_encode(fit_sdr(acc * sdr_scale));
    }
    return float4(saturate(acc), 1.0);
}
"#;

/// Constant buffer layout, matching `Params` above (16-byte aligned).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Params {
    src_texel: [f32; 2],
    footprint: [f32; 2],
    sdr_scale: f32,
    hdr: u32,
    taps: u32,
    pad: u32,
}

/// What the display a capture comes from is doing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayColor {
    /// The display is in HDR (PQ / BT.2020) mode.
    pub hdr: bool,
    /// Where the user has put SDR white, in nits. Only meaningful when `hdr`.
    pub sdr_white_nits: f32,
}

impl DisplayColor {
    pub const SDR: Self = Self {
        hdr: false,
        sdr_white_nits: SCRGB_WHITE_NITS,
    };

    /// The factor that puts this display's SDR white at 1.0 in scRGB.
    pub fn sdr_scale(&self) -> f32 {
        SCRGB_WHITE_NITS / self.sdr_white_nits.max(1.0)
    }
}

/// Inspect the display `monitor` is on.
///
/// Anything that cannot be determined reads as SDR, which is the behaviour
/// before this module existed: a mistake costs HDR fidelity, never a picture.
pub fn display_color(monitor: HMONITOR) -> DisplayColor {
    if !monitor_is_hdr(monitor) {
        return DisplayColor::SDR;
    }
    DisplayColor {
        hdr: true,
        sdr_white_nits: sdr_white_nits(monitor).unwrap_or(DEFAULT_SDR_WHITE_NITS),
    }
}

fn monitor_is_hdr(monitor: HMONITOR) -> bool {
    // SAFETY: plain DXGI enumeration; every out-value is checked.
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return false;
        };
        let mut a = 0;
        while let Ok(adapter) = factory.EnumAdapters1(a) {
            a += 1;
            let mut o = 0;
            while let Ok(output) = adapter.EnumOutputs(o) {
                o += 1;
                let Ok(output6) = output.cast::<IDXGIOutput6>() else {
                    continue;
                };
                let Ok(desc) = output6.GetDesc1() else {
                    continue;
                };
                if desc.Monitor == monitor {
                    return desc.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
                }
            }
        }
    }
    false
}

/// The "SDR content brightness" of the display `monitor` is on, in nits.
fn sdr_white_nits(monitor: HMONITOR) -> Option<f32> {
    use windows::Win32::Devices::Display::*;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO, MONITORINFOEXW};

    // SAFETY: every struct is sized and typed as the API documents, and every
    // call's status is checked before its output is read.
    unsafe {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(monitor, &mut info.monitorInfo as *mut MONITORINFO).as_bool() {
            return None;
        }

        let (mut n_paths, mut n_modes) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut n_paths, &mut n_modes).is_err() {
            return None;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); n_paths as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); n_modes as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut n_paths,
            paths.as_mut_ptr(),
            &mut n_modes,
            modes.as_mut_ptr(),
            None,
        )
        .is_err()
        {
            return None;
        }
        paths.truncate(n_paths as usize);

        for path in &paths {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            source.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            source.header.adapterId = path.sourceInfo.adapterId;
            source.header.id = path.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut source.header) != 0 {
                continue;
            }
            if !wide_eq(&source.viewGdiDeviceName, &info.szDevice) {
                continue;
            }
            let mut white = DISPLAYCONFIG_SDR_WHITE_LEVEL::default();
            white.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL;
            white.header.size = std::mem::size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>() as u32;
            white.header.adapterId = path.targetInfo.adapterId;
            white.header.id = path.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut white.header) != 0 {
                return None;
            }
            // Documented as a multiplier of 80 nits, in thousandths.
            let nits = white.SDRWhiteLevel as f32 / 1000.0 * SCRGB_WHITE_NITS;
            return (nits >= SCRGB_WHITE_NITS).then_some(nits);
        }
    }
    None
}

/// Compare two NUL-terminated UTF-16 buffers.
fn wide_eq(a: &[u16], b: &[u16]) -> bool {
    let trim = |s: &[u16]| s.iter().position(|&c| c == 0).map_or(s.len(), |n| n);
    a[..trim(a)] == b[..trim(b)]
}

/// Box-filter taps per axis for scaling `src` pixels down to `dst`.
pub fn taps_for(src: u32, dst: u32) -> u32 {
    if dst == 0 {
        return 1;
    }
    src.div_ceil(dst).clamp(1, MAX_TAPS)
}

/// The shader pipeline and the textures it reuses from frame to frame.
pub struct GpuConverter {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    params: ID3D11Buffer,
    out_w: u32,
    out_h: u32,
    target: ID3D11Texture2D,
    target_view: ID3D11RenderTargetView,
    readback: ID3D11Texture2D,
    /// Shader-readable copy of the source, when the capture texture cannot be
    /// bound directly. Recreated when the source's size or format changes.
    source_copy: Option<(
        ID3D11Texture2D,
        ID3D11ShaderResourceView,
        u32,
        u32,
        DXGI_FORMAT,
    )>,
}

impl GpuConverter {
    /// Build the pipeline for `out_w` x `out_h` output on `device`.
    pub fn new(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        out_w: u32,
        out_h: u32,
    ) -> anyhow::Result<Self> {
        let vs_blob = compile(SHADER, s!("vs_main"), s!("vs_4_0"))?;
        let ps_blob = compile(SHADER, s!("ps_main"), s!("ps_4_0"))?;

        // SAFETY: blobs are live compiler output; descriptors are fully
        // initialised; every created object is checked.
        unsafe {
            let mut vs = None;
            device.CreateVertexShader(blob_bytes(&vs_blob), None, Some(&mut vs))?;
            let mut ps = None;
            device.CreatePixelShader(blob_bytes(&ps_blob), None, Some(&mut ps))?;

            let sampler_desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                MaxLOD: f32::MAX,
                ..Default::default()
            };
            let mut sampler = None;
            device.CreateSamplerState(&sampler_desc, Some(&mut sampler))?;

            let params_desc = D3D11_BUFFER_DESC {
                ByteWidth: std::mem::size_of::<Params>() as u32,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                ..Default::default()
            };
            let mut params = None;
            device.CreateBuffer(&params_desc, None, Some(&mut params))?;

            let target = texture(
                device,
                out_w,
                out_h,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                D3D11_USAGE_DEFAULT,
                D3D11_BIND_RENDER_TARGET.0 as u32,
                0,
            )?;
            let mut target_view = None;
            device.CreateRenderTargetView(&target, None, Some(&mut target_view))?;
            let readback = texture(
                device,
                out_w,
                out_h,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                D3D11_USAGE_STAGING,
                0,
                D3D11_CPU_ACCESS_READ.0 as u32,
            )?;

            Ok(Self {
                device: device.clone(),
                context: context.clone(),
                vs: vs.ok_or_else(|| anyhow::anyhow!("vertex shader not created"))?,
                ps: ps.ok_or_else(|| anyhow::anyhow!("pixel shader not created"))?,
                sampler: sampler.ok_or_else(|| anyhow::anyhow!("sampler not created"))?,
                params: params.ok_or_else(|| anyhow::anyhow!("constant buffer not created"))?,
                out_w,
                out_h,
                target,
                target_view: target_view
                    .ok_or_else(|| anyhow::anyhow!("render target view not created"))?,
                readback,
                source_copy: None,
            })
        }
    }

    /// Scale and convert `source` into the letterboxed output, and read it
    /// back as tightly packed BGRA of the output size.
    pub fn convert(
        &mut self,
        source: &ID3D11Texture2D,
        color: DisplayColor,
    ) -> anyhow::Result<Vec<u8>> {
        // SAFETY: all resources belong to `self.device`; the mapped pointer is
        // read only between a successful Map and its Unmap.
        unsafe {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            source.GetDesc(&mut desc);
            let view = self.source_view(source, &desc)?;

            let (cw, ch) =
                super::scale::fit_within(desc.Width, desc.Height, self.out_w, self.out_h);
            let off_x = ((self.out_w - cw) / 2) & !1;
            let off_y = ((self.out_h - ch) / 2) & !1;

            let hdr = color.hdr && is_float_format(desc.Format);
            let params = Params {
                src_texel: [1.0 / desc.Width as f32, 1.0 / desc.Height as f32],
                footprint: [
                    (desc.Width as f32 / cw as f32).max(1.0),
                    (desc.Height as f32 / ch as f32).max(1.0),
                ],
                sdr_scale: color.sdr_scale(),
                hdr: u32::from(hdr),
                taps: taps_for(desc.Width, cw).max(taps_for(desc.Height, ch)),
                pad: 0,
            };
            self.context.UpdateSubresource(
                &self.params,
                0,
                None,
                std::ptr::from_ref(&params).cast(),
                0,
                0,
            );

            self.context
                .ClearRenderTargetView(&self.target_view, &[0.0, 0.0, 0.0, 1.0]);
            self.context
                .OMSetRenderTargets(Some(&[Some(self.target_view.clone())]), None);
            self.context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: off_x as f32,
                TopLeftY: off_y as f32,
                Width: cw as f32,
                Height: ch as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            self.context.IASetInputLayout(None);
            self.context
                .IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.context.VSSetShader(&self.vs, None);
            self.context.PSSetShader(&self.ps, None);
            self.context.PSSetShaderResources(0, Some(&[Some(view)]));
            self.context
                .PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            self.context
                .PSSetConstantBuffers(0, Some(&[Some(self.params.clone())]));
            self.context.Draw(3, 0);

            // Unbind, so the next frame's copy into the source is not a
            // read-write hazard the runtime has to resolve for us.
            self.context.PSSetShaderResources(0, Some(&[None]));
            self.context.OMSetRenderTargets(None, None);

            self.context.CopyResource(&self.readback, &self.target);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&self.readback, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let row = self.out_w as usize * 4;
            let mut out = vec![0u8; row * self.out_h as usize];
            if !mapped.pData.is_null() {
                for y in 0..self.out_h as usize {
                    let src = (mapped.pData as *const u8).add(y * mapped.RowPitch as usize);
                    std::ptr::copy_nonoverlapping(src, out.as_mut_ptr().add(y * row), row);
                }
            }
            self.context.Unmap(&self.readback, 0);
            Ok(out)
        }
    }

    /// A shader view of `source`, binding it directly when it allows that
    /// and otherwise through a reused copy.
    unsafe fn source_view(
        &mut self,
        source: &ID3D11Texture2D,
        desc: &D3D11_TEXTURE2D_DESC,
    ) -> anyhow::Result<ID3D11ShaderResourceView> {
        if desc.BindFlags & D3D11_BIND_SHADER_RESOURCE.0 as u32 != 0 {
            let mut view = None;
            self.device
                .CreateShaderResourceView(source, None, Some(&mut view))?;
            return view.ok_or_else(|| anyhow::anyhow!("source view not created"));
        }
        let stale = match &self.source_copy {
            Some((_, _, w, h, f)) => *w != desc.Width || *h != desc.Height || *f != desc.Format,
            None => true,
        };
        if stale {
            let copy = texture(
                &self.device,
                desc.Width,
                desc.Height,
                desc.Format,
                D3D11_USAGE_DEFAULT,
                D3D11_BIND_SHADER_RESOURCE.0 as u32,
                0,
            )?;
            let mut view = None;
            self.device
                .CreateShaderResourceView(&copy, None, Some(&mut view))?;
            let view = view.ok_or_else(|| anyhow::anyhow!("copy view not created"))?;
            self.source_copy = Some((copy, view, desc.Width, desc.Height, desc.Format));
        }
        let (copy, view, ..) = self
            .source_copy
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("source copy unavailable"))?;
        self.context.CopyResource(copy, source);
        Ok(view.clone())
    }
}

fn is_float_format(format: DXGI_FORMAT) -> bool {
    format == DXGI_FORMAT_R16G16B16A16_FLOAT
}

/// Create a 2D texture with one mip and one array slice.
unsafe fn texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    usage: D3D11_USAGE,
    bind: u32,
    cpu: u32,
) -> anyhow::Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: usage,
        BindFlags: bind,
        CPUAccessFlags: cpu,
        MiscFlags: 0,
    };
    let mut tex = None;
    device.CreateTexture2D(&desc, None, Some(&mut tex))?;
    tex.ok_or_else(|| anyhow::anyhow!("texture not created"))
}

fn compile(source: &str, entry: PCSTR, target: PCSTR) -> anyhow::Result<ID3DBlob> {
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: `source` outlives the call; out-params are checked below.
    let result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            s!("capture_convert"),
            None,
            None,
            entry,
            target,
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(e) = result {
        let detail = errors
            .map(|b| String::from_utf8_lossy(unsafe { blob_bytes(&b) }).into_owned())
            .unwrap_or_default();
        anyhow::bail!("shader compile failed: {e} {detail}");
    }
    code.ok_or_else(|| anyhow::anyhow!("shader compiler returned no code"))
}

/// The bytes of a compiler blob, valid while `blob` lives.
unsafe fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;

    /// WARP is Windows' software rasteriser: present on every install, so the
    /// shader runs in CI without a GPU.
    fn warp() -> (ID3D11Device, ID3D11DeviceContext) {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                windows::Win32::Foundation::HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .expect("WARP device");
        }
        (device.unwrap(), context.unwrap())
    }

    /// IEEE half-precision bits for a normal, positive `v` (all these tests
    /// use), rounded to nearest.
    fn f16_bits(v: f32) -> u16 {
        let bits = v.to_bits();
        let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
        assert!(
            (1..31).contains(&exp) && v > 0.0,
            "test value {v} out of range"
        );
        let mant = bits & 0x7f_ffff;
        let half = ((exp as u32) << 10) | (mant >> 13);
        (half + ((mant >> 12) & 1)) as u16
    }

    /// A solid FP16 scRGB texture, as WGC hands back on an HDR display.
    fn scrgb_texture(device: &ID3D11Device, w: u32, h: u32, rgb: [f32; 3]) -> ID3D11Texture2D {
        let px = [
            f16_bits(rgb[0]),
            f16_bits(rgb[1]),
            f16_bits(rgb[2]),
            f16_bits(1.0),
        ];
        let data: Vec<u16> = (0..w * h).flat_map(|_| px).collect();
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: 0,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let init = D3D11_SUBRESOURCE_DATA {
            pSysMem: data.as_ptr().cast(),
            SysMemPitch: w * 8,
            SysMemSlicePitch: 0,
        };
        let mut tex = None;
        unsafe { device.CreateTexture2D(&desc, Some(&init), Some(&mut tex)) }.unwrap();
        tex.unwrap()
    }

    fn centre_bgra(out: &[u8], w: u32, h: u32) -> [u8; 3] {
        let i = ((h / 2 * w + w / 2) * 4) as usize;
        [out[i + 2], out[i + 1], out[i]]
    }

    const HDR_200: DisplayColor = DisplayColor {
        hdr: true,
        sdr_white_nits: 200.0,
    };

    /// The washed-out bug in one assertion: SDR white on an HDR desktop is
    /// scRGB `sdr_white / 80`, and it must come out as full white — not the
    /// grey an unconverted capture produces.
    #[test]
    fn sdr_white_on_an_hdr_desktop_is_white() {
        let (device, context) = warp();
        let src = scrgb_texture(&device, 64, 36, [2.5, 2.5, 2.5]);
        let mut conv = GpuConverter::new(&device, &context, 32, 18).unwrap();
        let out = conv.convert(&src, HDR_200).unwrap();
        let [r, g, b] = centre_bgra(&out, 32, 18);
        assert!(r >= 245 && g >= 245 && b >= 245, "got {r},{g},{b}");
    }

    /// Mid-grey must come out as mid-grey in sRGB, which is what "not washed
    /// out" means below white: an 18% linear grey encodes to about 118.
    #[test]
    fn hdr_midtones_are_srgb_encoded() {
        let (device, context) = warp();
        let linear = 0.18 * 2.5; // 18% of a 200-nit white, in scRGB
        let src = scrgb_texture(&device, 64, 36, [linear, linear, linear]);
        let mut conv = GpuConverter::new(&device, &context, 32, 18).unwrap();
        let out = conv.convert(&src, HDR_200).unwrap();
        let [r, ..] = centre_bgra(&out, 32, 18);
        assert!((110..=126).contains(&r), "18% grey encoded to {r}");
    }

    /// A highlight brighter than SDR white is brought into range by its largest
    /// channel, so it stays the colour it was instead of washing out to white.
    #[test]
    fn highlights_keep_their_hue() {
        let (device, context) = warp();
        // A saturated 1000-nit red: 12.5 in scRGB.
        let src = scrgb_texture(&device, 64, 36, [12.5, 1.0, 1.0]);
        let mut conv = GpuConverter::new(&device, &context, 32, 18).unwrap();
        let out = conv.convert(&src, HDR_200).unwrap();
        let [r, g, b] = centre_bgra(&out, 32, 18);
        // (5, 0.4, 0.4) after SDR scaling, divided by its peak: (1, 0.08,
        // 0.08), which sRGB-encodes to about (255, 79, 79). A per-channel clip
        // would have given (255, 170, 170) — pink.
        assert!(r >= 250, "the peak channel reaches full scale, got {r}");
        assert!(
            (65..=95).contains(&g) && g == b,
            "hue must survive: {r},{g},{b}"
        );
    }

    /// The output is letterboxed to the configured size, black outside the
    /// picture, whatever the source's aspect.
    #[test]
    fn a_narrow_source_is_letterboxed() {
        let (device, context) = warp();
        let src = scrgb_texture(&device, 36, 64, [2.5, 2.5, 2.5]);
        let mut conv = GpuConverter::new(&device, &context, 64, 36).unwrap();
        let out = conv.convert(&src, HDR_200).unwrap();
        assert_eq!(out.len(), 64 * 36 * 4);
        assert_eq!(&out[..3], &[0, 0, 0], "left bar is black");
        let [r, ..] = centre_bgra(&out, 64, 36);
        assert!(r >= 245);
    }

    /// An ordinary SDR desktop goes through the same shader for its scaling
    /// alone: colour must come out exactly as it went in.
    #[test]
    fn an_sdr_source_is_scaled_without_a_colour_change() {
        let (device, context) = warp();
        let (w, h) = (128u32, 72u32);
        let data: Vec<u8> = (0..w * h).flat_map(|_| [50u8, 100, 200, 255]).collect();
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let init = D3D11_SUBRESOURCE_DATA {
            pSysMem: data.as_ptr().cast(),
            SysMemPitch: w * 4,
            SysMemSlicePitch: 0,
        };
        let mut src = None;
        unsafe { device.CreateTexture2D(&desc, Some(&init), Some(&mut src)) }.unwrap();
        let mut conv = GpuConverter::new(&device, &context, 64, 36).unwrap();
        // Even told the display is HDR, an 8-bit source is not re-encoded.
        let out = conv.convert(&src.unwrap(), HDR_200).unwrap();
        let [r, g, b] = centre_bgra(&out, 64, 36);
        for (got, want) in [(r, 200u8), (g, 100), (b, 50)] {
            assert!(got.abs_diff(want) <= 1, "got {r},{g},{b}");
        }
    }

    #[test]
    fn taps_follow_the_reduction() {
        assert_eq!(taps_for(1920, 1920), 1);
        assert_eq!(taps_for(3840, 1920), 2);
        assert_eq!(taps_for(3840, 1280), 3);
        assert_eq!(taps_for(7680, 640), MAX_TAPS);
        assert_eq!(taps_for(100, 0), 1);
    }

    #[test]
    fn sdr_scale_puts_sdr_white_at_one() {
        assert!((HDR_200.sdr_scale() * 2.5 - 1.0).abs() < 1e-6);
        assert_eq!(DisplayColor::SDR.sdr_scale(), 1.0);
    }
}
