//! ANSI statusline rendering.
//!
//! Each segment is a standalone function `fn(&Context) -> Option<String>`.
//! The layout is a list of lines, each line a list of segment functions.
//! `None` segments are skipped; present segments are joined with [`SEP`].
//! Lines are joined with `\n`.
//!
//! ```text
//! Line 1:  /home/user/projects/nyne · ◆ Opus 4.6 (1M context) · [N]
//! Line 2:  ████████████░░░░░░░░░░░░░░░░░░░ 38% · 76k/200k · +156 −23
//! ```

use std::fmt::Write;
use std::sync::Arc;
use std::time::SystemTime;

use nyne::dispatch::activation::ActivationContext;
use owo_colors::OwoColorize;
use palette::{Clamp, IntoColor, Mix, Oklch, Srgb};

use super::payload::{CurrentUsage, RateLimits, StatuslinePayload, VimMode};

/// Width of the progress bar in characters.
///
/// Stored as `u8` so that `f32::from(BAR_WIDTH)` is lossless — no `as` casts needed.
pub(super) const BAR_WIDTH: u8 = 30;

/// Separator between segments on a single line.
const SEP: &str = " \u{00b7} ";

/// Segment boundaries in tokens. The bar is divided at these thresholds;
/// each segment gets its own color gradient. The final implicit segment runs
/// from the last boundary to the full context window size.
const SEGMENT_BOUNDARIES: &[u64] = &[100_000, 300_000];

/// Oklch gradient stops per segment (index 0 = first segment, etc.).
/// Each inner slice is `(local_t, [L, C, h])` where `local_t` is 0.0..=1.0
/// within that segment.
const SEGMENT_GRADIENTS: &[&[(f32, [f32; 3])]] = &[
    // Segment 0 (0–100k): blue → green — "you're fine"
    &[
        (0.0, [0.55, 0.15, 180.0]), // blue
        (1.0, [0.65, 0.18, 145.0]), // green
    ],
    // Segment 1 (100k–300k): yellow → red — "context growing"
    &[
        (0.0, [0.70, 0.18, 95.0]), // yellow
        (1.0, [0.55, 0.22, 29.0]), // red
    ],
    // Segment 2 (300k–full): red → dark red — "context rot"
    &[
        (0.0, [0.45, 0.22, 29.0]), // red
        (1.0, [0.25, 0.20, 20.0]), // dark red
    ],
];

/// Inactive fill: same hue as segment gradient start, but 30% chroma and 30% lightness.
const INACTIVE_CHROMA_SCALE: f32 = 0.30;
/// Lightness for inactive bar cells.
const INACTIVE_LIGHTNESS: f32 = 0.30;

/// Exponent for the nonlinear scale. x⁰·³ compresses high values, expanding
/// low-usage precision. `sqrt` (0.5) is too mild; `log` crushes the top segments.
const SCALE_EXPONENT: f32 = 0.4;

/// Everything a segment function needs to produce its output.
pub(super) struct Context<'a> {
    pub payload: &'a StatuslinePayload,
    pub activation: &'a ActivationContext,
}

/// A segment function: given context, optionally produces a rendered string.
type SegmentFn = fn(&Context<'_>) -> Option<String>;

/// The statusline layout: lines of segments.
///
/// This is the single place that defines which segments appear and where.
/// Each inner slice is one line; segments within a line are joined with [`SEP`].
fn layout() -> &'static [&'static [SegmentFn]] {
    &[&[project, git_branch, model], &[
        context_window,
        code_churn,
        rate_limit_pacing,
        vim_mode,
    ]]
}

/// Render the full statusline from context.
pub(super) fn render(ctx: &Context<'_>) -> String {
    layout()
        .iter()
        .map(|line| {
            line.iter()
                .filter_map(|segment| segment(ctx))
                .collect::<Vec<_>>()
                .join(SEP)
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Project root path, dimmed.
#[expect(clippy::unnecessary_wraps, reason = "signature constrained by SegmentFn type alias")]
fn project(ctx: &Context<'_>) -> Option<String> {
    Some(ctx.activation.host_root().display().to_string().dimmed().to_string())
}

/// Git branch name.
fn git_branch(ctx: &Context<'_>) -> Option<String> {
    ctx.activation
        .get::<Arc<nyne_git::Repo>>()
        .map(|r| format!("\u{e0a0} {}", r.head_branch().bold()))
}

/// Model display name with diamond prefix.
fn model(ctx: &Context<'_>) -> Option<String> {
    ctx.payload
        .model
        .as_ref()
        .and_then(|m| m.display_name.as_deref())
        .map(|name| format!("\u{25c6} {}", name.bold()))
}

/// Vim mode badge (colored block).
fn vim_mode(ctx: &Context<'_>) -> Option<String> { ctx.payload.vim.and_then(|v| v.mode).map(render_vim_badge) }

/// Context window progress bar + token usage stats.
fn context_window(ctx: &Context<'_>) -> Option<String> {
    ctx.payload.context_window.as_ref().map(|cw| {
        let total = cw.context_window_size.unwrap_or(200_000);
        let used = cw.current_usage.as_ref().map_or(0, CurrentUsage::total);

        let bar = render_progress_bar(used, total);
        let stats = format!("{}k/{}k", used / 1000, total / 1000).dimmed().to_string();
        [bar, stats].join(SEP)
    })
}

/// Code churn: lines added/removed.
fn code_churn(ctx: &Context<'_>) -> Option<String> {
    ctx.payload.cost.as_ref().and_then(|cost| {
        let added = cost.total_lines_added.unwrap_or(0);
        let removed = cost.total_lines_removed.unwrap_or(0);
        (added > 0 || removed > 0)
            .then(|| format!("{} {}", format!("+{added}").green(), format!("\u{2212}{removed}").red()))
    })
}
/// Deadband (±percentage points) within which pacing is considered "on pace".
const PACING_DEADBAND: f64 = 1.0;

/// Number of seconds in a full 7-day window.
const SEVEN_DAYS_SECS: f64 = 7.0 * 24.0 * 3600.0;

/// Pacing result for the 7-day rate-limit window.
#[derive(Debug, Clone, Copy)]
pub(super) struct Pacing {
    /// Actual usage percentage (0–100).
    pub used: f64,
    /// Difference: expected − used. Positive = ahead (surplus), negative = behind (overspent).
    pub delta: f64,
}

/// Compute daily pacing from the 7-day rate-limit window.
///
/// Returns `None` if the required fields are absent or the window hasn't started yet.
#[expect(
    clippy::cast_precision_loss,
    reason = "unix epoch seconds fit comfortably in f64 mantissa until year 285,616,414"
)]
pub(super) fn compute_pacing(rate_limits: &RateLimits, now: SystemTime) -> Option<Pacing> {
    let window = rate_limits.seven_day.as_ref()?;
    let used = window.used_percentage?;
    let resets_at = window.resets_at?;

    let now_epoch = now.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs_f64();
    let remaining_secs = (resets_at as f64) - now_epoch;

    // Window hasn't started yet or already expired — can't compute pacing.
    if remaining_secs <= 0.0 || remaining_secs > SEVEN_DAYS_SECS {
        return None;
    }

    let elapsed_secs = SEVEN_DAYS_SECS - remaining_secs;
    let expected = (elapsed_secs / SEVEN_DAYS_SECS) * 100.0;
    let delta = expected - used;

    Some(Pacing { used, delta })
}

/// 7-day rate-limit pacing: usage + ahead/behind indicator.
fn rate_limit_pacing(ctx: &Context<'_>) -> Option<String> {
    let pacing = compute_pacing(ctx.payload.rate_limits.as_ref()?, SystemTime::now())?;

    let label = format!("7d: {:.0}%", pacing.used).dimmed().to_string();

    let indicator = if pacing.delta > PACING_DEADBAND {
        format!("+{:.1}%", pacing.delta).green().to_string()
    } else if pacing.delta < -PACING_DEADBAND {
        format!("{:.1}%", pacing.delta).red().to_string()
    } else {
        "on pace".dimmed().to_string()
    };

    Some(format!("{label} {indicator}"))
}

/// Render a colored vim mode badge.
fn render_vim_badge(mode: VimMode) -> String {
    match mode {
        VimMode::Normal => " N ".bold().black().on_green().to_string(),
        VimMode::Insert => " I ".bold().black().on_red().to_string(),
        VimMode::Visual => " V ".bold().black().on_magenta().to_string(),
        VimMode::Replace => " R ".bold().black().on_yellow().to_string(),
    }
}

/// Pre-computed Oklch gradient for interpolation over arbitrary stops.
///
/// Used to color progress bar cells. Each segment of the bar has its own
/// gradient (from green through yellow to red), and interpolation blends
/// smoothly between the defined Oklch color stops.
struct Gradient {
    stops: Vec<(f32, Oklch)>,
}

/// Methods for [`Gradient`].
impl Gradient {
    /// Build a gradient from raw Oklch stop definitions.
    fn from_stops(stops: &[(f32, [f32; 3])]) -> Self {
        Self {
            stops: stops.iter().map(|&(t, [l, c, h])| (t, Oklch::new(l, c, h))).collect(),
        }
    }

    /// Convert an Oklch color to an sRGB (u8) triplet.
    fn to_rgb(oklch: Oklch) -> (u8, u8, u8) {
        let srgb: Srgb<f32> = oklch.into_color();
        let c: Srgb<u8> = srgb.clamp().into_format();
        (c.red, c.green, c.blue)
    }

    /// Interpolate to an sRGB (u8) triplet at the given ratio (0.0..=1.0).
    fn rgb_at(&self, ratio: f32) -> (u8, u8, u8) { Self::to_rgb(self.interpolate(ratio.clamp(0.0, 1.0))) }

    /// Derive an inactive color: same hue as gradient start, reduced chroma and lightness.
    fn inactive_rgb(&self) -> (u8, u8, u8) {
        let start = self.stops.first().map_or_else(|| Oklch::new(0.3, 0.0, 0.0), |s| s.1);
        Self::to_rgb(Oklch::new(
            INACTIVE_LIGHTNESS,
            start.chroma * INACTIVE_CHROMA_SCALE,
            start.hue,
        ))
    }

    /// Blend in Oklch space between the bracketing stops.
    fn interpolate(&self, ratio: f32) -> Oklch {
        let Some(&(t_first, first)) = self.stops.first() else {
            return Oklch::new(0.5, 0.0, 0.0);
        };
        let Some(&(t_last, last)) = self.stops.last() else {
            return first;
        };

        if ratio <= t_first {
            return first;
        }
        if ratio >= t_last {
            return last;
        }

        self.stops
            .windows(2)
            .find_map(|pair| {
                let &[(t_lo, c_lo), (t_hi, c_hi)] = pair else {
                    return None;
                };
                if ratio < t_lo || ratio > t_hi {
                    return None;
                }
                let span = t_hi - t_lo;
                let local = if span > f32::EPSILON {
                    (ratio - t_lo) / span
                } else {
                    0.0
                };
                Some(c_lo.mix(c_hi, local))
            })
            .unwrap_or(last)
    }
}

/// Apply the nonlinear scale: `(x / max)^SCALE_EXPONENT`.
#[expect(
    clippy::cast_precision_loss,
    reason = "token counts lose sub-integer precision — irrelevant for a progress bar"
)]
/// Compute scaled position for progress bar rendering.
pub(super) fn scaled_position(tokens: u64, max: u64) -> f32 {
    if max == 0 {
        return 0.0;
    }
    (tokens as f32 / max as f32).powf(SCALE_EXPONENT)
}

/// Clamp a bar-space float to `[0, BAR_WIDTH]` and convert to `usize`.
///
/// Rust has no lossless `f32→integer` conversion; every `as` cast triggers
/// `cast_possible_truncation`.  This helper centralises the single unavoidable
/// cast behind an explicit `.clamp()` guard so callers stay lint-clean.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value is clamped to [0.0, BAR_WIDTH] which fits in u8"
)]
fn bar_fraction_to_cols(val: f32) -> usize {
    let clamped = val.clamp(0.0, f32::from(BAR_WIDTH));
    usize::from(clamped.round() as u8)
}

/// Compute the char-column boundaries and gradients for each segment.
///
/// Returns `(start_col, width_chars, gradient)` per segment. The segment
/// boundaries are mapped through the nonlinear scale, so early segments
/// occupy proportionally more bar width.
fn segment_layout(context_window: u64) -> Vec<(usize, usize, Gradient)> {
    let bar_width = usize::from(BAR_WIDTH);

    // Scaled positions of each boundary (0.0..1.0 in bar-space).
    let mut edges: Vec<f32> = Vec::with_capacity(SEGMENT_BOUNDARIES.len() + 2);
    edges.push(0.0);
    for &boundary in SEGMENT_BOUNDARIES {
        if boundary < context_window {
            edges.push(scaled_position(boundary, context_window));
        }
    }
    edges.push(1.0);

    // Convert scaled positions to character columns, distributing rounding
    // so total always equals BAR_WIDTH.
    let mut cols: Vec<usize> = edges
        .windows(2)
        .map(|w| {
            let &[lo, hi] = w else { unreachable!() };
            bar_fraction_to_cols((hi - lo) * f32::from(BAR_WIDTH))
        })
        .collect();

    // Fix rounding drift: adjust the widest segment.
    let total: usize = cols.iter().sum();
    if total != bar_width
        && let Some(widest) = cols.iter_mut().max()
    {
        if total > bar_width {
            *widest = widest.saturating_sub(total - bar_width);
        } else {
            *widest += bar_width - total;
        }
    }

    // Pair each column width with a gradient. If there are more segments than
    // gradient definitions, the last gradient repeats (via `.cycle()`).
    let mut col = 0;
    cols.iter()
        .zip(SEGMENT_GRADIENTS.iter().copied().cycle())
        .map(|(&width, stops)| {
            let start = col;
            col += width;
            (start, width, Gradient::from_stops(stops))
        })
        .collect()
}

/// Write a single bar cell — filled (gradient-colored) or inactive (tinted).
fn write_bar_cell(bar: &mut String, filled: bool, local_t: f32, gradient: &Gradient, inactive: (u8, u8, u8)) {
    if filled {
        let (r, g, b) = gradient.rgb_at(local_t);
        let _ = write!(bar, "{}", "\u{2501}".truecolor(r, g, b));
    } else {
        let (r, g, b) = inactive;
        let _ = write!(bar, "{}", "\u{2501}".truecolor(r, g, b));
    }
}

/// Render a progress bar showing token usage.
pub(super) fn render_progress_bar(used: u64, context_window: u64) -> String {
    let bar_width = usize::from(BAR_WIDTH);
    let segments = segment_layout(context_window);
    let fill_pos = scaled_position(used, context_window);
    let fill_col = bar_fraction_to_cols(fill_pos * f32::from(BAR_WIDTH));

    // Pre-compute per-segment inactive colors and pair with gradients.
    let segment_colors: Vec<_> = segments.iter().map(|(_, _, g)| (g, g.inactive_rgb())).collect();

    // Build the bar by iterating segments → cells, carrying the gradient ref.
    let mut bar = String::with_capacity(bar_width * 20); // ANSI codes are verbose
    for (&(start_col, width, _), (gradient, inactive)) in segments.iter().zip(&segment_colors) {
        for j in 0..width {
            let local_t = if width <= 1 {
                1.0
            } else {
                f32::from(u8::try_from(j).unwrap_or(0)) / f32::from(u8::try_from(width - 1).unwrap_or(1))
            };
            write_bar_cell(&mut bar, start_col + j < fill_col, local_t, gradient, *inactive);
        }
    }

    bar
}
