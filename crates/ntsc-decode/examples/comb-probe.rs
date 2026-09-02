use ntsc_grid::{FrameParity, Phase};
use ntsc_source_nes::{encode_frame, DotFrame, Levels};
fn main() {
    let dots = ntsc_testgen::stripes(FrameParity::Even, 0x16, 0x2a);
    let frame = encode_frame(&Levels::transcribed(), &dots, Phase::new(0));
    for line in 99..=102 {
        let s = &frame.lines[line].samples;
        print!("line {line} p{:2}: ", frame.phase_at(line, 0).get());
        for &v in s.iter().take(32).skip(8) { print!("{}", if v > 0.5 { 'H' } else { '.' }); }
        println!();
    }
    // mean of three lines, bandpass rms mid-row
    let l = |n: usize| &frame.lines[n].samples;
    let mean: Vec<f32> = (0..2728).map(|i| (l(99)[i] + l(100)[i] + l(101)[i]) / 3.0).collect();
    let two: Vec<f32> = (0..2728).map(|i| (l(99)[i] + l(100)[i]) / 2.0).collect();
    let taps = &ntsc_decode::tables::CHROMA_BANDPASS;
    let half = taps.len() / 2;
    let rms = |row: &[f32]| {
        let mut acc = 0.0f32; let mut n = 0;
        for i in 200..2000 {
            let c: f32 = taps.iter().enumerate().map(|(k, t)| t * row[i + k - half]).sum();
            acc += c * c; n += 1;
        }
        (acc / n as f32).sqrt()
    };
    println!("raw rms {:.4}  three-line mean rms {:.4}  two-line mean rms {:.4}", rms(l(100)), rms(&mean), rms(&two));
    // and a solid frame for comparison
    let solid = encode_frame(&Levels::transcribed(), &DotFrame::filled(FrameParity::Even, 0x16, 0), Phase::new(0));
    let l = |n: usize| &solid.lines[n].samples;
    let mean: Vec<f32> = (0..2728).map(|i| (l(99)[i] + l(100)[i] + l(101)[i]) / 3.0).collect();
    println!("solid raw rms {:.4}  solid three-line mean rms {:.4}", rms(l(100)), rms(&mean));
}
