use ntsc_decode::Decoder;
use ntsc_grid::{FrameParity, Phase};
use ntsc_source_nes::{burst_axis_offset, encode_frame, wave_high, levels, DotFrame, Levels};
fn main() {
    let theta0 = burst_axis_offset();
    let d = Decoder::transcribed(theta0, levels::LOW[1], levels::HIGH[2]);
    println!("theta0 = {:.4} rad = {:.1} deg", theta0, theta0.to_degrees());
    for hue in 1..=12u8 {
        let frame = encode_frame(&Levels::transcribed(), &DotFrame::filled(FrameParity::Even, 0x10 | hue, 0), Phase::new(0));
        let yuv = d.decode_yuv(&frame, 100, 1, 2048);
        let (u, v) = (yuv.u[1024] as f64, yuv.v[1024] as f64);
        let (mut eu, mut ev) = (0.0f64, 0.0f64);
        for p in 0..12u8 {
            let s = if wave_high(hue, p) { 1.0 } else { -1.0 };
            let th = std::f64::consts::TAU * p as f64 / 12.0 + theta0;
            eu += s * th.sin(); ev += s * th.cos();
        }
        println!("hue {hue:2}: measured {:7.1}  predicted {:7.1}  sat {:.4}", v.atan2(u).to_degrees(), ev.atan2(eu).to_degrees(), (u*u+v*v).sqrt());
    }
}
