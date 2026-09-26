/*
 * Narrow C surface over libvpx's VP8 and VP9 encoders and decoders.
 *
 * Rust talks to this file, not to libvpx directly. libvpx's public API is
 * built on large structs -- vpx_codec_ctx_t, vpx_codec_enc_cfg_t (fifty-odd
 * fields), vpx_image_t -- whose layouts depend on the same vpx_config.h the
 * build generates. Redeclaring those in Rust means hand-maintaining a byte
 * exact mirror of a struct that changes with configuration, and getting it
 * subtly wrong yields memory corruption rather than a compile error.
 *
 * So the FFI boundary is drawn here instead: opaque pointers and plain
 * integers, with signatures this project owns. The Rust side cannot get a
 * layout wrong because it never names a libvpx type.
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "vpx/vpx_codec.h"
#include "vpx/vpx_decoder.h"
#include "vpx/vpx_encoder.h"
#include "vpx/vp8cx.h"
#include "vpx/vp8dx.h"

/* Codec selector shared with lib.rs. */
#define CQ_VP8 0
#define CQ_VP9 1

static vpx_codec_iface_t *enc_iface(int codec) {
  switch (codec) {
    case CQ_VP8: return vpx_codec_vp8_cx();
    case CQ_VP9: return vpx_codec_vp9_cx();
    default: return NULL;
  }
}

static vpx_codec_iface_t *dec_iface(int codec) {
  switch (codec) {
    case CQ_VP8: return vpx_codec_vp8_dx();
    case CQ_VP9: return vpx_codec_vp9_dx();
    default: return NULL;
  }
}

struct cq_vpx_enc {
  vpx_codec_ctx_t ctx;
  /* The live configuration, kept so a bitrate change can modify one field and
   * hand the rest back unchanged. */
  vpx_codec_enc_cfg_t cfg;
  vpx_image_t img;
  int width;
  int height;
  /* Frame counter, in the timebase given at construction. libvpx wants a
   * monotonically increasing pts; the caller does not supply one. */
  int64_t pts;
  /* Set by cq_vpx_enc_request_keyframe, consumed by the next encode. */
  int force_keyframe;
  int codec;
  int fps;
  /* Target ceiling for one frame, 0 for libvpx's own judgement. Kept so a
   * bitrate change can re-derive the percentage libvpx takes it as. */
  int max_frame_bytes;
};

struct cq_vpx_dec {
  vpx_codec_ctx_t ctx;
  /* The picture from the last decode, owned by libvpx and valid until the next
   * decode call. Held so a caller whose buffer was too small can copy it out
   * after growing the buffer, instead of decoding the packet a second time --
   * which would advance an inter-predicted stream past the frame it wanted. */
  vpx_image_t *pending;
};

/* ---- Encoder ---------------------------------------------------------- */

/* log2 of the VP9 tile-column count for this width and thread budget.
 *
 * Tiles are what let VP9 encode (and decode) on several threads at once. Each
 * column must be at least 256 pixels wide, so 640 allows two and 1280 four;
 * past that, more threads than tiles buys nothing. */
static int vp9_tile_columns_log2(int width, int threads) {
  int log2 = 0;
  while ((1 << (log2 + 1)) <= threads && (width >> (log2 + 1)) >= 256) {
    ++log2;
  }
  return log2;
}

/* max_frame_bytes as the percentage of the average frame that libvpx's
 * frame-size caps are expressed in.
 *
 * libvpx has no absolute ceiling, only "this many percent of the per-frame
 * budget", so the same byte cap is a different percentage at every bitrate
 * and frame rate. 0 means no cap, which is also what libvpx reads 0 as. */
static unsigned int frame_cap_pct(int max_frame_bytes, int bitrate_bps,
                                  int fps) {
  if (max_frame_bytes <= 0 || bitrate_bps <= 0 || fps <= 0) return 0;
  double per_frame = (double)bitrate_bps / 8.0 / (double)fps;
  double pct = (double)max_frame_bytes * 100.0 / per_frame;
  if (pct < 1.0) return 1;
  if (pct > 1000000.0) return 1000000;
  return (unsigned int)pct;
}

/* Hold keyframes (and, on VP9, inter frames) to max_frame_bytes.
 *
 * A keyframe is by far the largest frame and the one a receiver cannot start
 * without. Left alone, libvpx sizes it from the rate controller's buffer, and
 * a detailed 1080p desktop comes out several times larger than the transport
 * can fragment -- so it is dropped, the receiver asks for another, and that
 * one is dropped too. VP8 has no inter-frame control; its inter frames sit far
 * below a keyframe at any realtime rate. */
static void apply_frame_caps(struct cq_vpx_enc *e, int bitrate_bps) {
  unsigned int pct = frame_cap_pct(e->max_frame_bytes, bitrate_bps, e->fps);
  vpx_codec_control(&e->ctx, VP8E_SET_MAX_INTRA_BITRATE_PCT, pct);
  if (e->codec == CQ_VP9) {
    vpx_codec_control(&e->ctx, VP9E_SET_MAX_INTER_BITRATE_PCT, pct);
  }
}

struct cq_vpx_enc *cq_vpx_enc_new(int codec, int width, int height,
                                  int bitrate_bps, int fps,
                                  int keyframe_interval_secs, int cpu_used,
                                  int threads, int max_frame_bytes) {
  if (width <= 0 || height <= 0 || fps <= 0) return NULL;
  vpx_codec_iface_t *iface = enc_iface(codec);
  if (!iface) return NULL;
  if (threads < 1) threads = 1;

  struct cq_vpx_enc *e = (struct cq_vpx_enc *)calloc(1, sizeof(*e));
  if (!e) return NULL;

  vpx_codec_enc_cfg_t *cfg = &e->cfg;
  if (vpx_codec_enc_config_default(iface, cfg, 0)) {
    free(e);
    return NULL;
  }

  cfg->g_w = (unsigned int)width;
  cfg->g_h = (unsigned int)height;
  cfg->g_timebase.num = 1;
  cfg->g_timebase.den = fps;
  cfg->rc_target_bitrate = (unsigned int)(bitrate_bps / 1000);
  cfg->rc_end_usage = VPX_CBR;
  /* Real-time: no lookahead, and a small buffer so the rate controller reacts
   * within a call's latency budget rather than averaging over seconds. */
  cfg->g_lag_in_frames = 0;
  cfg->g_pass = VPX_RC_ONE_PASS;
  cfg->rc_buf_sz = 1000;
  cfg->rc_buf_initial_sz = 500;
  cfg->rc_buf_optimal_sz = 600;
  /* Let the encoder drop frames rather than blow the bitrate; a dropped frame
   * costs less than a late one on a live link. */
  cfg->rc_dropframe_thresh = 30;
  cfg->kf_mode = VPX_KF_AUTO;
  cfg->kf_max_dist = (unsigned int)(keyframe_interval_secs > 0
                                        ? keyframe_interval_secs * fps
                                        : 4 * fps);
  /* VP8's error-resilient mode is cheap and keeps a lost frame from poisoning
   * the probability tables. VP9's disables temporal prediction of motion
   * vectors and costs real efficiency; a VP9 stream recovers from loss through
   * the keyframe requests the receiver already sends. */
  cfg->g_error_resilient = codec == CQ_VP8 ? VPX_ERROR_RESILIENT_DEFAULT : 0;
  cfg->g_threads = (unsigned int)threads;

  if (vpx_codec_enc_init(&e->ctx, iface, cfg, 0)) {
    free(e);
    return NULL;
  }

  /* cpu_used trades quality for speed; realtime calls live at the fast end. */
  vpx_codec_control(&e->ctx, VP8E_SET_CPUUSED, cpu_used);

  if (codec == CQ_VP9) {
    /* The realtime settings WebRTC runs VP9 with. Cyclic-refresh AQ spends
     * bits where the picture changed, which is most of a call's quality win
     * over VP8; row-based multithreading and tile columns are what let the
     * thread budget actually be used. */
    vpx_codec_control(&e->ctx, VP9E_SET_AQ_MODE, 3);
    vpx_codec_control(&e->ctx, VP9E_SET_ROW_MT, threads > 1 ? 1 : 0);
    vpx_codec_control(&e->ctx, VP9E_SET_TILE_COLUMNS,
                      vp9_tile_columns_log2(width, threads));
    vpx_codec_control(&e->ctx, VP9E_SET_FRAME_PARALLEL_DECODING, 0);
  }

  if (!vpx_img_alloc(&e->img, VPX_IMG_FMT_I420, (unsigned int)width,
                     (unsigned int)height, 1)) {
    vpx_codec_destroy(&e->ctx);
    free(e);
    return NULL;
  }

  e->width = width;
  e->height = height;
  e->pts = 0;
  e->force_keyframe = 0;
  e->codec = codec;
  e->fps = fps;
  e->max_frame_bytes = max_frame_bytes > 0 ? max_frame_bytes : 0;
  apply_frame_caps(e, bitrate_bps);
  return e;
}

void cq_vpx_enc_free(struct cq_vpx_enc *e) {
  if (!e) return;
  vpx_img_free(&e->img);
  vpx_codec_destroy(&e->ctx);
  free(e);
}

void cq_vpx_enc_request_keyframe(struct cq_vpx_enc *e) {
  if (e) e->force_keyframe = 1;
}

int cq_vpx_enc_set_bitrate(struct cq_vpx_enc *e, int bitrate_bps) {
  if (!e || bitrate_bps <= 0) return -1;
  /* vpx_codec_enc_config_set on a running encoder retargets the rate
   * controller without resetting reference frames, which is what keeps a rate
   * change from costing a keyframe. */
  e->cfg.rc_target_bitrate = (unsigned int)(bitrate_bps / 1000);
  if (vpx_codec_enc_config_set(&e->ctx, &e->cfg)) return -1;
  /* The byte cap is fixed but the per-frame budget just moved. */
  apply_frame_caps(e, bitrate_bps);
  return 0;
}

/*
 * Encode one I420 frame.
 *
 * Plane pointers are tightly packed (stride == width for Y, width/2 for U/V),
 * matching the caller's RawFrame. Output is written into `out` and the byte
 * count returned; `*is_keyframe` reports whether the encoder produced one.
 *
 * Returns the encoded length, 0 when the encoder produced nothing for this
 * frame (a legitimate outcome when it drops under rate control), or -1 on
 * error. A return of -2 means the output buffer was too small.
 */
int cq_vpx_enc_encode(struct cq_vpx_enc *e, const uint8_t *y, const uint8_t *u,
                      const uint8_t *v, uint8_t *out, int out_cap,
                      int *is_keyframe) {
  if (!e || !y || !u || !v || !out || !is_keyframe) return -1;
  *is_keyframe = 0;

  const int w = e->width;
  const int h = e->height;
  const int cw = (w + 1) / 2;
  const int ch = (h + 1) / 2;

  /* Copy row by row: the caller's planes are tightly packed, libvpx's are
   * stride-aligned, and the two only coincide by accident. */
  for (int r = 0; r < h; ++r) {
    memcpy(e->img.planes[VPX_PLANE_Y] + (size_t)r * e->img.stride[VPX_PLANE_Y],
           y + (size_t)r * w, (size_t)w);
  }
  for (int r = 0; r < ch; ++r) {
    memcpy(e->img.planes[VPX_PLANE_U] + (size_t)r * e->img.stride[VPX_PLANE_U],
           u + (size_t)r * cw, (size_t)cw);
    memcpy(e->img.planes[VPX_PLANE_V] + (size_t)r * e->img.stride[VPX_PLANE_V],
           v + (size_t)r * cw, (size_t)cw);
  }

  const vpx_enc_frame_flags_t flags = e->force_keyframe ? VPX_EFLAG_FORCE_KF : 0;
  e->force_keyframe = 0;

  if (vpx_codec_encode(&e->ctx, &e->img, e->pts, 1, flags, VPX_DL_REALTIME)) {
    return -1;
  }
  e->pts++;

  int written = 0;
  vpx_codec_iter_t iter = NULL;
  const vpx_codec_cx_pkt_t *pkt;
  while ((pkt = vpx_codec_get_cx_data(&e->ctx, &iter)) != NULL) {
    if (pkt->kind != VPX_CODEC_CX_FRAME_PKT) continue;
    if (written + (int)pkt->data.frame.sz > out_cap) return -2;
    memcpy(out + written, pkt->data.frame.buf, pkt->data.frame.sz);
    written += (int)pkt->data.frame.sz;
    if (pkt->data.frame.flags & VPX_FRAME_IS_KEY) *is_keyframe = 1;
  }
  return written;
}

/* ---- Decoder ---------------------------------------------------------- */

struct cq_vpx_dec *cq_vpx_dec_new(int codec, int threads) {
  vpx_codec_iface_t *iface = dec_iface(codec);
  if (!iface) return NULL;
  struct cq_vpx_dec *d = (struct cq_vpx_dec *)calloc(1, sizeof(*d));
  if (!d) return NULL;
  vpx_codec_dec_cfg_t cfg;
  memset(&cfg, 0, sizeof(cfg));
  cfg.threads = (unsigned int)(threads < 1 ? 1 : threads);
  if (vpx_codec_dec_init(&d->ctx, iface, &cfg, 0)) {
    free(d);
    return NULL;
  }
  return d;
}

void cq_vpx_dec_free(struct cq_vpx_dec *d) {
  if (!d) return;
  vpx_codec_destroy(&d->ctx);
  free(d);
}

/* Copy `img` into `out` as tightly-packed I420. Returns bytes written, -1 for
 * a format this copy does not handle, or -2 when `out_cap` is too small. */
static int copy_i420(const vpx_image_t *img, uint8_t *out, int out_cap,
                     int *width, int *height) {
  if (img->fmt != VPX_IMG_FMT_I420) return -1;

  const int w = (int)img->d_w;
  const int h = (int)img->d_h;
  const int cw = (w + 1) / 2;
  const int ch = (h + 1) / 2;
  *width = w;
  *height = h;

  const int needed = w * h + 2 * cw * ch;
  if (!out || needed > out_cap) return -2;

  int off = 0;
  for (int r = 0; r < h; ++r) {
    memcpy(out + off, img->planes[VPX_PLANE_Y] + (size_t)r * img->stride[VPX_PLANE_Y],
           (size_t)w);
    off += w;
  }
  for (int r = 0; r < ch; ++r) {
    memcpy(out + off, img->planes[VPX_PLANE_U] + (size_t)r * img->stride[VPX_PLANE_U],
           (size_t)cw);
    off += cw;
  }
  for (int r = 0; r < ch; ++r) {
    memcpy(out + off, img->planes[VPX_PLANE_V] + (size_t)r * img->stride[VPX_PLANE_V],
           (size_t)cw);
    off += cw;
  }
  return off;
}

/*
 * Decode one frame into tightly-packed I420.
 *
 * `*width`/`*height` receive the decoded dimensions, which the caller cannot
 * know in advance -- a stream carries its own size and may change it. Returns
 * bytes written, -1 on decode error, -2 when `out_cap` is too small (grow the
 * buffer to the reported size and call cq_vpx_dec_copy_pending), or -3 when the
 * packet decoded cleanly but holds no picture to show. VP9 can carry frames
 * that only update references; a realtime stream with no lookahead should not
 * produce them, but a decoder must not call one an error.
 */
int cq_vpx_dec_decode(struct cq_vpx_dec *d, const uint8_t *data, int len,
                      uint8_t *out, int out_cap, int *width, int *height) {
  if (!d || !data || len <= 0 || !width || !height) return -1;
  d->pending = NULL;

  if (vpx_codec_decode(&d->ctx, data, (unsigned int)len, NULL, 0)) return -1;

  vpx_codec_iter_t iter = NULL;
  vpx_image_t *img = vpx_codec_get_frame(&d->ctx, &iter);
  if (!img) return -3;
  d->pending = img;
  return copy_i420(img, out, out_cap, width, height);
}

/* Copy the picture the last decode produced, after a -2. */
int cq_vpx_dec_copy_pending(struct cq_vpx_dec *d, uint8_t *out, int out_cap,
                            int *width, int *height) {
  if (!d || !d->pending || !width || !height) return -1;
  return copy_i420(d->pending, out, out_cap, width, height);
}
