use ntsc_decode::{tables, Decoder};
use ntsc_grid::{FrameParity, Phase};
use ntsc_source_nes::{burst_axis_offset, encode_frame, wave_high, levels, DotFrame, Levels};
fn main() {
    let frame = encode_frame(&Levels::transcribed(), &DotFrame::filled(FrameParity::Even, 0x18, 0), Phase::new(0));
    let line = &frame.lines[100];
    let d = Decoder::transcribed(burst_axis_offset(), levels::LOW[1], levels::HIGH[2]);
    let taps = &d.chroma_taps;
    let m = taps.len() / 2;
    println!("gain const = {}", tables::CHROMA_GAIN_AT_SUBCARRIER);
    for i in 1024..1036 {
        let c: f32 = taps.iter().enumerate().map(|(k, t)| t * line.samples[i + k - m]).sum();
        let p = frame.phase_at(100, i).get();
        let s = line.samples[i];
        let high = wave_high(8, p);
        println!("i {i} phase {p:2} sample {s:.3} (high {high}) chroma {c:+.4}");
    }
}
