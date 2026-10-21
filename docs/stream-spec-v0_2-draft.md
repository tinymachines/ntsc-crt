# tm-stream: handoff spec v0.2 draft

A server-run NES console broadcasting to browsers, or a browser running
the console from a stream of what the server did. Drafted 2026-10-20 by
the implementing agent for the director to ratify, from the console
session's reading of v0.1 (`docs/stream-spec-v0_1-review.md`), whose
measurements this revision adopts. v0.1 (`docs/stream-spec-v0_1.md`)
is kept for the record, as the signal specs keep theirs.

Status of the numbers: section 4.1's are measured in `ntsc-wire`;
section 4.4's and 4.2's cycle count are measured by the console session
at nes @ 9ebbe30; everything else is still a **claim for a test to
confirm** and is marked as such.

## Changes from v0.1

- **The premise under principle 1 was wrong and is corrected.** v0.1
  assumed the console could not run in a browser (switch-level 2C02,
  decode at 41 fps). The console is table-driven (v2c02-fast,
  v2a03-micro, v2a03-dac) and `nes_wasm` runs all of it with sound at
  91 fps in the browser, measured 2026-09-06. The stream therefore has
  **two readings** (section 1.1): a viewer that runs the console needs
  only pad and state; a viewer that does not needs dots and sound as
  well. Pad plus state is now the primary path. [review, "The premise"]
- **Open question 1 closed: the console's own APU**, held to blargg's
  APU ROMs 8 of 8 and the mixer ROMs under 5 percent. The APU write-log
  channel stays, as the no-console path's sound. [review, OQ1]
- **Open question 2 closed by measurement: snapshots are cheap and the
  delta design is dropped.** A postcard snapshot is 327 to 351 KB raw
  and 3.2 to 9.5 KB deflated; an xor delta against 120 frames earlier
  deflates *larger* than the full. Full snapshot every 120 frames,
  under 5 KB/s. [review, OQ2]
- **State format byte: postcard**, not bincode or JSON. [review, OQ2]
- **Two traps become two tests** in S2: the server drains `frames` and
  the sound `out` buffer before a key (21 MB per 120 frames if not),
  and a key is taken at a CPU cycle end, so its `seq` is "frame N plus
  up to a cycle" and replay restores then runs to the boundary.
  [review, OQ2]
- **Cycles per frame confirmed**: 29,780.67 on a full frame, 29,780.33
  on an odd rendering frame (89,342 or 89,341 dots over 3), `u16`
  holds it. Pad bit order confirmed against `nes_wasm::set_pad`.
  [review, "Claims checked"]
- **Open question 3 closed: player path later**, after S3 measures
  viewers. The control channel stays defined. [review, OQ3]
- **The ROM rule, new section 1.2**: for a commercial cartridge the
  site serves neither the ROM nor the dots; the viewer brings the dump.
  [review, "Where this meets nes-bench"]
- This spec and `nes-bench/docs/stream-plan.md` describe one wire
  (section 1.1); stream-plan's "schedule stream" is reading A.

## 1. Design principles

1. **Send the cause, not the effect, wherever the receiver can compute
   the effect exactly.** The signal path does this once: dots go on the
   wire, not the waveform (`ntsc-wire`, held to the sample). The console
   does it wholesale: given a cartridge, a state and the pad bytes, every
   frame and every sound sample is determined. So the smallest stream is
   pad plus a state key, and a viewer running `nes_wasm` computes the
   rest at 91 fps. Dots and APU writes are the same principle applied
   one stage later, for a viewer that cannot or may not run the console.

2. **One clock.** Every packet names a frame number, `seq`, the video
   channel's own `NDOT` sequence whether or not video is sent. No packet
   carries wall time; the recorder stamps it on disk, the dashboard
   aligns by `seq`.

3. **Every channel starts cold and recovers from a gap the same way**:
   a KEY form that stands alone, a dependent form, and on any gap the
   channel's state is discarded until the next KEY. The key cadence is
   the longest wait for full context, and it is a number.

4. **Frames are the unit of dropping.** A relay shedding load drops
   whole frames, every channel together. A frame with video and no pad
   packet is a bug and the tests say so.

5. **Live and recorded are the same bytes.** A recording is envelopes
   appended to a file; playback is the same receiver; scrubbing is
   "restore the last key before N, run forward".

6. **The wire carries board state and does not understand it.** The
   state channel is `Console::save_state`'s postcard bytes, opaque here,
   decoded by the console's own types on the dashboard.

7. Every claim has a test named beside it, `MUTATE=1` has something to
   break in every milestone, and no em dashes in prose.

### 1.1 Two readings of one wire

**Reading A, the viewer runs the console.** Channels: pad, state,
marker, control. The browser loads the cartridge (section 1.2), takes
the first state key, runs `nes_wasm` from it, applies each frame's pad
byte as it arrives, and draws and sounds locally. Bandwidth is pad (2
to 5 B a frame) plus a key every 120 frames (3 to 10 KB): **well under
1 KB/s per viewer** on the measured cartridges, a claim to confirm
end to end in S3. A viewer that falls behind or drifts (section 5)
resyncs at the next key. This is `nes-bench/docs/stream-plan.md`'s
schedule stream, and the primary path.

**Reading B, the viewer does not run the console.** Channels: video,
audio, pad, state, marker, control. The server encodes dots and logs
APU writes; the browser runs `ntsc-wasm` and an APU. About 10 KB/s per
viewer on the measured scene. For: a dashboard that only wants to show
the picture, a device too slow for the console, and internal cartridges
where serving dots is fine but a viewer would rather not load the
cartridge.

A server emits whatever channels its clients subscribed to (section
4.6); the same recorder writes all of them; the same file plays either
way.

### 1.2 The ROM rule

Dots are the picture, so for a commercial cartridge the site serves no
dots and no ROM: reading A only, and the viewer brings the dump. For
the family's own cartridges the site serves the cartridge and either
reading is fine. The server enforces this per cartridge: a video
channel subscription on a commercial cartridge is refused with a
control reply, not silently honoured. Test: subscribing to video on
the test cart succeeds; on a cartridge flagged commercial it is
refused.

## 2. Crate layout

```
crates/ntsc-wire       the video channel: DotFrame framing (PR #3)
crates/tm-stream       the envelope, the pad, audio, state, marker and
                       control codecs, the stream file, the receiver
crates/tm-relay        (S3) fan-out: routes by channel byte, never
                       inflates, drops by frame, honours subscriptions
```

`tm-stream` depends on `ntsc-wire` only to check a video payload's own
`seq` against its envelope. It does not depend on the console; the
console's server loop and `nes_wasm` depend on it.

## 3. The envelope

Eight bytes, integers little-endian, in front of every packet:

```
0  u8   channel   0 video  1 audio  2 pad  3 state  4 marker  5 control
1  u8   flags     bit 0 KEY; other bits per channel
2  u16  len       payload length of this packet
4  u32  seq       the frame this packet belongs to
8  payload
```

A state snapshot spans several packets (section 4.4); every other
channel's packet fits `u16`. Claim: no video key exceeds 64 KB on any
frame a real cartridge produces (`ntsc-wire` measured 4,152 B on a
tiled scene; a worst-case frame is the number to find in S0, and if it
exceeds 64 KB video gains the chunk prefix state has).

Within one frame the sender emits in order: **pad, audio, video, state
(if due), marker (if any)**. Channels a client did not subscribe to are
simply absent; the order of those present is a test.

## 4. Channels

### 4.1 Video (channel 0), reading B

The payload is an `NDOT` packet as `ntsc-wire` produces it, 20-byte
header included. Envelope KEY mirrors NDOT KEY, envelope `seq` equals
NDOT `seq`, and the receiver refuses a packet where they differ.

Measured 2026-10-20 at PR #3's commit: 68,486 picture dots raw; key of
a tiled scene 4,152 B; one sprite walking, delta at most 169 B; still
scene at most 103 B. A key every 60 frames: about 10 KB/s per viewer
worst case on that scene. `MUTATE=1`: 7 of 11 red.

Loose end, unchanged: `ntsc-wasm`'s `Pipeline` needs a `set_origin` so
a cold-starting viewer can take the phase the NDOT header carries.

### 4.2 Audio (channel 1), reading B

The APU register writes the CPU made during frame `seq`, in order:

```
u16  cycle   CPU cycle within the frame, 0..29781
u8   reg     offset in the $4000 window: $00..$13, $15, $17
u8   value
```

Confirmed: the CPU runs at 39,375,000/22 Hz and a frame is 29,780.67
cycles (full) or 29,780.33 (odd rendering frame), so `u16` holds the
cycle. $4014 and $4016 are not APU writes and are not logged. An empty
payload is a frame with no writes, still sent, so silence and a missing
packet differ.

**KEY form**: the console's APU board state (postcard, the same bytes
the state channel would carry for that board alone) followed by the
frame's log. Not the register values: length counters, envelopes, sweep
and the frame sequencer are not recoverable from them. Claim for S1's
test, expected to hold: a register-only key starts the synthesiser in
the right configuration and the wrong moment, and the test that shows
it is kept as the reason.

**The claim this channel stands on**: a client APU restored from a KEY
and fed the log reproduces the server's 48 kHz output sample for
sample. The APU on both ends is the console's own (v2a03-micro and the
NES-001 sound stage), which `nes_wasm` already ships; `Nes_Snd_Emu` is
a third oracle in the tests, pinned by hash, never shipped. Size claim:
10 to 40 writes a frame of game music, 40 to 160 B, plus a key's APU
state (a few hundred bytes deflated; measure).

In reading A this channel is not sent: the viewer's console has the
same APU and the same writes come from running the same instructions.

### 4.3 Pad (channel 2), both readings

`u8 count`, then one byte per controller, bit 0 A, 1 B, 2 Select, 3
Start, 4 Up, 5 Down, 6 Left, 7 Right, confirmed against
`nes_wasm::set_pad`. It is the input the server's driver held for frame
`seq`, the cause, applied by a reading-A viewer with `set_pad` before it
runs that frame. Sent every frame, identical or not, KEY always set.
Other controller layouts are a `count` this version does not define;
the length is in the envelope, so an unknown layout is skipped.

### 4.4 State (channel 3), both readings

`Console::save_state`'s postcard bytes at the end of frame `seq`,
deflated, chunked:

```
u8   format   0 postcard, deflated
u8   part     this chunk's index
u8   parts    chunks in this snapshot
u8   pad      0
u32  total    deflated snapshot length
```

Reassembled by the receiver, refused whole if a part is missing. KEY
always set. Cadence 120 frames by default, plus one on `reset` and
`cart` markers.

Measured by the console session at nes @ 9ebbe30, frames and sound
drained, taken at a CPU cycle end, gzip level 6:

| cartridge | raw | deflated |
|---|---|---|
| test cart, no sound | 326,720 B | 3,194 B |
| test cart, with sound | 337,672 B | 3,287 B |
| Super Mario Bros., with sound | 350,813 B | 9,477 B |

Under 5 KB/s at the default cadence. No delta form: the xor against the
snapshot 120 frames earlier deflated larger than the full (3,754 vs
3,194 B; 11,993 vs 9,477 B), so v0.1's delta design is dropped. The raw
bytes are mostly zeros, which is why deflate does the work.

Two measured traps, each an S2 test:

- **Drain before you key.** `save_state` serialises every undrained
  `DotFrame` in `frames` (178,691 B each) and the sound `out` buffer
  (3.2 KB per undrained frame); the first measurement grew 21 MB per 120
  frames. The server drains both before a key. Test: a key taken after
  120 frames without draining is refused by a size guard, and the
  guard's threshold is twice the largest measured snapshot. Open
  question 1 asks whether the console should exclude them instead.
- **A key lands at a cycle end.** `run_to_cycle_end` runs up to 24
  master half-steps past the frame boundary. A key's `seq` means "the
  end of frame N, plus up to one CPU cycle", and a reading-A viewer
  restores, then runs to the next frame boundary, then applies frame
  N+1's pad. Test: restore from a key, run the recorded pads forward
  120 frames, take a key, compare to the server's key at that `seq`,
  byte for byte. That is also the determinism test the whole of
  reading A rests on.

### 4.5 Marker (channel 4), both readings

UTF-8 text. `reset`, `cart <name>`, `player 1 joined`, `recording
started`, operator notes. KEY always set. `reset` and `cart ` are
reserved: the server emits them itself and takes a state key on the
same `seq`.

### 4.6 Control (channel 5), client to server

`u8 kind`, then:

- `0 subscribe`: a channel bitmask. Reading A subscribes to pad, state,
  marker; a picture-only dashboard to video, pad, marker; the server
  replies with the mask it granted (section 1.2 can refuse video).
- `1 need_key`: a channel bitmask; the server keys those channels at
  its next frame, rate-limited per client.
- `2 pad`: a player's input, deferred to after S3 (section 8).

## 5. Recovery, and reading A's drift

Per channel, per receiver: expected `seq` tracked; the expected packet
is applied; a later `seq` is a gap, the channel's state is discarded,
and the packet is applied only if KEY; an earlier `seq` is refused (the
transport is ordered; unordered is out of scope). Missed frames are
reported as a count on the first packet after a gap.

Reading A adds a failure video does not have: **drift**. A viewer that
missed a pad packet, or whose console differs from the server's by one
instruction anywhere, diverges silently and stays diverged. Three
answers, all in S3:

- A gap on the pad channel stops the viewer's console until the next
  state key (principle 3; running on with a guessed pad is exactly the
  silent divergence to avoid).
- Every state key is also a check: the viewer, on receiving a key for
  `seq` N while its own console is at N, compares a hash of its own
  `save_state` to a hash the key carries in its first chunk (an 8-byte
  field added to the chunk prefix in S2). A mismatch is reported to the
  dashboard as a drift event and the viewer restores from the key.
  Claim: on the test cart and Super Mario Bros., zero drift events over
  ten minutes with no gaps. If that claim fails, the console is
  nondeterministic somewhere and that is a console bug to find, not a
  stream feature to add.
- The relay drops by frame, so a reading-A client under backpressure
  sees pad gaps and waits for a key rather than skipping input.

## 6. The stream file

```
"TMST"  magic
u8      version   1
u8      flags     bit 0: has index trailer
u16     reserved
u64     wall_ms   wall-clock time of seq 0, Unix milliseconds
...     envelopes, exactly as sent, in order
trailer (optional) u32 count, count x (u32 seq, u64 offset) of each
        state snapshot's first packet, then u64 trailer_offset
```

The recorder sits on the server before the relay and writes every
channel the server produces, whatever clients subscribed to. A file
truncated anywhere plays to its last whole packet and refuses the
partial one; that is a test.

Scrub to N: greatest snapshot `seq` S at or below N; restore; run pads
S+1..N in a reading-A console; or, for reading B, seek the channels to N
and decode from their next keys. Playback paces by `seq` at 60.09881 Hz
against the display's refresh as `ntsc-wasm` does.

## 7. Milestones

**S0: envelope, pad, marker, state, file.** `tm-stream` with the
envelope, the pad and marker channels, the chunked state channel
(opaque bytes here; its traps are S2's), the ordered-emission check,
per-channel recovery, the stream file reader and writer with the
truncation test, and video by delegation to `ntsc-wire`. Also the
worst-case video key size, found with a noise frame and recorded.
Sabotage: emission order swaps pad and video; the file reader accepts a
partial packet; a dropped chunk is accepted. All must go red.

**S1: reading A, end to end.** The console's server loop emits pad and
state keys; `nes_wasm` takes a key, applies pads, runs. The
determinism test (section 4.4, second trap) and the drift hash. Measured
and reported: bytes per second per viewer on the test cart and Super
Mario Bros.; drift events over ten minutes. Sabotage: the viewer applies
the pad one frame late; the determinism test must go red.

**S2: state traps and the audio log.** The drain guard and its test;
the cycle-end semantics written into the receiver. Then the audio
channel for reading B: the write-log codec, the APU-state key, the
sample-exact replay test with `Nes_Snd_Emu` as third oracle, and the
register-only-key test kept as the reason. Sabotage: `cycle` quantised
to the nearest 8; replay must go red.

**S3: relay, reading B client, dashboard.** `tm-relay` with
subscriptions, `need_key`, drop by frame, the ROM rule. The console page
grows a viewer: reading A by default, reading B when video is granted,
with `set_origin` added to `ntsc-wasm`. The dashboard draws pad and
markers on a timeline and shows state raw, decoded by the console's
types. Measured: fan-out at 10 and 100 viewers in each reading, and the
cost of a key burst when 100 viewers join at once (`need_key` rate
limit is the knob).

**S4, deferred: the player.** Control-channel pad, measured round trip
from control packet to the `seq` it landed on.

## 8. Open questions for the director

1. **Should `Console::save_state` exclude `frames` and `sound.out`?**
   The drain guard catches the trap; excluding them from the state
   removes it. The console session's call.
2. **The drift hash.** An 8-byte hash of the viewer's own `save_state`
   compared on every key costs a serialise per two seconds in the
   browser (6.7 to 10.5 ms native; measure in wasm). Acceptable, or
   compare every fourth key?
3. **Where the server loop lives.** The console's server side (emit
   pad and keys) belongs in nes-console or nes-bench, not here; which?

## 9. Not in this version

PCM audio; any re-encode for a non-tinymachines player (that is
`ffmpeg` from the CRT model's RGB, the YouTube path); encryption or
authentication; game-specific RAM maps (the state channel is the whole
board, and Mario's X is in the RAM the dashboard already holds);
unordered transports; the player path (S4).
