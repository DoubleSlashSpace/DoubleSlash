# Manual acceptance: Phase 1

Scripts for the Phase 1 checks in [`backlog.md`](../backlog.md) that no unit or
harness test can stand in for. Each one lists its setup, the steps, what counts
as a pass, and the log lines that serve as evidence. A check passes only when
someone has run it on the stated build and recorded the result under
[Results](#results). Compiling does not count, and neither do the harness tests.

## Setup common to every check

- **Two desktop profiles on one machine.** Launch the built binary directly,
  once per profile, with its own home and key directory:

  ```bat
  set DOUBLESLASH_HOME=Z:\ds-accept\A
  set DOUBLESLASH_KEY_DIR=%DOUBLESLASH_HOME%
  rust\doubleslash-client\target\release\doubleslash-client.exe
  ```

  Do the same for `B` from a second console. `run_client.bat` always uses
  `.clientA`, so it cannot provide the second profile.
- **Logs.** Each profile writes to `%DOUBLESLASH_HOME%\logs`. Android logs to
  logcat with tag `DoubleSlash`. Logcat keeps only about a minute of history,
  so start `adb logcat -v time > phone.log` *before* the step being tested.
- **Use a private two-party room**, not the public `default` room. Members of
  `default` run clients you do not control, and a single out-of-date client
  acting as a competing keyer looks exactly like a local bug.
- **Record the build:** the `git rev-parse --short HEAD` of each client and the
  supernode version reported by the manager.

## 1. Group-key convergence (live)

Unit and harness tests cover election padding, catch-up minting, forward epoch
jumps, reseal to a lagging member, and a listen-only member asking for a
missed epoch. These steps confirm the same behaviour between real clients.

1. A creates a private room and invites B. Both join voice.
2. A third profile or device C joins and then leaves, so the keyer rotates.
   Repeat twice so the room reaches at least epoch 2.
3. B is the listener: B stays muted the whole time. A talks.
4. Kill B's process during a rotation, restart it, and rejoin. Separately, cut
   B's network for about 30 s during a rotation, then restore it.

**Pass:** B hears A within a few seconds after every step, and both clients'
logs show the same final epoch for the room.

**Evidence:** Capture the **keyer's** log, not just the member's. The elected
keyer is the member whose public id sorts lowest. Look for these lines:

- `[group-key] keyer view of room <id> changed via <node>: -[..] +[..] ... per node ...`
  shows each change to the keyer's membership union and which supernode
  snapshot caused it. If a member vanished from every node's snapshot at once,
  this line shows it.
- `[group-key] rotating room <id> to epoch N after [..] left; sealing to [..]`
- `[group-key] no key for room <id>; asking keyer <id> (attempt n)` on the
  member, followed by `[group-key] installed epoch N` on the member.

If a member is still stranded, attach both logs to the backlog item. The
unexplained "desktop never got epochs 2 and 3" case needs exactly this pair.

## 2. Android: private-room voice and the background

Needs a phone with an APK built from the same commit, plus one desktop.

1. **Room voice:** desktop and phone join a private two-party room (not
   `default`). Talk in both directions for 60 s.
2. **Screen off:** with the room active, lock the phone for 2 minutes, then
   unlock it.
3. **Doze:** with the phone unplugged, force idle with
   `adb shell dumpsys deviceidle force-idle`, wait 2 minutes, then run
   `adb shell dumpsys deviceidle unforce`.
4. **Handover:** during a direct 1:1 call, switch the phone from Wi-Fi to
   cellular, then back.

**Pass:** Audio flows both ways before and after each step. The persistent
connected notification stays up the whole time, and the session reconnects
without the user opening the app. Record how long the audio gap was after each
disruption.

## 3. Windows two-client + room media

This checklist decides whether "video calls ship". Run it on two machines if
you can. On one machine, two cameras (or one camera and screen share) will do.

| # | Step | Pass |
|---|---|---|
| 3.1 | A calls B directly and both turn cameras on | Each side sees the other's camera, and the local preview shows the local camera, not the remote one |
| 3.2 | Room with three members, two cameras on | Every member sees both cameras. A camera-off member shows a placeholder, not a frozen frame |
| 3.3 | A fourth member joins mid-stream | The joiner sees video within about 2 s (keyframe recovery), not after the next scheduled keyframe |
| 3.4 | Block direct UDP between A and B (firewall rule), then call | After about 5 s the log shows `No direct QUIC to … within fallback grace` then `Direct-call fallback for …: creating temp private room`, and **video** still flows over the fallback room |
| 3.5 | Unplug the camera, or start with none | The UI shows the camera as unavailable. The log shows `[video] could not open camera source` or `[video] capture ended on its own`, with no crash or hang |
| 3.6 | Open the camera in another app first | Same as 3.5 |
| 3.7 | Run a build without a video encoder, or force encoder failure | `[video] no usable … encoder` and an explicit unavailable state |
| 3.8 | Camera off (user action) vs. sender stall (kill the sender's network) | Receivers show "camera off" for the first case and a stall for the second (`SfuVideoState`). The two must not look the same |
| 3.9 | Throttle the uplink (see §4) until quota or ABR sheds | Video degrades (lower bitrate or resolution) and voice stays intact |

## 4. A/V sync on a real network

Target: audio leads by 40–80 ms, never lags. Unit coverage uses a fake clock.

**Measure with a clap and flash.** Share a window that shows a full-screen
flash timed to a sharp sound (or clap in front of a camera with a light).
Record the *receiving* screen and speaker with a phone at 120 or 240 fps. Count
the frames between the flash and the audio transient.

Run each condition for at least 3 measurements:

1. Clean LAN.
2. 1–2 % random loss. On Windows use [clumsy](https://jagt.github.io/clumsy/)
   (Drop, 1.5 %, outbound). On Linux use
   `tc qdisc add dev <if> root netem loss 1.5%`.
3. Keyframe burst: during the test, have a new member join (forcing a keyframe)
   or toggle the camera.
4. **Multi-peer:** two senders share content audio + video at the same time.
   Each receiver's per-sender sync must hold independently. One sender's
   stall must not freeze or skew the other's picture.

**Pass:** Every measurement falls in 0 to +80 ms (audio first). None shows
video leading audio, even under loss.

**Voice unchanged (already confirmed, 2026-09-22):** the direct voice datagram
(`[AUDIO_TAG][id_len][peer_id][opus]`), the room `SfuAudio` envelope, the
sealed frame layout, the voice AAD, and the Opus encoder settings all match
the pre-video code at `3c2ceb5f^`. The `voice_aad_bytes_are_frozen` and
`voice_media_aad_is_identical_to_legacy_voice_aad` tests pin the AAD.

## 5. Linux / macOS camera hardware

**Linux:** needs a real Linux machine, a VM with true USB passthrough, or a
kernel with `vivid`. WSL's `usbipd` cannot stream UVC because it lacks
isochronous support.

```sh
cd rust/doubleslash-client
cargo test --lib video::camera -- --ignored --nocapture --test-threads=1
```

Record the `offers:` / `chose:` / `negotiated` lines. Then, with the client
running a call:

- **Unplug mid-call:** the capture ends as "camera lost". It must not hang
  the call or panic.
- **Starvation:** a stalled driver now times out every 2 s and is reported
  lost after three stalls in a row (`camera delivered no frame in 6s`). Ending
  the call during a stall must release the camera within about 2 s.

**macOS:** same test command on a Mac. The first run triggers the TCC prompt.
Accept it and run again. Record the `requested 640x360, session delivers WxH`
line: that is the answer to whether `AVCaptureSessionPreset` gives the
requested size.

## 6. Portal over real QUIC

Two real devices, not the in-tree fixtures. Follow [`games/README.md`](../games/README.md):
open the same app/room on both, then check catch-up (join late), coordinator
leave (close the first page), and the Focus/Session layout. Focus Timer assumes
synchronised clocks, so skew between devices is a known demo limit.

## Results

Add a row per run. Link logs and recordings instead of pasting them.

| Date | Check | Client build(s) | Supernode | Result | Evidence / notes |
|---|---|---|---|---|---|
| 2026-09-22 | 4 (voice unchanged) | `bc366cce`+ | — | Pass | Source comparison against `3c2ceb5f^`; see §4 |
