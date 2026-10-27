//! Audio output thread for the mixer, with load measurement and recording.
//!
//! The cpal stream lives on its own thread, so the stream handle never has to
//! be shared and audio keeps running regardless of what callers do.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::engine::Mixer;

/// Ten seconds of stereo at 48 kHz, so the audio thread rarely reallocates.
const RECORD_RESERVE: usize = 48_000 * 2 * 10;

struct Recording {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    writer: thread::JoinHandle<Result<()>>,
}

pub struct Output {
    pub sample_rate: u32,
    /// Fraction of the buffer period spent rendering, as f32 bits.
    load: Arc<AtomicU32>,
    /// Samples captured by the audio thread while recording.
    capture: Arc<Mutex<Option<Vec<f32>>>>,
    recording: Mutex<Option<Recording>>,
    _shutdown: mpsc::Sender<()>,
}

pub fn start(mixer: Arc<Mutex<Mixer>>) -> Result<Output> {
    let load = Arc::new(AtomicU32::new(0));
    let capture: Arc<Mutex<Option<Vec<f32>>>> = Arc::new(Mutex::new(None));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<u32>>();
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
    let (thread_load, thread_capture) = (Arc::clone(&load), Arc::clone(&capture));

    thread::Builder::new().name("innuto-audio".into()).spawn(move || {
        let stream = match build_stream(mixer, thread_load, thread_capture) {
            Ok((stream, rate)) => {
                let _ = ready_tx.send(Ok(rate));
                stream
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        // Blocks until the Output is dropped.
        let _ = shutdown_rx.recv();
        drop(stream);
    })?;

    let sample_rate = ready_rx.recv().map_err(|_| anyhow!("audio thread exited"))??;
    Ok(Output { sample_rate, load, capture, recording: Mutex::new(None), _shutdown: shutdown_tx })
}

fn build_stream(
    mixer: Arc<Mutex<Mixer>>,
    load: Arc<AtomicU32>,
    capture: Arc<Mutex<Option<Vec<f32>>>>,
) -> Result<(cpal::Stream, u32)> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| anyhow!("no audio output device"))?;
    let config = device.default_output_config()?;
    if config.sample_format() != cpal::SampleFormat::F32 {
        bail!("output device uses {:?} samples; innuto currently needs f32", config.sample_format());
    }
    let channels = config.channels() as usize;
    let rate = config.sample_rate().0;
    let mut scratch = Vec::new();
    let stream = device.build_output_stream(
        &config.into(),
        move |data: &mut [f32], _| {
            let started = Instant::now();
            let frames = data.len() / channels;
            scratch.resize(frames * 2, 0.0);
            match mixer.try_lock() {
                Ok(mut m) => m.render(&mut scratch, rate),
                Err(_) => scratch.fill(0.0),
            }
            for (out, st) in data.chunks_exact_mut(channels).zip(scratch.chunks_exact(2)) {
                for (c, s) in out.iter_mut().enumerate() {
                    *s = if c < 2 { st[c] } else { 0.0 };
                }
            }
            if let Ok(mut cap) = capture.try_lock() {
                if let Some(buf) = cap.as_mut() {
                    buf.extend_from_slice(&scratch);
                }
            }
            let period = frames as f32 / rate as f32;
            let used = started.elapsed().as_secs_f32() / period.max(1e-6);
            load.store(used.to_bits(), Ordering::Relaxed);
        },
        |e| eprintln!("audio error: {e}"),
        None,
    )?;
    stream.play()?;
    Ok((stream, rate))
}

impl Output {
    pub fn load(&self) -> f32 {
        f32::from_bits(self.load.load(Ordering::Relaxed))
    }

    pub fn recording_path(&self) -> Option<PathBuf> {
        self.recording.lock().ok()?.as_ref().map(|r| r.path.clone())
    }

    /// Starts writing the master output to a 32-bit float WAV file.
    pub fn start_recording(&self, path: &Path) -> Result<()> {
        let mut rec = self.recording.lock().map_err(|_| anyhow!("recording lock poisoned"))?;
        if rec.is_some() {
            bail!("already recording");
        }
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: self.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut wav = hound::WavWriter::create(path, spec).with_context(|| format!("creating {}", path.display()))?;
        *self.capture.lock().map_err(|_| anyhow!("capture lock poisoned"))? = Some(Vec::with_capacity(RECORD_RESERVE));
        let stop = Arc::new(AtomicBool::new(false));
        let (capture, thread_stop) = (Arc::clone(&self.capture), Arc::clone(&stop));
        let writer = thread::Builder::new().name("innuto-record".into()).spawn(move || -> Result<()> {
            let mut spare = Vec::with_capacity(RECORD_RESERVE);
            loop {
                let stopping = thread_stop.load(Ordering::Relaxed);
                thread::sleep(Duration::from_millis(200));
                if let Ok(mut cap) = capture.lock() {
                    match cap.as_mut() {
                        Some(buf) => std::mem::swap(buf, &mut spare),
                        None => break,
                    }
                    if stopping {
                        *cap = None;
                    }
                }
                for s in spare.drain(..) {
                    wav.write_sample(s)?;
                }
                if stopping {
                    break;
                }
            }
            wav.finalize()?;
            Ok(())
        })?;
        *rec = Some(Recording { path: path.to_path_buf(), stop, writer });
        Ok(())
    }

    /// Stops recording and returns the finished file's path.
    pub fn stop_recording(&self) -> Result<Option<PathBuf>> {
        let rec = self.recording.lock().map_err(|_| anyhow!("recording lock poisoned"))?.take();
        let Some(rec) = rec else { return Ok(None) };
        rec.stop.store(true, Ordering::Relaxed);
        rec.writer.join().map_err(|_| anyhow!("recording thread panicked"))??;
        Ok(Some(rec.path))
    }
}
