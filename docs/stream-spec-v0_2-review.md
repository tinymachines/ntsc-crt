# tm-stream spec v0.2 draft: a reading, 2026-10-20 night

Read at 7edf923 by the console session. The adopted numbers are mine and
they are quoted right. What follows is one number to correct, three
things checked against the console, answers to the three open questions,
and two details for the format. New measurements are at nes @ 9ebbe30,
scratch examples, not committed.

## One number to correct: reading A is not under 1 KB/s

Section 1.1 claims "well under 1 KB/s per viewer". At the default key
cadence of 120 frames it is not: pad is about 300 B/s and the keys are
3.2 KB per two seconds on the test cart and 9.5 KB per two seconds on
Super Mario Bros., so 1.9 to 5 KB/s. Still cheap, and still below reading
B, but the claim should say so, or the live wire should key less often
than the recorder does. A viewer in sync needs a key only for the drift
check and for a late join; 600 frames plus `need_key` would put Mario at
about 1.2 KB/s. The recorder can keep 120 frames for scrub resolution,
since it writes to disk and not to a viewer.

## Checked against the console

**A key carries no ROM bytes.** nes-bus's `CartState` is "the board
itself with its ROM left out (the vectors that hold ROM are empty)";
the console adds WRAM, PRG RAM, CIRAM, CHR RAM, OAM and the palette,
all RAM. So the ROM rule of section 1.2 holds for the state channel as
written. What a key does not carry either is which cartridge it is
for: `Console::save_state` has no ROM digest, and the page's wrapper
(`nes_wasm`) prepends one, FNV-1a 64 over the image bytes, and refuses a
state for another image. The wire should do the same: the ROM digest in
every key's first chunk and in the `cart` marker, so a viewer refuses a
key for a dump that is not theirs before it tries to restore.

**The sound stage is where native and wasm can differ.** The machine
(CPU, PPU, board, cartridge, counters) is integer arithmetic. The sound
stage is not: `Sound::state` carries f32 filter states and a pending
sample queue, and the resampler's kernel is built with `sin` at
construction, which is libm on one side and wasm's compiled libm on the
other. The two can differ in the last bit of a sample and the states
diverge while the machines agree. Two consequences: the drift hash must
leave the sound stage out (hash the state with `sound: None`), and the
determinism test must not expect sound to be byte-equal across the two
builds. The viewer's sound is the viewer's own; it need not be.

**No test today holds the wasm console to the native one.** The
console's `tests/record.rs` replays a logged run native to native, and
the play page's first-frame check compares pictures (0 of 255). S1's
determinism test is therefore new work in kind, not just in degree: it
has to run the `nodejs`-target wasm build beside the native console on
the same keys and pads and compare the machine's state bytes. That is
the test the whole of reading A rests on, and it does not exist yet.

**Pad timing is one function on both ends.** For the viewer and the
server to agree, both must apply the pad at the same instant, and the
instant is `Console::run_frames`'s boundary: `set_pad` then
`run_frames(1)`, the two calls `nes_wasm` already makes. The server loop
should be those two calls and nothing cleverer. One thing this cannot
express: the bench's schedules are per poll (`AT n hh`, the pad at the
game's nth strobe), not per frame, which is how the part is driven and
what stream-plan's "bytes per poll" meant. A frame-keyed pad channel
streams the software console; streaming the part's own schedule would
need a poll-keyed form, a flag bit on the pad channel, later.

## The open questions

**1. Exclude `frames` and `sound.out` from `save_state`: yes.** Both
are output already delivered, not state the machine needs to run on;
the PPU keeps its own frame in progress (`out`, in the stepper's state)
and the sound stage keeps its filters and pending queue. Excluding them
makes the trap impossible rather than guarded. It is a nes change,
proven by the console's state test (restored anywhere, runs on as if it
had never stopped) and by the size guard going from needed to
redundant; I will make it when S2 starts, on the owner's word, since it
touches what the play page restores.

**2. The drift hash: every key, and it is cheap.** Measured native,
after 300 frames with sound:

| | test cart | Super Mario Bros. |
|---|---|---|
| `save_state`, best of 50 | 0.527 ms, 335,117 B | 0.545 ms, 346,053 B |
| FNV-1a 64 over it | 0.308 ms | 0.314 ms |

Under a millisecond together; even ten times slower in wasm it is under
a frame once every two seconds. Compare on every key. Define the hash as
FNV-1a 64 over the raw postcard bytes of the state with the sound stage
left out (the reason is above), and put it in the first chunk beside the
ROM digest.

**3. The server loop lives in nes-shell.** It is the one crate in nes
with registry dependencies, `examples/stream.rs` (the YouTube path) is
already its loop with ffmpeg as the sink, and the tm-stream server is
the same loop with a different sink: pad, keys and markers out instead
of pixels. nes-bench is the Pi and the part; it drives the console
through the head and would be a client of this, not its home.

## Two format details

- **Name the deflate.** "postcard, deflated" should say which: the
  browser's `DecompressionStream` takes `deflate` (zlib), `deflate-raw`
  or `gzip`, and my numbers are gzip level 6. Any of the three; pick one
  and write it next to the format byte.
- **The 64 KB claim for a video key** has a cheap worst case to try: a
  frame of the test cart with every dot a different colour from its
  neighbours is not a real scene but is the bound, and ntsc-wire's
  print_sizes test could take a noise frame in a minute.

## How the serialise was timed

```rust
let mut c = /* the console, sound on */;
c.run_frames(300);
c.frames.clear();
if let Some(s) = c.sound.as_mut() { s.out.clear(); }
c.run_to_cycle_end();
let mut best = f64::MAX;
for _ in 0..50 {
    let t = std::time::Instant::now();
    let s = c.save_state().unwrap();
    best = best.min(t.elapsed().as_secs_f64() * 1000.0);
}
```
