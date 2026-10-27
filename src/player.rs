//! Real-time output and controls for the two-deck engine.

use std::io::BufRead;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};

use crate::engine::Mixer;
use crate::midimap::Mapping;

pub const HELP: &str = "\
commands (deck is a or b):
  p <deck>          play/pause
  c <deck>          cue (paused: set cue, playing: return to cue)
  t <deck> <pct>    pitch in percent, e.g. t a -2.5
  s <deck>          sync deck to the other deck
  x <-1..1>         crossfader (-1 = A, 1 = B)
  i                 deck status
  q                 quit";

fn deck_index(s: Option<&str>) -> Result<usize> {
    match s {
        Some("a" | "A" | "1") => Ok(0),
        Some("b" | "B" | "2") => Ok(1),
        _ => bail!("expected deck a or b"),
    }
}

/// Applies one console command; returns false on quit.
pub fn apply_command(mixer: &mut Mixer, line: &str) -> Result<bool> {
    let mut words = line.split_whitespace();
    match words.next() {
        None => {}
        Some("q") => return Ok(false),
        Some("p") => mixer.decks[deck_index(words.next())?].toggle_play(),
        Some("c") => mixer.decks[deck_index(words.next())?].cue_button(),
        Some("t") => {
            let d = deck_index(words.next())?;
            let pct: f64 = words.next().context("missing percent")?.parse()?;
            mixer.decks[d].set_pitch_percent(pct);
        }
        Some("s") => {
            if !mixer.sync(deck_index(words.next())?) {
                bail!("both decks need a loaded, analyzed track to sync");
            }
        }
        Some("x") => mixer.crossfader = words.next().context("missing value")?.parse::<f32>()?.clamp(-1.0, 1.0),
        Some("i") => {
            for (name, d) in ["A", "B"].iter().zip(&mixer.decks) {
                println!(
                    "{name}: {} {:>7.2}s  pitch {:+.2}%  bpm {}",
                    if d.playing { "PLAY " } else { "PAUSE" },
                    d.seconds(),
                    d.pitch_percent(),
                    d.current_bpm().map_or("-".into(), |b| format!("{b:.2}")),
                );
            }
            println!("crossfader {:+.2}", mixer.crossfader);
        }
        Some(other) => bail!("unknown command {other:?}\n{HELP}"),
    }
    Ok(true)
}

/// Translates a controller message using a rekordbox-layout mapping.
pub fn apply_midi(mixer: &mut Mixer, mapping: &Mapping, msg: &[u8]) {
    let [status, data1, value] = match msg {
        [s, d, v, ..] => [*s, *d, *v],
        _ => return,
    };
    let Some(b) = mapping.lookup(status, data1) else { return };
    let pressed = value > 0 && status & 0xF0 == 0x90;
    let deck = b.deck.filter(|&d| d < 2);
    let unit = value as f32 / 127.0;
    match (b.function.as_str(), deck) {
        ("PlayPause", Some(d)) if pressed => mixer.decks[d].toggle_play(),
        ("Cue", Some(d)) if pressed => mixer.decks[d].cue_button(),
        ("Sync", Some(d)) if pressed => {
            mixer.sync(d);
        }
        // Slider top is +range on many controllers; rekordbox's default range is ±8%.
        ("TempoSlider", Some(d)) => mixer.decks[d].set_pitch_percent((0.5 - unit as f64) * 16.0),
        ("ChannelFader", Some(d)) => mixer.decks[d].gain = unit,
        ("CrossFader", None) => mixer.crossfader = unit * 2.0 - 1.0,
        _ => {}
    }
}

pub fn run(mixer: Mixer, midi: Option<(Mapping, Option<usize>)>) -> Result<()> {
    let mixer = Arc::new(Mutex::new(mixer));
    let output = crate::output::start(Arc::clone(&mixer))?;
    let rate = output.sample_rate;

    let _midi_conn = match midi {
        Some((mapping, port)) => {
            let input = midir::MidiInput::new("innuto")?;
            let ports = input.ports();
            let port = match port {
                Some(i) => ports.get(i).cloned().with_context(|| format!("no MIDI input {i}"))?,
                None => ports.first().cloned().context("no MIDI input devices found")?,
            };
            println!("MIDI: {} via {}", input.port_name(&port)?, mapping.device.as_deref().unwrap_or("mapping"));
            let midi_mixer = Arc::clone(&mixer);
            Some(
                input
                    .connect(&port, "innuto-in", move |_, msg, _| {
                        if let Ok(mut m) = midi_mixer.lock() {
                            apply_midi(&mut m, &mapping, msg);
                        }
                    }, ())
                    .map_err(|e| anyhow!("connecting MIDI input: {e}"))?,
            )
        }
        None => None,
    };

    println!("output {rate} Hz\n{HELP}");
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let mut m = mixer.lock().map_err(|_| anyhow!("mixer lock poisoned"))?;
        match apply_command(&mut m, &line) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => eprintln!("{e}"),
        }
    }
    Ok(())
}

pub fn list_midi_inputs() -> Result<Vec<String>> {
    let input = midir::MidiInput::new("innuto")?;
    input.ports().iter().map(|p| input.port_name(p).map_err(Into::into)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Track;

    fn loaded() -> Mixer {
        let mut m = Mixer::new();
        for bpm in [128.0, 125.0] {
            let i = if bpm == 128.0 { 0 } else { 1 };
            m.decks[i].load(Track {
                sample_rate: 1000,
                samples: Arc::new(vec![0.0; 120_000]),
                bpm,
                first_beat_ms: 0.0,
            });
        }
        m
    }

    #[test]
    fn console_commands_drive_the_mixer() {
        let mut m = loaded();
        assert!(apply_command(&mut m, "p a").unwrap());
        assert!(m.decks[0].playing);
        apply_command(&mut m, "t b 2").unwrap();
        assert!((m.decks[1].pitch_percent() - 2.0).abs() < 1e-9);
        apply_command(&mut m, "s b").unwrap();
        assert!((m.decks[1].current_bpm().unwrap() - 128.0).abs() < 1e-9);
        apply_command(&mut m, "x 5").unwrap();
        assert_eq!(m.crossfader, 1.0);
        assert!(apply_command(&mut m, "z").is_err());
        assert!(!apply_command(&mut m, "q").unwrap());
    }

    #[test]
    fn midi_mapping_drives_the_mixer() {
        let map = Mapping::parse(
            "PlayPause,PlayPause,Button,900B,0,1\nCrossFader,CrossFader,KnobSliderHiRes,B61F,,",
        )
        .unwrap();
        let mut m = loaded();
        apply_midi(&mut m, &map, &[0x91, 0x0B, 0x7F]);
        apply_midi(&mut m, &map, &[0x91, 0x0B, 0x00]);
        assert!(!m.decks[0].playing && m.decks[1].playing);
        apply_midi(&mut m, &map, &[0xB6, 0x1F, 0]);
        assert_eq!(m.crossfader, -1.0);
    }
}
