# tm-stream: handoff spec v0.1 draft

A server-run NES console broadcasting to browsers: video, sound, pad
input and board state on one wire, keyed to one clock, recorded by
appending and replayed by the same code that plays it live. Drafted
2026-10-20 by the implementing agent for the director to ratify. The
only measured numbers in it are `ntsc-wire`'s (section 4.1); every
other number is a **claim for a test to confirm**, marked as such, in
the spirit of the v0.2 signal spec and with its track record in mind
(three of its claims did not survive measurement).

Companion to `docs/ntsc-crt-handoff-v0_3.md` (the signal path) and to
`nes-bus`, whose `DotFrame` is the waist this wire carries. The video
channel is `crates/ntsc-wire` as merged; this document is the three
channels around it and the envelope that holds all four.

## 1. Design principles

1. **Send the cause, not the effect, wherever the receiver can compute
   the effect exactly.** The signal path already does this once: dots
   go on the wire, not the 42.95 MSa/s waveform, because the waveform
   is a pure function of the dots (`ntsc-wire`'s contract, held to the
   sample). Sound is the same shape: APU register writes, not PCM, and
   the client's APU synthesises. Pad input is the limit case, the one
   thing nothing else determines, so it is always on the wire. The
   principle stops where the client cannot keep up: the 2C02 is
   switch-level and the NTSC decode alone runs at 41 fps in WASM
   (`docs/perf-report.md`), so the console runs on the server and video
   stays dots. If a faster PPU ever runs in the browser, the video
   channel becomes optional and nothing else in this document changes.

2. **One clock.** Every packet names a frame number, `seq`, and `seq`
   is the video channel's own `NDOT` sequence. No packet carries wall
   time. The recorder stamps wall time when it writes to disk; the
   dashboard aligns everything by `seq` and never guesses.

3. **Every channel can start cold and recover from a gap the same
   way.** Each channel has a KEY form that stands alone and a delta or
   stream form that depends on what came before. On any gap the
   receiver discards that channel's state and waits for its next KEY.
   The server's key cadence is therefore the longest any viewer ever
   waits for full context, and it is a number, not a hope.

4. **Frames are the unit of dropping.** When a relay must shed load it
   drops whole frames, every channel together, never one channel's
   packet alone. A receiver then sees one gap on every channel and
   principle 3 applies uniformly. A pad packet that arrives without its
   frame's video is still a pad packet; a frame with video and no pad
   packet is a bug, and the tests say so.

5. **Live and recorded are the same bytes.** A recording is the
   envelopes appended to a file. Playback feeds them to the same
   receiver. Scrubbing is "restore the last state snapshot before N,
   replay forward". No second format, no transcoding step, nothing the
   live path does not already do.

6. **The wire carries board state, it does not understand it.** The
   state channel is the console's own serde output, opaque to this
   crate. A dashboard decodes it with the console's types. Nothing
   here knows what a PPU is.

7. Every claim below has a test named beside it, `MUTATE=1` has
   something to break in every milestone, and no em dashes in prose.

## 2. Crate layout

```
crates/ntsc-wire       the video channel: DotFrame framing (merged, PR #3)
crates/tm-stream       the envelope, the pad and audio codecs, the
                       state and marker channels, the stream file
crates/tm-relay        (later) fan-out server: routes by channel byte,
                       never inflates a payload, drops by frame
```

`tm-stream` depends on `ntsc-wire` only to assert that a video payload's
own `seq` equals its envelope's. It does not depend on the console.

## 3. The envelope

Eight bytes, all integers little-endian, in front of every packet on
the wire and in the file:

```
0  u8   channel   0 video  1 audio  2 pad  3 state  4 marker  5 control
1  u8   flags     bit 0 KEY; other bits per channel, below
2  u16  len       payload length
4  u32  seq       the frame this packet belongs to
8  payload
```

`len` is `u16` because no packet but state is anywhere near 64 KB, and
state is the one with a length of its own inside (section 4.4), so an
envelope's `len` for state is the first chunk's and the payload says
how many follow. A claim to confirm: no video key exceeds 64 KB on any
frame the test generator or a real cartridge produces (`ntsc-wire`'s
measured key is 4,152 bytes on a tiled scene; a worst-case noise frame
is the number to find).

Within one frame the sender emits, in order: **pad, audio, video,
state (if due), marker (if any)**. A receiver that holds video for `seq`
N therefore already holds the inputs that produced it. The order is a
test, not a convention.

## 4. Channels

### 4.1 Video (channel 0)

The payload is an `NDOT` packet exactly as `ntsc-wire` produces it,
including its own 20-byte header. The envelope's KEY flag mirrors the
NDOT KEY flag and the envelope's `seq` equals the NDOT `seq`; a relay
routes on the envelope and never opens the NDOT header; the receiver
checks both agree and refuses the packet if not.

Measured (`cargo test -p ntsc-wire print_sizes -- --ignored --nocapture`,
2026-10-20, at the commit in PR #3): 68,486 picture dots raw per frame;
key of a tiled scene 4,152 B; delta with one sprite walking at most
169 B; still scene at most 103 B including the NDOT header. At 60 fps
with a key every 60 frames: about 10 KB/s per viewer worst case on that
scene. Sabotage: 7 of 11 tests go red under `MUTATE=1`.

One loose end on the far side: `ntsc-wasm`'s `Pipeline` has `origin()`
and `advance()` and no setter, and a cold-starting viewer needs to set
the origin phase the NDOT header hands it. One line; not in PR #3.

### 4.2 Audio (channel 1)

Payload: a log of the APU register writes the CPU made during frame
`seq`, in order, four bytes each:

```
u16  cycle   CPU cycle within the frame, 0..29781
u8   reg     register offset in the $4000 window, 0x00..0x17
u8   value
```

`cycle` fits `u16` because a frame is 29,780.5 CPU cycles at 1,789,773
Hz (a claim to confirm from the console's own counters, the way the
grid's rates were confirmed in M0). Writes to $4014 (OAM DMA) and
$4016 (controller strobe) are not APU writes and are not logged; the
set that is logged is $4000..$4013, $4015, $4017. A frame with no
writes is an empty payload, still sent, so that silence and a dropped
packet are different things.

**KEY form.** A key is a full APU board state, serialised the same way
the state channel serialises everything (section 4.4), followed by the
frame's write log. It is not the 22 register values: an APU's length
counters, envelopes, sweep units and frame sequencer are not
recoverable from the registers alone, so a register-only key would
start the synthesiser in the right configuration and the wrong moment.
The key cadence follows video's, and the first audio packet after any
gap is a key.

**The claim this channel stands or falls on:** a client APU started
from a KEY and fed the write log reproduces the server APU's output
sample-for-sample. The test is the audio twin of `ntsc-wire`'s
round-trip test, and it needs an APU on both ends. Which APU runs in
the browser is open question 1 (section 8). The oracle for either is
blargg's `Nes_Snd_Emu`, used the way `nes_ntsc` is used for video:
inside the tests, pinned by hash, never shipped.

Size claim: a typical frame of game music is 10 to 40 writes, so 40 to
160 bytes; a key is the APU state blob plus that. Measure before
believing.

### 4.3 Pad (channel 2)

Payload: `u8 count`, then one byte per controller, the eight buttons in
the standard shift order, bit 0 A, 1 B, 2 Select, 3 Start, 4 Up, 5
Down, 6 Left, 7 Right. It is **the input the server's driver held for
frame `seq`**, the cause, not what the game happened to read through
$4016, which a game may read zero or several times a frame and which
the state channel captures anyway.

Sent every frame, even when identical to the previous frame, so that a
missing pad packet always means a missing frame (principle 4). No delta
form; at two to five bytes a frame there is nothing to save. KEY is
always set, since every pad packet stands alone.

Two players are two bytes. A Zapper or a Four Score is a `count` and a
layout this version does not define; the byte count is in the envelope
either way, so an unknown layout is skipped, not misread.

### 4.4 State (channel 3)

Payload: the console's own serialised board state at the end of frame
`seq`, the serde output `nes-bus` v0.1.8 made every board capable of.
Prefixed by:

```
u8   format   0 bincode  1 JSON
u8   part     this chunk's index
u8   parts    chunks in this snapshot
u8   pad      0
u32  total    whole snapshot's byte length
```

A snapshot larger than one envelope is split across `parts` packets
with the same `seq`, reassembled by the receiver, and refused whole if
any part is missing. KEY is always set.

Cadence: every 120 frames (about two seconds) by default, configurable,
plus one on every marker that says the console was reset or a
cartridge loaded. The cadence is also the scrub resolution of a
recording.

**The number that decides whether this channel is cheap:** the size of
a snapshot. The 2C02 is simulated at its switches, and if its state is
every node's level, a snapshot could be hundreds of kilobytes, which at
one every two seconds is more than every other channel combined and
would push the cadence out. This is open question 2 and is measured
before anything else in milestone S2 is built. If it is large, the
remedy is a delta snapshot (xor against the previous, deflated, exactly
as `ntsc-wire` does dots) with a full one every N, and the format byte
gains a value for it.

### 4.5 Marker (channel 4)

Payload: UTF-8 text, at most the envelope's `len`. Human events the
dashboard draws as ticks on its timeline: `reset`, `cart <name>`,
`player 1 joined`, `recording started`, anything an operator types.
KEY always set. Two markers are reserved and the sender emits them
itself: `reset` and `cart ` trigger a state snapshot on the same `seq`.

### 4.6 Control (channel 5), client to server only

The one packet that flows upstream. Payload: `u8 kind` then per kind:

- `0 subscribe`: a bitmask of channels this client wants. A dashboard
  that only draws telemetry subscribes to pad, state and marker and
  never receives a dot. The relay honours this per client.
- `1 need_key`: a bitmask of channels. The client got a gap or just
  joined and would rather not wait for the cadence; the server emits a
  key on those channels at the next frame. Rate-limited server side so
  a flapping client cannot force a key every frame.
- `2 pad`: for a client that is a player, not a viewer: `count` and the
  pad bytes, same layout as channel 2. Arrives whenever; the server
  applies it to the next frame it starts. Latency is the round trip
  plus up to one frame, and the dashboard can measure it because the
  pad packet the server echoes back on channel 2 carries the `seq` it
  landed on.

## 5. Recovery

Per channel, per receiver:

- **Expected `seq`** is tracked per channel. A packet with the expected
  `seq` is applied. A packet with a later `seq` is a gap: the channel's
  state is discarded, and the packet is applied only if it is a KEY
  (pad and marker always are; state and audio keys are; a video delta
  is refused, exactly as `ntsc-wire` already does).
- **Missed frames** are reported to the caller as a count on the first
  packet after a gap, as `ntsc-wire` reports `missed`, so the video
  decoder can advance phase or simply take the origin the NDOT header
  carries.
- **A packet with an earlier `seq`** than expected is a reorder or a
  replay and is refused; this wire runs over an ordered transport
  (WebSocket, or a WebRTC data channel in ordered mode) and does not
  reorder itself. An unordered transport is a non-goal of v0.1.
- **The relay** drops by frame (principle 4): when a client's send
  buffer passes a threshold, the relay stops forwarding that client
  every packet of the current frame and every frame after it until the
  buffer drains and a frame boundary arrives, then resumes. The client
  sees one gap on every channel at once. Pad is not exempt: a pad
  packet without its frame is of no use to a viewer, and a recorder is
  not a client of the relay, it sits on the server (section 6).

## 6. The stream file

A recording is:

```
"TMST"  magic
u8      version   1
u8      flags     bit 0: has index trailer
u16     reserved
u64     wall_ms   wall-clock time of seq 0, Unix milliseconds
...     envelopes, exactly as sent, in order
trailer (optional) u32 count, then count x (u32 seq, u64 offset)
        of every state snapshot's first packet, then u64 trailer_offset
```

The recorder sits on the server, before the relay, and writes every
packet of every frame; it never sees a drop. The trailer is written on
close; a file cut off mid-stream has no trailer and is still playable
by scanning, which is the test: a file truncated at a random byte
plays up to its last whole packet and the reader refuses the partial
one.

Scrub to `seq` N: find the greatest snapshot `seq` S at or below N (the
trailer, or a scan), restore the console from it, then feed the pad
packets S+1..N to a console running on the dashboard's side if there
is one, or simply seek the video, audio and pad channels to N and
start decoding at their next keys if there is not. The second is what
v0.1 ships; the first is what makes the recording a save state with a
timeline, and it waits on the browser console question.

Playback at speed: the player paces by `seq` at the rendering-enabled
NES rate, 60.09881 Hz exactly (`ntsc-crt-handoff-v0_3.md` section 3.2),
against the display's refresh the way `ntsc-wasm` already does.

## 7. Milestones

Each closes with its tests green, its `MUTATE=1` reds counted, and a
report in `docs/` carrying the run stamp, as M0 through M5 did.

**S0: envelope, pad, marker, file.** `tm-stream` with the envelope
codec, the pad and marker channels, the ordered-emission check, the
per-channel recovery state machine, and the stream file reader and
writer including the truncation test. Video is wrapped by delegating
to `ntsc-wire`. Tests: every frame's packets come out in order; a
dropped frame shows as one gap on every channel; a pad packet is never
missing while a video packet for the same `seq` is present; a file
truncated anywhere plays to its last whole packet. Sabotage: the
emission order swaps pad and video, and the truncation reader accepts
a partial packet; both must go red.

**S1: audio.** The write log codec, the KEY form, and the sample-exact
replay test against an APU on both ends (open question 1 decides which).
Oracle: `Nes_Snd_Emu`, pinned by hash, tests only. Claims confirmed or
corrected: the cycles-per-frame count, the typical write count,
whether a register-only key would in fact have been enough (it is
expected not to be, and the test that shows it is kept as the reason
the key is a state blob). Sabotage: a write's `cycle` is quantised to
the nearest 8, and the output must go red.

**S2: state.** First, measure a snapshot: bytes, by board, bincode and
JSON. Then the chunked channel, the cadence, the marker-triggered
snapshot, and if the measurement says so, delta snapshots. Test:
restore from any snapshot in a recording and the next frame's video,
audio and pad packets apply cleanly, which is the scrub test. Sabotage:
a part is dropped from a chunked snapshot and the reassembler accepts
it; must go red.

**S3: relay and client.** `tm-relay` with per-client subscriptions,
`need_key`, drop-by-frame, and the player path (control channel pad).
A browser client in the console page: `ntsc-wasm` for video with the
`set_origin` line added, the S1 APU for audio, pad and marker drawn
on a timeline, state decoded by the console's own types and shown
raw (a board, a register, a value: generic and free). Measured and
reported: bytes per second per viewer on a real cartridge, relay fan-out
at 10 and 100 clients, and the round trip for a player's pad press
from control packet to the `seq` it landed on.

## 8. Open questions for the director

1. **Which APU runs in the browser.** Three options: the console's own
   APU compiled to WASM, if the console has one that is not switch-
   level and runs faster than real time; a small purpose-built APU in
   `tm-stream` verified against `Nes_Snd_Emu`; or shipping PCM after
   all and giving up principle 1 for sound. The second is the author's
   expectation; the first is better if it exists.

2. **Snapshot size.** Measured before S2 is designed (section 4.4). If
   the switch-level 2C02's state dominates, is a delta snapshot
   acceptable, or should the PPU's state be reduced to its
   architectural registers and memories for the wire, with the
   switch-level detail kept server-side only?

3. **Transport.** WebSocket is enough for viewers and for recording.
   For a player, a WebRTC data channel in ordered-reliable mode is the
   same wire with lower latency; unordered or unreliable modes are out
   of scope for v0.1 because section 5 assumes order. Is the player
   path wanted in S3 or later?

4. **What the dashboard is.** v0.1 assumes a page in the console site
   showing the four channels raw. If the dashboard is meant to become
   a product surface, its own spec follows this one and this wire
   should not grow features for it.

## 9. Things this version does not do

Audio is not delivered as PCM. Video is not re-encoded to anything a
non-tinymachines player could show; that is `ffmpeg` on the server
side from the CRT model's RGB, and it is the YouTube path, not this
one. The wire does not encrypt or authenticate; it runs on the fleet
or behind whatever already does. Nothing here is a game-specific RAM
map; the state channel is the whole board and a dashboard that wants
Mario's X reads it out of the RAM it already has.
