use std::time::{Duration, SystemTime};

use rstest::rstest;

use super::payload::{RateLimits, RateWindow};
use super::render::compute_pacing;

/// Build a `SystemTime` from a unix epoch (seconds).
fn epoch(secs: u64) -> SystemTime { SystemTime::UNIX_EPOCH + Duration::from_secs(secs) }

/// Build `RateLimits` with the given 7-day window fields.
fn limits(used_percentage: Option<f64>, resets_at: Option<u64>) -> RateLimits {
    RateLimits {
        seven_day: Some(RateWindow {
            used_percentage,
            resets_at,
        }),
    }
}

const SECS_PER_DAY: f64 = 24.0 * 3600.0;
const NOW_EPOCH: f64 = 1_000_000.0;

/// Compute `resets_at` given how many days have elapsed in the 7-day window.
fn resets_at_after(days_elapsed: f64) -> u64 { (NOW_EPOCH + (7.0 * SECS_PER_DAY - days_elapsed * SECS_PER_DAY)) as u64 }

#[rstest]
#[case::day1_of7_used_20pct(
    // Day 1: expected ~14.3%, used 20% → behind by ~5.7%
    1.0, 20.0, -5.7
)]
#[case::day1_of7_used_10pct(
    // Day 1: expected ~14.3%, used 10% → ahead by ~4.3%
    1.0, 10.0, 4.3
)]
#[case::midweek_on_pace(
    // Day 3.5: expected 50%, used 50% → on pace (delta ≈ 0)
    3.5, 50.0, 0.0
)]
#[case::day6_heavy_usage(
    // Day 6: expected ~85.7%, used 95% → behind by ~9.3%
    6.0, 95.0, -9.3
)]
#[case::day7_nearly_done(
    // Near end of window: expected ~99.9%, used 80% → ahead by ~20%
    6.99, 80.0, 19.9
)]
#[case::just_started(
    // 1 hour in: expected ~0.6%, used 0% → ahead by ~0.6%
    1.0 / 24.0, 0.0, 0.6
)]
fn pacing_delta(#[case] days_elapsed: f64, #[case] used: f64, #[case] expected_delta: f64) {
    let pacing = compute_pacing(
        &limits(Some(used), Some(resets_at_after(days_elapsed))),
        epoch(NOW_EPOCH as u64),
    )
    .expect("should compute pacing");

    assert!(
        (pacing.used - used).abs() < f64::EPSILON,
        "used should pass through: got {}, expected {used}",
        pacing.used,
    );
    assert!(
        (pacing.delta - expected_delta).abs() < 0.15,
        "delta: got {:.2}, expected {expected_delta:.1}",
        pacing.delta,
    );
}

#[test]
fn returns_none_when_seven_day_absent() {
    assert!(compute_pacing(&RateLimits { seven_day: None }, SystemTime::now()).is_none());
}

#[test]
fn returns_none_when_used_percentage_absent() {
    assert!(compute_pacing(&limits(None, Some(1_700_000_000)), epoch(1_699_000_000)).is_none());
}

#[test]
fn returns_none_when_resets_at_absent() {
    assert!(compute_pacing(&limits(Some(50.0), None), epoch(1_699_000_000)).is_none());
}

#[test]
fn returns_none_when_window_expired() {
    assert!(compute_pacing(&limits(Some(50.0), Some(1_000_000)), epoch(2_000_000)).is_none());
}

#[test]
fn returns_none_when_window_not_started() {
    // remaining > 7 days means window hasn't started yet
    let now = 1_000_000u64;
    assert!(compute_pacing(&limits(Some(0.0), Some(now + 8 * 24 * 3600)), epoch(now)).is_none());
}
