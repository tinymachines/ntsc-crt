//! The drift policy, measured: the counts are deterministic functions of
//! the two exact rates, so the expected numbers are derived here from
//! the same rationals and the counters must land on them.

use ntsc_wasm::Pacing;

#[test]
fn a_60hz_display_against_the_nes_rate_drops_the_beat_frequency() {
    // Source 60.09881 Hz, display exactly 60 Hz: the source gains
    // 0.09881 frames per second, each surfacing as one dropped frame.
    // Over 600 seconds that is 59.28..: the derivation below uses the
    // same rationals as the pacing itself, so the assertion is that the
    // counting logic agrees with the rates, not a retyped number.
    let mut p = Pacing::nes_rendering_enabled();
    let dt = 1_000_000_000u64 / 60;
    let ticks = 600 * 60;
    let mut advanced = 0u64;
    for _ in 0..ticks {
        advanced += p.tick(dt) as u64;
    }
    // Every source frame is either presented or dropped.
    let elapsed_ns = dt as u128 * ticks as u128;
    let due = (elapsed_ns * 2 * 472_500_000 / (11 * 1_000_000_000)) / (714_736 + 714_728);
    assert_eq!(advanced as u128, due);
    assert_eq!(p.stats.presented, ticks);
    assert!(p.stats.dropped >= 55 && p.stats.dropped <= 62, "dropped {}", p.stats.dropped);
    assert_eq!(p.stats.duplicated, 0, "a faster source never duplicates");
}

#[test]
fn a_120hz_display_duplicates_roughly_every_other_frame() {
    let mut p = Pacing::nes_rendering_enabled();
    let dt = 1_000_000_000u64 / 120;
    for _ in 0..1200 {
        p.tick(dt);
    }
    // 1200 callbacks in 10 s cover about 601 source frames: the other
    // ~599 presentations are duplicates.
    assert!(p.stats.duplicated >= 595 && p.stats.duplicated <= 601, "dup {}", p.stats.duplicated);
    assert_eq!(p.stats.dropped, 0);
}

#[test]
fn broadcast_against_a_2997_display_never_drifts() {
    // A display at exactly the broadcast frame rate: one frame per tick,
    // forever, no duplicates, no drops. The period is 33,366,666.66 ns,
    // not an integer, so the ticks alternate 667/667/666 to sum to
    // exactly three periods (100,100,000 ns); a first version truncated
    // one dt and manufactured a slow display.
    let mut p = Pacing::broadcast();
    for _ in 0..10_000 {
        for dt_ns in [33_366_667u64, 33_366_667, 33_366_666] {
            p.tick(dt_ns);
        }
    }
    assert_eq!(p.stats.duplicated + p.stats.dropped, 0);
}
