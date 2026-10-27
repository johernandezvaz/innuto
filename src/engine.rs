//! Two-deck playback engine: tempo control, cue, beat sync, EQ, filter, and crossfader.

use std::sync::Arc;

pub struct Track {
    pub sample_rate: u32,
    /// Interleaved stereo samples.
    pub samples: Arc<Vec<f32>>,
    pub bpm: f64,
    pub first_beat_ms: f64,
}

/// Beat-phase difference (fraction of a beat) beyond which a synced deck is
/// reported as drifting.
const SYNC_LOST_PHASE: f64 = 0.04;
const EQ_LOW_HZ: f32 = 250.0;
const EQ_HIGH_HZ: f32 = 2_500.0;

fn one_pole_coeff(cutoff_hz: f32, rate: u32) -> f32 {
    1.0 - (-2.0 * std::f32::consts::PI * cutoff_hz / rate as f32).exp()
}

/// Per-channel filter memory for the EQ split and the sweep filter.
#[derive(Default, Clone, Copy)]
struct ChannelState {
    eq_low: f32,
    eq_lowmid: f32,
    filt: [f32; 2],
}

#[derive(Default)]
pub struct Deck {
    pub track: Option<Track>,
    /// Playhead in source frames.
    pub position: f64,
    pub playing: bool,
    /// Playback rate; 1.0 is the original tempo.
    pub rate: f64,
    pub cue: f64,
    /// Channel fader, 0..=1.
    pub gain: f32,
    /// Linear gains for low, mid, and high bands; 1.0 is flat.
    pub eq: [f32; 3],
    /// -1..0 sweeps a low-pass down, 0..1 sweeps a high-pass up.
    pub filter: f32,
    /// Tempo follows the other deck while engaged.
    pub sync_engaged: bool,
    /// Holding cue while paused previews from the cue point.
    previewing: bool,
    /// Post-fader, pre-crossfader peak of the last rendered block.
    pub peak: [f32; 2],
    state: [ChannelState; 2],
}

impl Deck {
    pub fn new() -> Self {
        Deck { rate: 1.0, gain: 1.0, eq: [1.0; 3], ..Default::default() }
    }

    pub fn load(&mut self, track: Track) {
        self.cue = track.first_beat_ms / 1000.0 * track.sample_rate as f64;
        self.position = self.cue;
        self.track = Some(track);
        self.playing = false;
        self.previewing = false;
        self.rate = 1.0;
        self.sync_engaged = false;
        self.state = Default::default();
    }

    pub fn toggle_play(&mut self) {
        self.playing = self.track.is_some() && !self.playing;
        self.previewing = false;
    }

    /// Paused: store the cue at the playhead. Playing: return to the cue and stop.
    pub fn cue_button(&mut self) {
        self.cue_down();
        self.cue_up();
    }

    /// Playing: return to the cue and stop. Paused: set the cue here and
    /// preview from it until `cue_up`.
    pub fn cue_down(&mut self) {
        if self.track.is_none() {
            return;
        }
        if self.playing && !self.previewing {
            self.position = self.cue;
            self.playing = false;
        } else if !self.playing {
            self.cue = self.position;
            self.playing = true;
            self.previewing = true;
        }
    }

    pub fn cue_up(&mut self) {
        if self.previewing {
            self.previewing = false;
            self.playing = false;
            self.position = self.cue;
        }
    }

    pub fn seek_seconds(&mut self, secs: f64) {
        if let Some(t) = &self.track {
            let max = (t.samples.len() / 2).saturating_sub(2) as f64;
            self.position = (secs * t.sample_rate as f64).clamp(0.0, max);
        }
    }

    pub fn set_pitch_percent(&mut self, pct: f64) {
        self.rate = (1.0 + pct / 100.0).clamp(0.5, 2.0);
        self.sync_engaged = false;
    }

    pub fn pitch_percent(&self) -> f64 {
        (self.rate - 1.0) * 100.0
    }

    pub fn current_bpm(&self) -> Option<f64> {
        self.track.as_ref().map(|t| t.bpm * self.rate)
    }

    pub fn seconds(&self) -> f64 {
        self.track.as_ref().map_or(0.0, |t| self.position / t.sample_rate as f64)
    }

    pub fn duration_seconds(&self) -> f64 {
        self.track.as_ref().map_or(0.0, |t| (t.samples.len() / 2) as f64 / t.sample_rate as f64)
    }

    pub fn cue_seconds(&self) -> f64 {
        self.track.as_ref().map_or(0.0, |t| self.cue / t.sample_rate as f64)
    }

    /// Fraction of the current beat elapsed, in [0, 1).
    pub fn beat_phase(&self) -> Option<f64> {
        let t = self.track.as_ref()?;
        let beat = 60.0 / t.bpm;
        Some(((self.seconds() - t.first_beat_ms / 1000.0) / beat).rem_euclid(1.0))
    }

    /// Matches `master`'s effective tempo and beat phase.
    pub fn sync_to(&mut self, master: &Deck) -> bool {
        let (Some(master_bpm), Some(master_phase)) = (master.current_bpm(), master.beat_phase()) else {
            return false;
        };
        let (Some(t), Some(phase)) = (self.track.as_ref(), self.beat_phase()) else {
            return false;
        };
        self.rate = master_bpm / t.bpm;
        let beat_frames = 60.0 / t.bpm * t.sample_rate as f64;
        self.position = (self.position + wrap_phase(master_phase - phase) * beat_frames).max(0.0);
        true
    }

    /// Adds this deck's output into `out` (interleaved stereo at `out_rate`).
    fn mix_into(&mut self, out: &mut [f32], out_rate: u32, xfade: f32) {
        self.peak = [0.0; 2];
        let Some(t) = self.track.as_ref() else { return };
        if !self.playing {
            return;
        }
        let frames = t.samples.len() / 2;
        let step = self.rate * t.sample_rate as f64 / out_rate as f64;
        let (a_low, a_high) = (one_pole_coeff(EQ_LOW_HZ, out_rate), one_pole_coeff(EQ_HIGH_HZ, out_rate));
        let filter = self.filter.clamp(-1.0, 1.0);
        let filt_a = if filter < 0.0 {
            // 20 kHz down to 100 Hz.
            one_pole_coeff(20_000.0 * (100.0f32 / 20_000.0).powf(-filter), out_rate)
        } else {
            // 20 Hz up to 8 kHz.
            one_pole_coeff(20.0 * (8_000.0f32 / 20.0).powf(filter), out_rate)
        };
        let filter_active = filter.abs() > 0.01;
        let [gl, gm, gh] = self.eq;
        for frame in out.chunks_exact_mut(2) {
            let i = self.position as usize;
            if i + 1 >= frames {
                self.playing = false;
                self.previewing = false;
                break;
            }
            let f = (self.position - i as f64) as f32;
            for ch in 0..2 {
                let a = t.samples[i * 2 + ch];
                let b = t.samples[(i + 1) * 2 + ch];
                let x = a + (b - a) * f;
                let st = &mut self.state[ch];
                // Three bands that sum back to the input exactly at unity gain.
                st.eq_low += a_low * (x - st.eq_low);
                st.eq_lowmid += a_high * (x - st.eq_lowmid);
                let (low, mid, high) = (st.eq_low, st.eq_lowmid - st.eq_low, x - st.eq_lowmid);
                let mut y = gl * low + gm * mid + gh * high;
                if filter_active {
                    // Two cascaded one-pole stages: 12 dB/octave.
                    st.filt[0] += filt_a * (y - st.filt[0]);
                    st.filt[1] += filt_a * (st.filt[0] - st.filt[1]);
                    y = if filter < 0.0 { st.filt[1] } else { y - st.filt[1] };
                }
                let y = y * self.gain;
                self.peak[ch] = self.peak[ch].max(y.abs());
                frame[ch] += y * xfade;
            }
            self.position += step;
        }
    }
}

fn wrap_phase(mut delta: f64) -> f64 {
    if delta > 0.5 {
        delta -= 1.0;
    } else if delta < -0.5 {
        delta += 1.0;
    }
    delta
}

pub struct Mixer {
    pub decks: [Deck; 2],
    /// -1.0 is fully deck A, 1.0 fully deck B.
    pub crossfader: f32,
    pub master_peak: [f32; 2],
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

impl Mixer {
    pub fn new() -> Self {
        Mixer { decks: [Deck::new(), Deck::new()], crossfader: 0.0, master_peak: [0.0; 2] }
    }

    /// Constant-power crossfader gains for decks A and B.
    pub fn crossfader_gains(&self) -> (f32, f32) {
        let x = (self.crossfader.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        (x.cos(), x.sin())
    }

    pub fn render(&mut self, out: &mut [f32], out_rate: u32) {
        out.fill(0.0);
        for d in 0..2 {
            if self.decks[d].sync_engaged {
                if let (Some(master_bpm), Some(t)) = (self.decks[1 - d].current_bpm(), &self.decks[d].track) {
                    self.decks[d].rate = master_bpm / t.bpm;
                }
            }
        }
        let (ga, gb) = self.crossfader_gains();
        self.decks[0].mix_into(out, out_rate, ga);
        self.decks[1].mix_into(out, out_rate, gb);
        self.master_peak = [0.0; 2];
        for frame in out.chunks_exact_mut(2) {
            for ch in 0..2 {
                frame[ch] = frame[ch].clamp(-1.0, 1.0);
                self.master_peak[ch] = self.master_peak[ch].max(frame[ch].abs());
            }
        }
    }

    /// One-shot beat sync of `slave` to the other deck.
    pub fn sync(&mut self, slave: usize) -> bool {
        let [a, b] = &mut self.decks;
        if slave == 0 {
            a.sync_to(b)
        } else {
            b.sync_to(a)
        }
    }

    /// Engages or releases tempo lock on `deck`; engaging also aligns phase.
    pub fn set_sync(&mut self, deck: usize, engaged: bool) -> bool {
        if !engaged {
            self.decks[deck].sync_engaged = false;
            return true;
        }
        if !self.sync(deck) {
            return false;
        }
        self.decks[deck].sync_engaged = true;
        self.decks[1 - deck].sync_engaged = false;
        true
    }

    /// True when `deck` is sync-locked but its beats drifted from the master's.
    pub fn sync_lost(&self, deck: usize) -> bool {
        let (d, m) = (&self.decks[deck], &self.decks[1 - deck]);
        if !d.sync_engaged || !d.playing || !m.playing {
            return false;
        }
        match (d.beat_phase(), m.beat_phase()) {
            (Some(a), Some(b)) => wrap_phase(a - b).abs() > SYNC_LOST_PHASE,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(bpm: f64, first_beat_ms: f64, secs: f64) -> Track {
        let rate = 1000;
        let samples = (0..(secs * rate as f64) as usize * 2).map(|i| (i / 2) as f32).collect();
        Track { sample_rate: rate, samples: Arc::new(samples), bpm, first_beat_ms }
    }

    #[test]
    fn cue_button_sets_then_returns() {
        let mut d = Deck::new();
        d.load(track(120.0, 0.0, 10.0));
        d.position = 500.0;
        d.cue_button();
        d.toggle_play();
        d.position = 900.0;
        d.cue_button();
        assert_eq!((d.position, d.playing), (500.0, false));
    }

    #[test]
    fn holding_cue_previews_and_release_returns() {
        let mut d = Deck::new();
        d.load(track(120.0, 0.0, 10.0));
        d.position = 300.0;
        d.cue_down();
        assert!(d.playing);
        d.position = 700.0;
        d.cue_up();
        assert_eq!((d.position, d.playing, d.cue), (300.0, false, 300.0));
    }

    #[test]
    fn rate_scales_playhead_and_bpm() {
        let mut m = Mixer::new();
        m.decks[0].load(track(100.0, 0.0, 10.0));
        m.decks[0].set_pitch_percent(10.0);
        m.decks[0].toggle_play();
        let mut out = vec![0.0; 200];
        m.render(&mut out, 1000);
        assert!((m.decks[0].position - 110.0).abs() < 1e-6);
        assert!((m.decks[0].current_bpm().unwrap() - 110.0).abs() < 1e-9);
    }

    #[test]
    fn sync_matches_tempo_and_phase() {
        let mut m = Mixer::new();
        m.decks[0].load(track(128.0, 100.0, 60.0));
        m.decks[1].load(track(124.0, 0.0, 60.0));
        m.decks[0].position = 5_321.0;
        m.decks[1].position = 2_000.0;
        assert!(m.sync(1));
        let (a, b) = (&m.decks[0], &m.decks[1]);
        assert!((b.current_bpm().unwrap() - 128.0).abs() < 1e-9);
        let diff = (a.beat_phase().unwrap() - b.beat_phase().unwrap()).abs();
        assert!(diff < 1e-6 || (1.0 - diff) < 1e-6, "phase diff {diff}");
    }

    #[test]
    fn sync_lock_follows_master_tempo_and_reports_drift() {
        let mut m = Mixer::new();
        m.decks[0].load(track(128.0, 0.0, 60.0));
        m.decks[1].load(track(124.0, 0.0, 60.0));
        assert!(m.set_sync(1, true));
        m.decks[0].toggle_play();
        m.decks[1].toggle_play();
        m.decks[0].rate = 1.02;
        let mut out = vec![0.0; 64];
        m.render(&mut out, 1000);
        assert!((m.decks[1].current_bpm().unwrap() - 128.0 * 1.02).abs() < 1e-9);
        assert!(!m.sync_lost(1));
        m.decks[1].position += 100.0;
        assert!(m.sync_lost(1));
        m.decks[1].set_pitch_percent(1.0);
        assert!(!m.decks[1].sync_engaged);
    }

    #[test]
    fn flat_eq_reconstructs_input_and_kill_removes_lows() {
        let rate = 44_100;
        let samples: Vec<f32> = (0..rate as usize * 2)
            .map(|i| (2.0 * std::f32::consts::PI * 60.0 * (i / 2) as f32 / rate as f32).sin() * 0.5)
            .collect();
        let mut m = Mixer::new();
        m.crossfader = -1.0;
        m.decks[0].load(Track { sample_rate: rate, samples: Arc::new(samples.clone()), bpm: 120.0, first_beat_ms: 0.0 });
        m.decks[0].toggle_play();
        let mut out = vec![0.0; 8_192];
        m.render(&mut out, rate);
        for (o, s) in out.iter().zip(&samples) {
            assert!((o - s).abs() < 1e-4);
        }
        m.decks[0].eq = [0.0, 1.0, 1.0];
        for _ in 0..4 {
            m.render(&mut out, rate);
        }
        assert!(m.decks[0].peak[0] < 0.2, "60 Hz peak after low kill {}", m.decks[0].peak[0]);
    }

    #[test]
    fn crossfader_is_constant_power() {
        let mut m = Mixer::new();
        for x in [-1.0, -0.3, 0.0, 0.7, 1.0] {
            m.crossfader = x;
            let (a, b) = m.crossfader_gains();
            assert!((a * a + b * b - 1.0).abs() < 1e-5);
        }
        m.crossfader = -1.0;
        assert!(m.crossfader_gains().1.abs() < 1e-6);
    }
}
