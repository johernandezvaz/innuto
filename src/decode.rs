//! Audio file decoding into interleaved stereo f32 frames.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTagKey, Tag};
use symphonia::core::probe::Hint;

pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "flac", "aiff", "aif", "m4a", "aac", "ogg"];

/// Decoded audio: interleaved stereo samples at the source sample rate.
pub struct Audio {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

#[derive(Default)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
}

type Probed = (Box<dyn symphonia::core::formats::FormatReader>, Tags);

fn probe(path: &Path) -> Result<Probed> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .with_context(|| format!("unsupported audio format: {}", path.display()))?;

    let mut tags: Vec<Tag> = Vec::new();
    if let Some(mut md) = probed.metadata.get() {
        if let Some(rev) = md.skip_to_latest() {
            tags.extend(rev.tags().iter().cloned());
        }
    }
    let mut format = probed.format;
    if let Some(rev) = format.metadata().current() {
        tags.extend(rev.tags().iter().cloned());
    }
    let tag = |key: StandardTagKey| {
        tags.iter().find(|t| t.std_key == Some(key)).map(|t| t.value.to_string())
    };
    let tags = Tags { title: tag(StandardTagKey::TrackTitle), artist: tag(StandardTagKey::Artist) };
    Ok((format, tags))
}

/// Reads title and artist without decoding audio.
pub fn read_tags(path: &Path) -> Result<Tags> {
    Ok(probe(path)?.1)
}

impl Audio {
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    pub fn duration_secs(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }

    /// Mono mixdown, used by analysis.
    pub fn mono(&self) -> Vec<f32> {
        self.samples.chunks_exact(2).map(|f| 0.5 * (f[0] + f[1])).collect()
    }
}

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn decode_file(path: &Path) -> Result<Audio> {
    let (mut format, _) = probe(path)?;
    let track = format.default_track().ok_or_else(|| anyhow!("no audio track"))?;
    let track_id = track.id;
    let params = track.codec_params.clone();
    let sample_rate = params.sample_rate.ok_or_else(|| anyhow!("unknown sample rate"))?;
    let mut decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default())?;

    let mut samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let spec = *buf.spec();
        let channels = spec.channels.count().max(1);
        let mut sb = SampleBuffer::<f32>::new(buf.capacity() as u64, spec);
        sb.copy_interleaved_ref(buf);
        for frame in sb.samples().chunks_exact(channels) {
            let l = frame[0];
            let r = if channels > 1 { frame[1] } else { l };
            samples.push(l);
            samples.push(r);
        }
    }
    Ok(Audio { sample_rate, samples })
}
