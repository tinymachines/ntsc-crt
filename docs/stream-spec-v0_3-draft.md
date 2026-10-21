# tm-stream: handoff spec v0.3 draft

A server-run NES console broadcasting to browsers, or a browser running
the console from a stream of what the server did. Drafted 2026-10-20 by
the implementing agent for the director to ratify, from the console
session's two readings (`docs/stream-spec-v0_1-review.md`,
`docs/stream-spec-v0_2-review.md`). v0.1 and v0.2 are kept for the
record. This revision is intended to be the one that gets ratified; it
has no open questions of its own, only claims with tests named.

Status of the numbers: section 4.1's are measured in `ntsc-wire` at
2cbd0f0; sections 4.2, 4.4 and 5's are the console session's at nes @
9ebbe30; everything else is a **claim for a test to confirm**, marked.

## Changes from v0.2

- **The envelope's length is `u32` and the envelope is 12 bytes.**
  v0.2 claimed no video key exceeds 64 KB. Measured: a noise frame with
  per-dot emphasis keys at **84,196 B** (`ntsc-wire` at 2cbd0f0,
  `print_worst_case_key`). Rather than chunk two channels, the length
  grows. [section 3]
- **Reading A's bandwidth corrected**: 1.9 to 5 KB/s at a key every 120
  frames, not "well under 1 KB/s". The live wire now keys every 600
  frames plus `need_key`; the recorder keeps 120 for scrub resolution.
  [review 2, "One number to correct"]
- **Every key and every `cart` marker carries the ROM digest**, FNV-1a
  64 over the image bytes, the same one `nes_wasm` already prepends to
  a state and refuses on mismatch. A key carries no ROM bytes (checked:
  `CartState` leaves ROM out), so the ROM rule holds; it carries no
  cartridge identity either, so the wire adds it. [review 2, "A key
  carries no ROM bytes"]
- **The drift hash is over the state with the sound stage left out**,
  FNV-1a 64, in every key, compared on every key. The sound stage is
  f32 filters and a `sin`-built resampler and can differ in the last
  bit between native libm and wasm's; the machine is integer arithmetic
  and must not. Measured: serialise 0.53 ms plus hash 0.31 ms native.
  [review 2, "The sound stage", OQ2]
- **The determinism test is named as new work in kind**: no test today
  holds the wasm console to the native one. S1 runs the `nodejs`-target
  wasm build beside the native console on the same keys and pads and
  compares machine state bytes, sound excluded. [review 2, "No test
  today"]
- **Pad timing is `set_pad` then `run_frames(1)`** on both ends, the
  two calls `nes_wasm` already makes; the server loop is those two
  calls. A per-poll pad form for the part's own schedules is reserved
  as a flag bit, later. [review 2, "Pad timing"]
- **`save_state` will exclude `frames` and `sound.out`** (a nes change,
  made at S2 start on the owner's word); the size guard stays until it
  does and is then a redundant check that stays green. [review 2, OQ1]
- **The server loop lives in nes-shell**, where `examples/stream.rs` is
  already the loop with ffmpeg as the sink. nes-bench is a client.
  [review 2, OQ3]
- **The deflate is named: gzip**, the console session's measured
  format and one of the three the browser's `DecompressionStream`
  takes. [review 2, "Name the deflate"]

## 1. Design principles

1. **Send the cause, not the effect, wherever the receiver can compute
   the effect exactly.** The signal path does this once: dots, not the
   waveform (`ntsc-wire`, held to the sample). The console does it
   wholesale: a cartridge, a state and the pad bytes determine every
   frame and every sample. The smallest stream is pad plus a state key,
   and a viewer running `nes_wasm` computes the rest at 91 fps. Dots and
   APU writes are the same principle one stage later, for a viewer that
   cannot or may not run the console.

2. **One clock.** Every packet names a frame number, `seq`. No packet
   carries wall time; the recorder stamps it on disk.

3. **Every channel starts cold and recovers from a gap the same way**:
   a KEY form that stands alone, a dependent form, and on any gap the
   channel's state is discarded until the next KEY. The key cadence is
   the longest wait for full context, and it is a number.

4. **Frames are the unit of dropping.** A relay shedding load drops
   whole frames, every channel together.

5. **Live and recorded are the same bytes.** A recording is envelopes
   appended to a file; playback is the same receiver; scrubbing is
   restore the last key before N, run forward.

6. **The wire carries board state and does not understand it.** The
   state channel is `Console::save_state`'s postcard bytes, opaque
   here, decoded by the console's types on the dashboard.

7. **The machine is deterministic; the sound stage is not held to
   it.** CPU, PPU, board, cartridge and counters are integer arithmetic
   and must agree byte for byte between native and wasm. The sound
   stage is float and the viewer's sound is the viewer's own.

8. Every claim has a test named beside it, `MUTATE=1` has something to
   break in every milestone, and no em dashes in prose.

### 1.1 Two readings of one wire

**Reading A, the viewer runs the console** (primary). Channels: pad,
state, marker, control. The browser holds the cartridge (section 1.2),
takes the first key, checks its ROM digest, restores, runs `set_pad`
then `run_frames(1)` per pad packet, draws and sounds locally. On every
key it compares its drift hash (section 5). Bandwidth at the live
cadence (key every 600 frames): pad about 300 B/s plus a key every ten
seconds, **about 0.6 KB/s on the test cart and 1.2 KB/s on Super Mario
Bros.**, a claim to confirm end to end in S1 from the measured parts
(3,194 and 9,477 B keys). This is `nes-bench/docs/stream-plan.md`'s
schedule stream.

**Reading B, the viewer does not run the console.** Channels: video,
audio, pad, state, marker, control. The server encodes dots and logs
APU writes; the browser runs `ntsc-wasm` and the console's APU. About
10 KB/s per viewer on the measured scene, 84 KB on a worst-case key.
For a picture-only dashboard, a device too slow for the console, and
internal cartridges where a viewer would rather not load the ROM.

A server emits the channels its clients subscribed to; the recorder
writes all of them; the file plays either way.

### 1.2 The ROM rule

Dots are the picture, so for a commercial cartridge the site serves no
dots and no ROM: reading A only, and the viewer brings the dump. For
the family's own cartridges the site serves the cartridge and either
reading is fine. The server refuses a video subscription on a
commercial cartridge with a control reply. Test: video on the test cart
is granted; on a cartridge flagged commercial it is refused.

The ROM digest (section 4.4) lets a viewer refuse a key for a dump
that is not theirs before trying to restore it, and lets the recorder
name which image a file is for without holding a byte of it.

## 2. Crate layout

```
crates/ntsc-wire       the video channel: DotFrame framing (PR #3)
crates/tm-stream       envelope; pad, audio, state, marker, control
                       codecs; the stream file; the receiver
crates/tm-relay        (S3) fan-out: routes by channel, never inflates,
                       drops by frame, honours subscriptions
nes-shell              (nes repo) the server loop: set_pad,
                       run_frames(1), emit; examples/stream.rs is the
                       same loop with ffmpeg as the sink
```

`tm-stream` depends on `ntsc-wire` only to check a video payload's own
`seq` against its envelope. It does not depend on the console.

## 3. The envelope

Twelve bytes, integers little-endian, in front of every packet:

```
0  u8   channel   0 video  1 audio  2 pad  3 state  4 marker  5 control
1  u8   flags     bit 0 KEY; bit 1 per channel (section 4)
2  u16  reserved  0
4  u32  len       payload length
8  u32  seq       the frame this packet belongs to
12 payload
```

`len` is `u32` because a video key can be 84,196 B (measured, noise
frame, per-dot emphasis) and a state key is chunked only for the
relay's benefit, not the envelope's. Within one frame the sender emits
in order: **pad, audio, video, state (if due), marker (if any)**;
channels not subscribed are absent; the order of those present is a
test.

## 4. Channels

### 4.1 Video (channel 0), reading B

An `NDOT` packet as `ntsc-wire` produces it, 20-byte header included.
Envelope KEY mirrors NDOT KEY, envelope `seq` equals NDOT `seq`, and
the receiver refuses a packet where they differ.

Measured at 2cbd0f0: 68,486 picture dots; key of a tiled scene 4,152 B;
one sprite walking, delta at most 169 B; still scene at most 103 B;
worst case (noise, per-dot emphasis) 84,196 B. A key every 60 frames:
about 10 KB/s per viewer on the tiled scene. `MUTATE=1`: 7 of 11 red.

Loose end, unchanged: `ntsc-wasm`'s `Pipeline` needs `set_origin`.

### 4.2 Audio (channel 1), reading B

The APU register writes the CPU made during frame `seq`, in order:

```
u16  cycle   CPU cycle within the frame, 0..29781
u8   reg     offset in the $4000 window: $00..$13, $15, $17
u8   value
```

Confirmed: the CPU runs at 39,375,000/22 Hz; a frame is 29,780.67
cycles (full) or 29,780.33 (odd rendering frame); `u16` holds it.
$4014 and $4016 are not logged. An empty payload is a silent frame,
still sent.

**KEY form**: the APU board's postcard state followed by the frame's
log. Not the register values: length counters, envelopes, sweep and the
frame sequencer are not recoverable from them; S2 keeps the test that
shows a register-only key starts in the right configuration and the
wrong moment.

**The claim this channel stands on**: a client APU restored from a KEY
and fed the log reproduces the server's writes exactly and the server's
48 kHz output to within the sound stage's float tolerance (principle
7), measured and recorded in S2. The APU on both ends is the console's
own (v2a03-micro, the NES-001 sound stage), held to blargg's APU ROMs
8 of 8; `Nes_Snd_Emu` is a third oracle in the tests, pinned by hash,
never shipped. Size claim: 10 to 40 writes a frame of game music.

Not sent in reading A: the viewer's console makes the same writes by
running the same instructions.

### 4.3 Pad (channel 2), both readings

`u8 count`, then one byte per controller, bit 0 A, 1 B, 2 Select, 3
Start, 4 Up, 5 Down, 6 Left, 7 Right, confirmed against
`nes_wasm::set_pad`. It is the input the server applied with `set_pad`
before `run_frames(1)` for frame `seq`, and the viewer applies it the
same way; the two calls are the whole timing contract. Sent every
frame, KEY always set.

Flag bit 1, **POLL**, reserved and refused by v0.3: a per-poll form
(`AT n hh`, the pad at the game's nth strobe) for streaming the part's
own schedules from nes-bench. Defined when nes-bench needs it; a v0.3
receiver that sees it refuses the packet with `WireError::Reserved`.

### 4.4 State (channel 3), both readings

`Console::save_state`'s postcard bytes at a CPU cycle end after frame
`seq`, gzip level 6, chunked:

```
u8   format   0 postcard, gzip
u8   part     this chunk's index
u8   parts    chunks in this snapshot
u8   pad      0
u32  total    deflated snapshot length
u64  rom      FNV-1a 64 over the cartridge image bytes
u64  drift    FNV-1a 64 over the raw postcard state with sound: None
```

The prefix is on every chunk, identical, so a relay or a scanning file
reader can route or index on any part. Reassembled by the receiver,
refused whole if a part is missing, refused before reassembly if `rom`
is not the viewer's. KEY always set. Cadence: **600 frames live, 120
in the recorder**, plus one on `reset` and `cart` markers, plus
`need_key`.

Measured at nes @ 9ebbe30, frames and sound drained, at a cycle end,
gzip 6:

| cartridge | raw | gzip | save_state | FNV-1a 64 |
|---|---|---|---|---|
| test cart, no sound | 326,720 B | 3,194 B | | |
| test cart, with sound | 337,672 B | 3,287 B | 0.527 ms | 0.308 ms |
| Super Mario Bros., with sound | 350,813 B | 9,477 B | 0.545 ms | 0.314 ms |

No delta form: an xor against 120 frames earlier deflates larger than
the full. Two measured traps, each a test:

- **Drain before you key.** Until the nes change lands (S2), the server
  drains `frames` and `sound.out` before a key, and a size guard
  refuses a key larger than twice the largest measured snapshot (the
  undrained trap grew 21 MB per 120 frames). After the change the guard
  stays as a check that stays green.
- **A key lands at a cycle end**, up to 24 master half-steps past the
  frame boundary. A key's `seq` means the end of frame N plus up to a
  CPU cycle; a viewer restores, runs to the next frame boundary, then
  applies N+1's pad with `set_pad` and `run_frames(1)`.

### 4.5 Marker (channel 4), both readings

UTF-8 text, KEY always set. `reset` and `cart <name>` are reserved: the
server emits them and takes a key on the same `seq`; `cart` is followed
by a space, the name, a tab, and the ROM digest as 16 hex digits, so a
viewer can check its dump against the marker before the key arrives.

### 4.6 Control (channel 5), client to server

`u8 kind`, then:

- `0 subscribe`: a channel bitmask; the server replies with the mask
  granted (section 1.2 can refuse video).
- `1 need_key`: a channel bitmask; keyed at the next frame, rate-limited
  per client (one per 120 frames; a claim about the right knob, set in
  S3 from the join-burst measurement).
- `2 pad`: a player's input, S4.

## 5. Recovery, drift, and the determinism test

Per channel, per receiver: expected `seq` tracked; the expected packet
is applied; a later `seq` is a gap, the channel's state is discarded,
and the packet is applied only if KEY; an earlier `seq` is refused (the
transport is ordered); missed frames are reported as a count on the
first packet after a gap.

**Drift**, reading A's own failure: a viewer that missed a pad, or whose
console differs by one instruction, diverges silently and stays
diverged. The answers:

- A gap on the pad channel stops the viewer's console until the next
  key. Running on with a guessed pad is the divergence to avoid.
- On every key for `seq` N, a viewer whose console is at N computes its
  own drift hash (serialise with `sound: None`, FNV-1a 64; under a
  millisecond native, measured; wasm to be measured in S1) and compares
  it to the key's. A mismatch is a drift event on the dashboard and a
  restore from the key.
- The relay drops by frame, so a client under backpressure sees pad
  gaps and waits for a key rather than skipping input.

**The determinism test** is the claim all of reading A rests on and it
does not exist today: nothing holds the wasm console to the native one
(`tests/record.rs` is native to native; the play page compares
pictures). S1 builds it: the `nodejs`-target wasm build and the native
console take the same key and the same 600 pads, and their machine
states, sound excluded, compare byte for byte at every key boundary.
Claim: zero mismatches over ten minutes on the test cart and Super
Mario Bros. If it fails, that is a console bug to find, not a stream
feature to add, and the spec waits on it.

## 6. The stream file

```
"TMST"  magic
u8      version   1
u8      flags     bit 0: has index trailer
u16     reserved
u64     wall_ms   wall-clock time of seq 0, Unix milliseconds
u64     rom       FNV-1a 64 of the cartridge image (0 if unknown)
...     envelopes, exactly as sent, in order
trailer (optional) u32 count, count x (u32 seq, u64 offset) of each
        state snapshot's first packet, then u64 trailer_offset
```

The recorder sits in nes-shell before the relay and writes every
channel the server produces at the recorder's cadence (120). A file
truncated anywhere plays to its last whole packet and refuses the
partial one; that is a test.

Scrub to N: greatest snapshot `seq` S at or below N; restore; run pads
S+1..N in a reading-A console; or, for reading B, seek to N and decode
from the next keys. Playback paces by `seq` at 60.09881 Hz against the
display's refresh as `ntsc-wasm` does.

## 7. Milestones

**S0: envelope, pad, marker, state, file** (`tm-stream`). The 12-byte
envelope, the pad and marker channels, the chunked state channel with
its prefix (opaque bytes here), the ordered-emission check,
per-channel recovery, the stream file with the truncation test, video
by delegation to `ntsc-wire`, the POLL bit refused. Sabotage: emission
order swaps pad and video; the file reader accepts a partial packet; a
dropped chunk is accepted; a key with a foreign ROM digest is accepted.
All must go red.

**S1: reading A, end to end, and the determinism test.** nes-shell's
loop emits pad and keys; `nes_wasm` takes a key, checks the digest,
restores, runs. The determinism test of section 5, built new. Measured
and reported: bytes per second per viewer on both cartridges at the
live cadence; the drift hash's cost in wasm; drift events over ten
minutes. Sabotage: the viewer applies the pad one frame late; the
determinism test must go red.

**S2: the nes change, the state traps, the audio log.** `save_state`
excludes `frames` and `sound.out` (on the owner's word; proven by the
console's state test and the size guard going redundant). The cycle-end
semantics in the receiver. Then the audio channel for reading B: the
write-log codec, the APU-state key, the replay test with `Nes_Snd_Emu`
as third oracle, the register-only-key test kept as the reason, and the
sound stage's native-to-wasm tolerance measured and recorded. Sabotage:
`cycle` quantised to the nearest 8; replay must go red.

**S3: relay, reading B client, dashboard.** `tm-relay` with
subscriptions, `need_key` and its rate limit, drop by frame, the ROM
rule. The console page grows a viewer: reading A by default, B when
granted, with `set_origin` added to `ntsc-wasm`. The dashboard draws
pad and markers on a timeline and shows state raw, decoded by the
console's types. Measured: fan-out at 10 and 100 viewers in each
reading; the cost of 100 viewers joining at once; the `need_key` knob
set from that.

**S4, deferred: the player.** Control-channel pad, round trip measured
from control packet to the `seq` it landed on.

## 8. Decisions recorded, for the director to ratify

1. Envelope length `u32`, envelope 12 bytes.
2. Live key cadence 600 frames, recorder 120.
3. ROM digest FNV-1a 64 in every key chunk, every `cart` marker, and
   the file header.
4. Drift hash FNV-1a 64 over the raw postcard state with `sound:
   None`, in every key chunk, compared on every key.
5. Sound is not held byte-equal across builds; the machine is.
6. Pad timing is `set_pad` then `run_frames(1)`, both ends.
7. `save_state` excludes delivered output; the nes change is S2's.
8. The server loop and recorder live in nes-shell.
9. gzip level 6 for state; the format byte says so.
10. The determinism test is built before anything else in S1 depends
    on it.

## 9. Not in this version

PCM audio; any re-encode for a non-tinymachines player (ffmpeg from the
CRT model's RGB in `nes-shell/examples/stream.rs`, the YouTube path);
encryption or authentication; game-specific RAM maps; unordered
transports; the per-poll pad form; the player path (S4).
