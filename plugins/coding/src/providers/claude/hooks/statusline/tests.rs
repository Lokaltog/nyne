// #[test]
// fn scaled_position_zero_tokens() {
//     assert!((scaled_position(0, 1_000_000) - 0.0).abs() < f32::EPSILON);
// }
//
// #[test]
// fn scaled_position_full_window() {
//     assert!((scaled_position(1_000_000, 1_000_000) - 1.0).abs() < f32::EPSILON);
// }
//
// #[test]
// fn scaled_position_zero_max_returns_zero() {
//     assert!((scaled_position(500, 0) - 0.0).abs() < f32::EPSILON);
// }
//
// #[test]
// fn scaled_position_monotonic() {
//     let max = 1_000_000;
//     let values: Vec<f32> = (0..=10).map(|i| scaled_position(i * 100_000, max)).collect();
//     for pair in values.windows(2) {
//         assert!(pair[1] >= pair[0], "scale must be monotonically increasing");
//     }
// }
//
// #[test]
// fn scaled_position_expands_low_range() {
//     // x^0.3 should give the first 10% of tokens more than 10% of bar space.
//     let pos_at_10pct = scaled_position(100_000, 1_000_000);
//     assert!(
//         pos_at_10pct > 0.10,
//         "10% of tokens should map to >10% of bar, got {:.1}%",
//         pos_at_10pct * 100.0,
//     );
// }
//
// #[test]
// fn render_progress_bar_length_is_bar_width() {
//     // Strip ANSI codes and count visible characters.
//     let bar = render_progress_bar(150_000, 1_000_000);
//     let visible: usize = strip_ansi(&bar)
//         .chars()
//         .filter(|c| *c == '\u{2588}' || *c == '\u{2591}')
//         .count();
//     assert_eq!(visible, usize::from(BAR_WIDTH));
// }
//
// #[test]
// fn render_progress_bar_zero_usage_all_inactive() {
//     let bar = render_progress_bar(0, 1_000_000);
//     let stripped = strip_ansi(&bar);
//     assert!(
//         !stripped.contains('\u{2588}'),
//         "zero usage should have no filled blocks",
//     );
// }
//
// #[test]
// fn render_progress_bar_full_usage_all_filled() {
//     let bar = render_progress_bar(1_000_000, 1_000_000);
//     let stripped = strip_ansi(&bar);
//     assert!(!stripped.contains('\u{2591}'), "full usage should have no empty blocks",);
// }
//
// /// Strip ANSI escape sequences from a string.
// fn strip_ansi(s: &str) -> String {
//     let mut out = String::with_capacity(s.len());
//     let mut in_escape = false;
//     for c in s.chars() {
//         if in_escape {
//             if c.is_ascii_alphabetic() {
//                 in_escape = false;
//             }
//         } else if c == '\x1b' {
//             in_escape = true;
//         } else {
//             out.push(c);
//         }
//     }
//     out
// }
