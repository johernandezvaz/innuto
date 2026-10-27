//! Track analysis: tempo, beat grid, and waveform preview.

const HOP: usize = 512;
const MIN_BPM: f64 = 78.0;
const MAX_BPM: f64 = 176.0;
pub const PREVIEW_COLUMNS: usize = 400;

#[derive(Debug, Clone, PartialEq)]
pub struct Beat {
    /// Position within the bar, 1..=4.
    pub bar_position: u16,
    pub time_ms: u32,
}

#[derive(Debug, Clone)]
pub struct Analysis {
    pub bpm: f64,
    pub first_beat_ms: f64,
    /// One byte per column: low 5 bits height (0..=31), high 3 bits brightness (0..=7).
    pub preview: Vec<u8>,
}

/// Finer hop for the low-band envelope that places beats (~2.9 ms at 44.1 kHz).
const FINE_HOP: usize = 128;
/// Kick drums sit below this; low-band onsets mark beats, not off-beat hats.
const LOW_BAND_HZ: f32 = 150.0;

pub fn analyze(mono: &[f32], sample_rate: u32) -> Analysis {
    let low = low_pass(mono, sample_rate, LOW_BAND_HZ);
    let coarse = estimate_tempo(&combined_envelope(mono, &low, HOP, Blend::Either), sample_rate);
    let fine = smooth(&combined_envelope(mono, &low, FINE_HOP, Blend::Both), 5);
    let (bpm, first_beat_ms) = refine_grid(&fine, sample_rate, coarse);
    Analysis {
        bpm,
        first_beat_ms,
        preview: waveform_preview(mono, PREVIEW_COLUMNS),
    }
}

fn low_pass(mono: &[f32], sample_rate: u32, cutoff_hz: f32) -> Vec<f32> {
    let a = 1.0 - (-2.0 * std::f32::consts::PI * cutoff_hz / sample_rate as f32).exp();
    let mut y = 0.0f32;
    mono.iter()
        .map(|&x| {
            y += a * (x - y);
            y
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Blend {
    /// Onsets in either band: robust periodicity for tempo estimation.
    Either,
    /// Onsets in both bands at once: kicks are broadband transients, so this
    /// keeps the grid off bassline notes and off-beat hats.
    Both,
}

/// Blends low-band and full-band onsets, each normalized to unit mean so
/// loudness differences between bands do not decide the mix.
fn combined_envelope(full: &[f32], low: &[f32], hop: usize, blend: Blend) -> Vec<f32> {
    let normalize = |env: Vec<f32>| {
        let mean = env.iter().sum::<f32>() / env.len().max(1) as f32;
        let scale = if mean > 0.0 { 1.0 / mean } else { 0.0 };
        env.into_iter().map(move |v| v * scale)
    };
    normalize(onset_envelope(low, hop))
        .zip(normalize(onset_envelope(full, hop)))
        .map(|(l, f)| match blend {
            Blend::Either => 2.0 * l + f,
            Blend::Both => (l * f).sqrt(),
        })
        .collect()
}

/// Centered moving average, so a single noisy hop cannot win the phase search.
fn smooth(env: &[f32], width: usize) -> Vec<f32> {
    let half = width / 2;
    (0..env.len())
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(half), (i + half + 1).min(env.len()));
            env[lo..hi].iter().sum::<f32>() / (hi - lo) as f32
        })
        .collect()
}

/// Strongest beat comb for a period (in envelope hops) over the whole track:
/// returns (score, phase in hops).
fn best_comb(env: &[f32], period: f64) -> (f64, f64) {
    let mut best = (0.0f64, 0.0f64);
    for p in 0..period.ceil() as usize {
        let mut score = 0.0f64;
        let mut x = p as f64;
        while x < env.len() as f64 {
            score += sample_at(env, x) as f64;
            x += period;
        }
        if score > best.0 {
            best = (score, p as f64);
        }
    }
    best
}

/// Narrows a coarse tempo to 0.01 BPM using whole-track beat combs on the
/// low-band envelope, then places the grid. Returns (bpm, first beat ms).
fn refine_grid(env: &[f32], sample_rate: u32, coarse_bpm: f64) -> (f64, f64) {
    let hops_per_sec = sample_rate as f64 / FINE_HOP as f64;
    if env.len() < (hops_per_sec * 4.0) as usize {
        return (coarse_bpm, 0.0);
    }
    let period = |bpm: f64| 60.0 * hops_per_sec / bpm;
    let mut best = (0.0f64, coarse_bpm);
    for step in -100..=100 {
        let bpm = coarse_bpm + step as f64 * 0.01;
        let (score, _) = best_comb(env, period(bpm));
        if score > best.0 {
            best = (score, bpm);
        }
    }
    // Dance music is overwhelmingly produced on whole or half BPM values.
    let mut bpm = best.1;
    let rounded = (bpm * 2.0).round() / 2.0;
    if (bpm - rounded).abs() <= 0.05 {
        bpm = rounded;
    }
    let (_, phase) = best_comb(env, period(bpm));
    (bpm, phase / hops_per_sec * 1000.0)
}

/// Positive energy flux: the rise in RMS energy between consecutive hops.
fn onset_envelope(mono: &[f32], hop: usize) -> Vec<f32> {
    let energies: Vec<f32> = mono
        .chunks(hop)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let mut env = Vec::with_capacity(energies.len());
    env.push(0.0);
    for w in energies.windows(2) {
        env.push((w[1] - w[0]).max(0.0));
    }
    let mean = env.iter().sum::<f32>() / env.len().max(1) as f32;
    env.iter().map(|v| (v - mean).max(0.0)).collect()
}

fn sample_at(env: &[f32], x: f64) -> f32 {
    let i = x.floor() as usize;
    if i + 1 >= env.len() {
        return 0.0;
    }
    let t = (x - i as f64) as f32;
    env[i] * (1.0 - t) + env[i + 1] * t
}

/// Coarse tempo (0.05 BPM steps) from autocorrelation of the onset envelope.
fn estimate_tempo(env: &[f32], sample_rate: u32) -> f64 {
    let hops_per_sec = sample_rate as f64 / HOP as f64;
    if env.len() < (hops_per_sec * 4.0) as usize {
        return 120.0;
    }
    // Two minutes of envelope is plenty for a stable tempo estimate.
    let ac_len = env.len().min((hops_per_sec * 120.0) as usize) as f64;
    let mut best = (0.0f64, 120.0f64);
    let mut bpm = MIN_BPM;
    while bpm <= MAX_BPM {
        let lag = 60.0 * hops_per_sec / bpm;
        let mut score = 0.0f64;
        let mut i = 0usize;
        while (i as f64 + 4.0 * lag) < ac_len {
            let a = env[i] as f64;
            if a > 0.0 {
                for k in 1..=4 {
                    score += a * sample_at(env, i as f64 + k as f64 * lag) as f64;
                }
            }
            i += 1;
        }
        if score > best.0 {
            best = (score, bpm);
        }
        bpm += 0.05;
    }
    best.1
}

pub fn beat_grid(bpm: f64, first_beat_ms: f64, duration_ms: f64) -> Vec<Beat> {
    let beat_ms = 60_000.0 / bpm;
    let mut beats = Vec::new();
    let mut t = first_beat_ms;
    let mut n = 0u16;
    while t < duration_ms {
        beats.push(Beat { bar_position: n % 4 + 1, time_ms: t.round() as u32 });
        n = n.wrapping_add(1);
        t += beat_ms;
    }
    beats
}

pub const BAND_COLUMNS_PER_SECOND: u32 = 150;

/// Detailed waveform: three bytes per column (low, mid, high band peaks),
/// all scaled by the loudest band so their heights stay comparable. Bands
/// split at 200 Hz and 2.5 kHz.
pub fn band_waveform(mono: &[f32], sample_rate: u32) -> Vec<u8> {
    let per_col = (sample_rate / BAND_COLUMNS_PER_SECOND).max(1) as usize;
    let (a_low, a_high) = (
        1.0 - (-2.0 * std::f32::consts::PI * 200.0 / sample_rate as f32).exp(),
        1.0 - (-2.0 * std::f32::consts::PI * 2_500.0 / sample_rate as f32).exp(),
    );
    let (mut low, mut lowmid) = (0.0f32, 0.0f32);
    let mut peaks: Vec<[f32; 3]> = Vec::with_capacity(mono.len() / per_col + 1);
    for chunk in mono.chunks(per_col) {
        let mut p = [0.0f32; 3];
        for &x in chunk {
            low += a_low * (x - low);
            lowmid += a_high * (x - lowmid);
            p[0] = p[0].max(low.abs());
            p[1] = p[1].max((lowmid - low).abs());
            p[2] = p[2].max((x - lowmid).abs());
        }
        peaks.push(p);
    }
    let max = peaks.iter().flatten().fold(0.0f32, |m, v| m.max(*v)).max(1e-6);
    peaks.iter().flat_map(|p| p.map(|v| (v / max * 255.0).round() as u8)).collect()
}

/// Peak height per column, with brightness from zero-crossing rate (a cheap
/// proxy for high-frequency content).
pub fn waveform_preview(mono: &[f32], columns: usize) -> Vec<u8> {
    if mono.is_empty() {
        return vec![0; columns];
    }
    let peak_all = mono.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-6);
    (0..columns)
        .map(|c| {
            let start = c * mono.len() / columns;
            let end = ((c + 1) * mono.len() / columns).max(start + 1).min(mono.len());
            let chunk = &mono[start..end];
            let peak = chunk.iter().fold(0.0f32, |m, s| m.max(s.abs())) / peak_all;
            let crossings = chunk.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count();
            let zcr = crossings as f32 / chunk.len().max(1) as f32;
            let height = (peak * 31.0).round() as u8;
            let brightness = ((zcr * 40.0).min(7.0)) as u8;
            height.min(31) | (brightness << 5)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click_track(bpm: f64, offset_s: f64, secs: f64, rate: u32) -> Vec<f32> {
        let n = (secs * rate as f64) as usize;
        let mut v = vec![0.0f32; n];
        let beat = 60.0 / bpm;
        let mut t = offset_s;
        while t < secs {
            let start = (t * rate as f64) as usize;
            // A 60 Hz kick on the beat and a bright hat on the off-beat.
            for i in 0..(rate as usize / 10) {
                let decay = (-(i as f32) / (rate as f32 * 0.02)).exp();
                let kick = decay * (2.0 * std::f32::consts::PI * 60.0 * i as f32 / rate as f32).sin();
                if start + i < n {
                    v[start + i] += kick;
                }
                let hat_at = start + (beat / 2.0 * rate as f64) as usize + i;
                if hat_at < n && i < rate as usize / 100 {
                    v[hat_at] += 0.8 * decay * if i % 2 == 0 { 1.0 } else { -1.0 };
                }
            }
            t += beat;
        }
        v
    }

    #[test]
    fn detects_tempo_and_phase_of_click_track() {
        let rate = 44_100;
        let audio = click_track(128.0, 0.25, 30.0, rate);
        let a = analyze(&audio, rate);
        assert!((a.bpm - 128.0).abs() < 0.3, "bpm {}", a.bpm);
        let beat_ms = 60_000.0 / 128.0;
        let phase_err = (a.first_beat_ms - 250.0).rem_euclid(beat_ms);
        let phase_err = phase_err.min(beat_ms - phase_err);
        assert!(phase_err < 25.0, "first beat {} ms", a.first_beat_ms);
    }

    #[test]
    fn grid_cycles_bar_positions() {
        let grid = beat_grid(120.0, 100.0, 2_700.0);
        let positions: Vec<u16> = grid.iter().map(|b| b.bar_position).collect();
        assert_eq!(positions, vec![1, 2, 3, 4, 1, 2]);
        assert_eq!(grid[1].time_ms, 600);
    }

    #[test]
    fn band_waveform_puts_bass_in_the_low_band() {
        let rate = 44_100;
        let bass: Vec<f32> = (0..rate).map(|i| (2.0 * std::f32::consts::PI * 50.0 * i as f32 / rate as f32).sin()).collect();
        let w = band_waveform(&bass, rate);
        assert_eq!(w.len(), BAND_COLUMNS_PER_SECOND as usize * 3);
        let mid_col = &w[w.len() / 2..w.len() / 2 + 3];
        assert!(mid_col[0] > 200 && mid_col[2] < 30, "{mid_col:?}");
    }

    #[test]
    fn preview_has_requested_columns_and_valid_heights() {
        let p = waveform_preview(&click_track(120.0, 0.0, 5.0, 8_000), 400);
        assert_eq!(p.len(), 400);
        assert!(p.iter().all(|b| b & 0x1f <= 31));
        assert!(p.iter().any(|b| b & 0x1f == 31));
    }
}
