# tm-stream spec v0.1: a reading, 2026-10-20 evening

Read from this branch at b2d8c86 (PR #3), by the console session. Measurements below were
taken on the console at nes @ 9ebbe30 with a scratch example (not committed).

## The premise to correct before S1 and S2

Principle 1 assumes the console cannot run in a browser (switch-level 2C02,
decode at 41 fps). The console is not switch-level: nes-console builds on
v2c02-fast and v2a03-micro (tables), with the NES-001 sound stage (v2a03-dac).
nes_wasm already runs the whole console on tinymachines.ai/nes/play, with
sound() draining 48 kHz f32 and set_pad(); measured 91 fps in wasm on
2026-09-06. Native here: 6.7 ms a frame without sound, 10.5 ms with.

So the spec's own escape clause applies today: the video channel is optional,
and so is audio. For a viewer running nes_wasm, the pad channel plus a state key
is the whole stream, and video and audio are what the browser computes.

## Open question 1 (which APU): the console's own

It exists, it is faster than real time in wasm, and it is held to blargg's APU
ROMs (8 of 8) and the mixer ROMs (under 5%). Nes_Snd_Emu is still a fine third
oracle for a test. The write-log channel is only needed for a viewer that does
not run the console; keep it in the spec as that, not as the primary path.

## Open question 2 (snapshot size): measured, cheap, no delta needed

Console::save_state (postcard, not bincode or JSON; the format byte should say
so), frames and sound buffer drained, at a CPU cycle end:

| cartridge | raw | gzip -6 | xor delta vs 120 frames earlier, gzip -6 |
|---|---|---|---|
| testcart (family), no sound | 326,720 B | 3,194 B | 3,754 B |
| testcart, with sound | 337,672 B | 3,287 B | 3,868 B |
| Super Mario Bros., with sound | 350,813 B | 9,477 B | 11,993 B |

A deflated full snapshot every 120 frames is under 5 KB/s, half the video
channel's worst case. The delta is larger than the full after deflate, so drop
the delta design. The raw snapshot is mostly zeros (a sparse table somewhere),
which is why deflate does so well.

Two traps for S2:
- Console::save_state serialises `frames` (every DotFrame not yet drained,
  178,691 B each) and the sound `out` buffer (3.2 KB per undrained frame). The
  first measurement, taken without draining, grew 21 MB per 120 frames. The
  server must drain both before taking a key, or the console should exclude
  them from the state.
- A state can only be taken at a CPU cycle end (run_to_cycle_end, up to 24
  master half-steps past the frame boundary). The key's `seq` is "frame N plus
  up to a cycle", and the replay must restore and then run to the frame.

## Claims checked

- Cycles per frame: the console's CPU rate is 39,375,000/22 Hz and a frame is
  89,342 dots (89,341 on an odd rendering frame) / 3 = 29,780.67 or 29,780.33
  CPU cycles. u16 holds it. The spec's 29,780.5 is the average, fine for the
  field width.
- Pad byte order matches nes_wasm::set_pad (A bit 0 .. Right bit 7).

## Where this meets nes-bench/docs/stream-plan.md

stream-plan's "schedule stream" (pad bytes per poll + branch events over SSE,
viewer's browser runs the model on the viewer's own ROM) is tm-stream's pad +
marker + state channels with video and audio off. One wire, two readings. The
ROM rule stands: for a commercial cartridge the site serves no dots either
(dots are the picture), so the viewer brings the dump; for internal games the
site serves the cartridge and any channel is fine.

## Open question 3 (player path): later

The control channel is defined, so nothing is lost by deferring. S3 should
measure viewers first (bytes per viewer, fan-out at 10 and 100).

## How the snapshot was measured

A scratch example in nes-console (not committed), run three ways: the
family's test cartridge (public web/e2e/fixtures/testcart.nes) without
and with sound, and the owner's own Super Mario Bros. dump with sound.
Gzip at level 6 on the written bytes.

```rust
use nes_console::{ines, Alignment, Console};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&args[1]).expect("rom");
    let rom = ines::parse(&bytes).expect("iNES");
    let chr_ram = rom.chr_ram.then(|| vec![0u8; 0x2000]);
    let cart = rom.cart().expect("cart");
    let mut c = Console::with_prg_ram(cart, chr_ram, Alignment::default(), true);
    if std::env::var_os("SOUND").is_some() { c.sound = Some(nes_console::Sound::default()); }
    let mut prev: Option<Vec<u8>> = None;
    for step in 0..6 {
        c.run_frames(120);
        c.frames.clear();                                   // the trap: drain, or it is in the state
        if let Some(s) = c.sound.as_mut() { s.out.clear(); } // likewise
        c.run_to_cycle_end();                               // a state is only taken here
        let s = c.save_state().expect("state");
        std::fs::write(format!("/tmp/zz-state-{step}.bin"), &s).unwrap();
        if let Some(p) = &prev {
            let n = s.len().max(p.len());
            let x: Vec<u8> = (0..n).map(|i| s.get(i).copied().unwrap_or(0) ^ p.get(i).copied().unwrap_or(0)).collect();
            std::fs::write(format!("/tmp/zz-delta-{step}.bin"), &x).unwrap();
        }
        println!("frame {:4}: snapshot {} B", (step + 1) * 120, s.len());
        prev = Some(s);
    }
}
```
