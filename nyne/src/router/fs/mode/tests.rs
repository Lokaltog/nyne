use rstest::rstest;

use super::{FILE_DEFAULT, MODE_MASK, narrow};

/// `narrow` strips stdlib file-type bits while preserving permission bits,
/// setuid/setgid/sticky, and defaults to fallback on impossible overflow.
#[rstest]
#[case::strips_ifreg(0o100_644, 0o644)]
#[case::strips_ifdir(0o040_755, 0o755)]
#[case::preserves_setuid(0o104_755, 0o4755)]
#[case::preserves_setgid(0o102_755, 0o2755)]
#[case::preserves_sticky(0o101_755, 0o1755)]
#[case::max_mask_bits(0o7777, 0o7777)]
#[case::only_high_bits_dropped(0o170_000, 0)]
#[case::zero_passthrough(0, 0)]
fn narrow_cases(#[case] input: u32, #[case] expected: u16) {
    assert_eq!(narrow(input, FILE_DEFAULT), expected);
}

#[test]
fn mode_mask_fits_in_u16() {
    // Sanity: MODE_MASK fits in u16, so `narrow` never returns the fallback
    // for any real stdlib mode value — the `.unwrap_or(default)` branch is
    // unreachable in practice.
    assert!(u16::try_from(MODE_MASK).is_ok());
}
