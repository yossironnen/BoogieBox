//! Robust rhythmic-grid recovery from neural beat-tracking output.
//!
//! madmom's beat list is not a clean metronome. Measured across the reference
//! playlist, real grids contain three defects that all break beat-matching:
//!
//! * **Half-beat slips.** Whole sections lock onto the offbeat. One track's
//!   phase trace reads `+0, -230, +13, +12, +12, -229, -229, -220, +23` ms
//!   against a 484 ms beat — the tracker jumps a half beat back and forth.
//! * **Dropped and doubled beats.** Measuring tempo over thirds of a track
//!   gives readings like 127 / 145 / 125 BPM for a track that is 125 BPM
//!   throughout.
//! * **Sub-frame tempo error.** A single global BPM taken over a whole track
//!   is typically 0.03-0.06% off, enough to walk the grid 160-220 ms away from
//!   the music by the end of the track.
//!
//! A blend only needs the grid to be right *where the two tracks overlap*, so
//! everything here fits a uniform grid over a short window centred on the mix
//! point and rejects beats that do not sit on it. A slip elsewhere in the track
//! then has no effect, and a slip inside the window is outvoted: the fit is
//! tried from several seeds and the interpretation the most detected beats
//! agree with wins.

/// A uniform beat grid fitted over one region of a track: beat `n` of the grid
/// falls at `anchor + n * period`, in the track's own (pre-tempo-adjust) seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalGrid {
    /// Seconds per beat.
    pub period: f64,
    /// Time of grid beat 0. Chosen near the fit's centre, not the track start.
    pub anchor: f64,
    /// RMS distance from the detected beats to the fitted grid, in seconds.
    pub residual_sec: f64,
    /// How many detected beats the fit is built from.
    pub support: usize,
}

/// Beats needed inside the window before a fit is trusted.
pub const MIN_GRID_SUPPORT: usize = 8;

/// How far off its grid slot a beat may sit and still count as an inlier, as a
/// fraction of a beat. Comfortably above real jitter (a few percent) and
/// comfortably below the 0.5 of a half-beat slip.
const INLIER_TOLERANCE: f64 = 0.22;

/// Largest RMS residual a fit may have, as a fraction of a beat. Above this the
/// window is too ragged to align against and callers should fall back.
const MAX_RESIDUAL_FRACTION: f64 = 0.05;

/// Beat periods outside this range are not plausible tempos (50-300 BPM).
const MIN_PERIOD_SEC: f64 = 0.2;
/// Upper bound of the plausible beat-period range.
const MAX_PERIOD_SEC: f64 = 1.2;

/// Refinement passes. The fit converges in two; four is cheap insurance.
const FIT_PASSES: usize = 4;

/// How many beats near the centre are tried as fit seeds. Enough to cross a
/// slipped run and reach the correct interpretation on the other side.
const FIT_SEEDS: usize = 12;

impl LocalGrid {
    /// Tempo the fit implies.
    pub fn bpm(&self) -> f64 {
        60.0 / self.period
    }

    /// The grid beat nearest `t`.
    pub fn nearest_beat(&self, t: f64) -> f64 {
        self.anchor + ((t - self.anchor) / self.period).round() * self.period
    }

    /// The first grid beat at or after `t`.
    pub fn beat_at_or_after(&self, t: f64) -> f64 {
        self.anchor + ((t - self.anchor) / self.period).ceil() * self.period
    }

    /// Signed distance from `t` to the nearest grid beat, in `[-period/2, period/2]`.
    /// Positive when `t` sits after the beat.
    pub fn offset_from_beat(&self, t: f64) -> f64 {
        t - self.nearest_beat(t)
    }

    /// Bar length, assuming 4/4 (every style this renderer mixes).
    pub fn bar_sec(&self) -> f64 {
        self.period * 4.0
    }
}

/// Fits a uniform grid to the beats within `half_window` seconds of `center`.
///
/// Returns `None` when the window holds too few beats, the spacing is not a
/// plausible tempo, or the surviving beats are too ragged to be a grid.
pub fn fit_local_grid(beats: &[f64], center: f64, half_window: f64) -> Option<LocalGrid> {
    let lo = center - half_window;
    let hi = center + half_window;
    let window: Vec<f64> = beats
        .iter()
        .copied()
        .filter(|b| b.is_finite() && *b >= lo && *b <= hi)
        .collect();
    if window.len() < MIN_GRID_SUPPORT {
        return None;
    }

    let mut spacings: Vec<f64> = window
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|d| d.is_finite() && *d > 0.0)
        .collect();
    if spacings.is_empty() {
        return None;
    }
    spacings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let seed_period = spacings[spacings.len() / 2];
    if !(MIN_PERIOD_SEC..=MAX_PERIOD_SEC).contains(&seed_period) {
        return None;
    }

    // Try the beats nearest the mix point as seeds and keep whichever fit the
    // most detected beats agree with.
    //
    // One seed is not enough. When the window straddles one of the tracker's
    // half-beat slips, a seed that happens to land inside the slipped run locks
    // the grid to the slip and throws the (larger) correct run away as outliers.
    // Measured on the reference playlist, that cost one track's fit two thirds
    // of its support (33 beats at a 19 ms residual, against 70 at 8 ms from the
    // majority interpretation) and made another window fail to fit at all.
    let mut order: Vec<f64> = window.clone();
    order.sort_by(|a, b| {
        (a - center)
            .abs()
            .partial_cmp(&(b - center).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut best: Option<LocalGrid> = None;
    for &seed in order.iter().take(FIT_SEEDS) {
        let Some(grid) = fit_from_seed(&window, seed, seed_period, center) else {
            continue;
        };
        let better = match &best {
            None => true,
            // More agreement wins; equal agreement goes to the tighter fit.
            Some(b) => (grid.support, -grid.residual_sec) > (b.support, -b.residual_sec),
        };
        if better {
            best = Some(grid);
        }
    }
    best
}

/// One grid fit, anchored on `seed` and refined by alternating inlier selection
/// with a least-squares re-estimate of period and phase.
fn fit_from_seed(window: &[f64], seed: f64, seed_period: f64, center: f64) -> Option<LocalGrid> {
    let mut period = seed_period;
    let mut anchor = seed;
    let mut inliers: Vec<(f64, f64)> = Vec::with_capacity(window.len());
    let mut residual_sec = 0.0;
    for _ in 0..FIT_PASSES {
        inliers.clear();
        for &t in window {
            let x = (t - anchor) / period;
            let k = x.round();
            if (x - k).abs() <= INLIER_TOLERANCE {
                inliers.push((k, t));
            }
        }
        if inliers.len() < MIN_GRID_SUPPORT {
            return None;
        }
        let n = inliers.len() as f64;
        let mean_k = inliers.iter().map(|(k, _)| *k).sum::<f64>() / n;
        let mean_t = inliers.iter().map(|(_, t)| *t).sum::<f64>() / n;
        let den: f64 = inliers.iter().map(|(k, _)| (k - mean_k).powi(2)).sum();
        if den <= f64::EPSILON {
            return None;
        }
        let num: f64 = inliers
            .iter()
            .map(|(k, t)| (k - mean_k) * (t - mean_t))
            .sum();
        let next_period = num / den;
        if !(MIN_PERIOD_SEC..=MAX_PERIOD_SEC).contains(&next_period) {
            return None;
        }
        period = next_period;
        anchor = mean_t - period * mean_k;
        residual_sec = (inliers
            .iter()
            .map(|(k, t)| (t - (anchor + period * k)).powi(2))
            .sum::<f64>()
            / n)
            .sqrt();
    }

    if residual_sec > period * MAX_RESIDUAL_FRACTION {
        return None;
    }

    // Re-anchor onto the grid beat nearest the centre so `anchor` stays inside
    // the fitted region and beat indices around the mix point stay small.
    let anchor = anchor + ((center - anchor) / period).round() * period;

    Some(LocalGrid {
        period,
        anchor,
        residual_sec,
        support: inliers.len(),
    })
}

/// Which beat of the bar the grid's `anchor` is, given real downbeat timestamps.
///
/// Returns the offset `d` in `0..4` such that grid beats with
/// `(n - d) % 4 == 0` are bar ones. `None` when there is no usable downbeat
/// evidence — in that case callers must align on beats only and must not claim
/// the bars line up.
pub fn bar_offset(
    grid: &LocalGrid,
    downbeats: &[f64],
    center: f64,
    half_window: f64,
) -> Option<i64> {
    let lo = center - half_window;
    let hi = center + half_window;
    let mut votes = [0usize; 4];
    let mut total = 0usize;
    for &d in downbeats {
        if !d.is_finite() || d < lo || d > hi {
            continue;
        }
        let x = (d - grid.anchor) / grid.period;
        let k = x.round();
        if (x - k).abs() > INLIER_TOLERANCE {
            continue;
        }
        let slot = k.rem_euclid(4.0) as usize;
        votes[slot % 4] += 1;
        total += 1;
    }
    if total < 2 {
        return None;
    }
    let (best, count) = votes
        .iter()
        .enumerate()
        .max_by_key(|(_, c)| **c)
        .map(|(i, c)| (i as i64, *c))?;
    // A real 4/4 downbeat series votes almost unanimously; a ragged spread
    // means the downbeats are not trustworthy at bar level.
    (count * 2 >= total).then_some(best)
}

/// Folds `bpm` by whole octaves until it sits closest to `target`.
///
/// A track tagged or detected at half or double time describes the same groove,
/// so 66 against a 128 BPM neighbour should be read as 132 rather than an
/// impossible 94% stretch.
pub fn fold_bpm_octave(target: f64, bpm: f64) -> f64 {
    if !(target.is_finite() && bpm.is_finite()) || target <= 0.0 || bpm <= 0.0 {
        return bpm;
    }
    let mut folded = bpm;
    // sqrt(2) is the break-even point: past it, the other octave is closer.
    while folded / target > std::f64::consts::SQRT_2 {
        folded /= 2.0;
    }
    while target / folded > std::f64::consts::SQRT_2 {
        folded *= 2.0;
    }
    folded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perfect_beats(bpm: f64, start: f64, count: usize) -> Vec<f64> {
        let period = 60.0 / bpm;
        (0..count).map(|i| start + i as f64 * period).collect()
    }

    #[test]
    fn fits_a_clean_grid_to_the_exact_tempo() {
        let beats = perfect_beats(125.0, 3.0, 200);
        let g = fit_local_grid(&beats, 60.0, 20.0).expect("fit");
        assert!((g.bpm() - 125.0).abs() < 1e-6, "bpm {}", g.bpm());
        assert!(g.residual_sec < 1e-9);
        assert!((g.offset_from_beat(g.nearest_beat(60.0))).abs() < 1e-9);
    }

    #[test]
    fn recovers_tempo_a_global_median_would_miss() {
        // 0.05% off 125 BPM: the kind of error that walks a grid 200 ms across a
        // track but must not survive into a blend.
        let beats = perfect_beats(125.0625, 0.0, 400);
        let g = fit_local_grid(&beats, 100.0, 25.0).expect("fit");
        assert!((g.bpm() - 125.0625).abs() < 1e-3, "bpm {}", g.bpm());
    }

    #[test]
    fn rejects_a_half_beat_slipped_section_instead_of_splitting_the_difference() {
        let period = 60.0 / 125.0;
        let mut beats = perfect_beats(125.0, 0.0, 400);
        // Slip everything past the 250th beat by half a beat, as madmom does
        // when it locks onto the offbeat for a section.
        for b in beats.iter_mut().skip(250) {
            *b += period * 0.5;
        }
        // Centre on clean territory: the slipped run is outside the window.
        let g = fit_local_grid(&beats, 40.0, 20.0).expect("fit");
        assert!((g.bpm() - 125.0).abs() < 1e-3, "bpm {}", g.bpm());
        assert!(
            g.offset_from_beat(0.0).abs() < 1e-6,
            "grid moved off the beat"
        );
    }

    #[test]
    fn picks_the_majority_grid_when_the_centre_sits_inside_a_slipped_run() {
        // The mix point lands inside a short half-beat-slipped run, with clean
        // beats either side. Seeding only on the beat nearest the centre would
        // lock onto the slip and throw the majority away; trying several seeds
        // has to recover the interpretation most beats agree with.
        let period = 60.0 / 125.0;
        let mut beats = perfect_beats(125.0, 0.0, 300);
        for b in beats.iter_mut().skip(120).take(14) {
            *b += period * 0.5;
        }
        let centre = 125.0 * period; // beat 125, inside the slipped run
        let g = fit_local_grid(&beats, centre, 30.0).expect("fit");
        assert!((g.bpm() - 125.0).abs() < 1e-3, "bpm {}", g.bpm());
        assert!(
            g.offset_from_beat(0.0).abs() < 1e-6,
            "locked onto the slipped run instead of the majority"
        );
        assert!(g.support > 100, "support was only {}", g.support);
    }

    #[test]
    fn survives_dropped_and_doubled_beats() {
        let period = 60.0 / 128.0;
        let mut beats = perfect_beats(128.0, 1.0, 300);
        beats.retain(|b| ((b - 1.0) / period).round() as i64 % 17 != 0); // drops
        let mut doubled = beats.clone();
        for i in (0..beats.len()).step_by(23) {
            doubled.push(beats[i] + period * 0.5); // spurious offbeat detections
        }
        doubled.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let g = fit_local_grid(&doubled, 60.0, 25.0).expect("fit");
        assert!((g.bpm() - 128.0).abs() < 1e-2, "bpm {}", g.bpm());
    }

    #[test]
    fn refuses_windows_with_too_little_evidence() {
        let beats = perfect_beats(125.0, 0.0, 200);
        assert!(fit_local_grid(&beats, 100.0, 1.0).is_none());
        assert!(fit_local_grid(&[], 10.0, 30.0).is_none());
    }

    #[test]
    fn refuses_noise_that_is_not_a_grid() {
        // Nominal 125 BPM beats smeared by up to +-45% of a beat. Roughly half
        // still land inside the inlier window, so the guard that has to fire is
        // the residual bound, not the support count.
        let period = 60.0 / 125.0;
        let mut seed: u64 = 0x5eed;
        let beats: Vec<f64> = (0..80)
            .map(|i| {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let jitter = ((seed >> 33) as f64 / (1u64 << 31) as f64 - 0.5) * 0.9;
                i as f64 * period + jitter * period
            })
            .collect();
        assert!(fit_local_grid(&beats, 19.0, 19.0).is_none());
    }

    #[test]
    fn reads_bar_phase_from_real_downbeats() {
        let period = 60.0 / 120.0;
        let beats = perfect_beats(120.0, 0.0, 200);
        // Bar one on every 4th beat starting at beat 2.
        let downbeats: Vec<f64> = (0..50).map(|i| (2 + i * 4) as f64 * period).collect();
        let g = fit_local_grid(&beats, 20.0, 20.0).expect("fit");
        let d = bar_offset(&g, &downbeats, 20.0, 20.0).expect("bar offset");
        // `d` is a grid-relative slot, so adding the anchor's own absolute beat
        // index must land back on the bar ones at absolute index 2 mod 4.
        let anchor_index = (g.anchor / period).round() as i64;
        assert_eq!((anchor_index + d).rem_euclid(4), 2);
    }

    #[test]
    fn rejects_bar_phase_when_downbeats_disagree() {
        let beats = perfect_beats(120.0, 0.0, 200);
        // Downbeats scattered across all four slots: no real bar evidence.
        let downbeats: Vec<f64> = (0..12).map(|i| i as f64 * 0.5).collect();
        let g = fit_local_grid(&beats, 20.0, 20.0).expect("fit");
        assert!(bar_offset(&g, &downbeats, 20.0, 20.0).is_none());
    }

    #[test]
    fn folds_octave_equivalent_tempos() {
        assert!((fold_bpm_octave(128.0, 66.0) - 132.0).abs() < 1e-9);
        assert!((fold_bpm_octave(128.0, 260.0) - 130.0).abs() < 1e-9);
        assert!((fold_bpm_octave(128.0, 125.0) - 125.0).abs() < 1e-9);
        assert!((fold_bpm_octave(0.0, 125.0) - 125.0).abs() < 1e-9);
    }
}
