use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};

use innuto::library::{self, Cue, Library};
use innuto::{analysis, anlz, decode, engine, midimap, player};

#[derive(Parser)]
#[command(name = "innuto", version, about = "DJ library manager, track analyzer, and two-deck player")]
struct Cli {
    /// Library database (default: the platform data directory).
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add every audio file under a directory to the collection.
    Import {
        dir: PathBuf,
        /// Analyze new tracks immediately.
        #[arg(long)]
        analyze: bool,
    },
    /// Detect tempo, beat grid, and waveform for tracks.
    Analyze {
        /// Track ids; omit to analyze every unanalyzed track.
        ids: Vec<i64>,
        /// Re-analyze tracks that already have results.
        #[arg(long)]
        force: bool,
    },
    /// List tracks, optionally filtered by title, artist, or path.
    List { filter: Option<String> },
    /// Show one track with its waveform and cues.
    Show { id: i64 },
    /// Set a memory cue, or a hot cue with --hot.
    Cue {
        id: i64,
        position_ms: f64,
        #[arg(long)]
        hot: Option<u8>,
        #[arg(long, default_value = "")]
        label: String,
    },
    /// Manage playlists and folders.
    #[command(subcommand)]
    Playlist(PlaylistCommand),
    /// Print the sections of an ANLZ analysis file.
    Anlz { file: PathBuf },
    /// Write a track's analysis as an ANLZ .DAT file.
    ExportAnlz { id: i64, out: PathBuf },
    /// Adopt beat grids from a rekordbox USBANLZ directory for matching tracks.
    ImportAnlz { dir: PathBuf },
    /// Print the bindings of a rekordbox-layout controller mapping CSV.
    Midimap { csv: PathBuf },
    /// List MIDI input devices.
    MidiInputs,
    /// Load tracks onto decks A and B and play through the default output.
    Play {
        a: i64,
        b: Option<i64>,
        /// Controller mapping CSV to drive the decks from a MIDI device.
        #[arg(long)]
        midi_map: Option<PathBuf>,
        /// MIDI input index from `midi-inputs` (default: first).
        #[arg(long)]
        midi_port: Option<usize>,
    },
}

#[derive(Subcommand)]
enum PlaylistCommand {
    New {
        name: String,
        #[arg(long)]
        parent: Option<i64>,
        #[arg(long)]
        folder: bool,
    },
    Add { playlist: i64, tracks: Vec<i64> },
    List,
    Show { id: i64 },
}

fn fmt_duration(ms: Option<i64>) -> String {
    ms.map_or("--:--".into(), |ms| format!("{}:{:02}", ms / 60_000, ms / 1000 % 60))
}

fn waveform_text(preview: &[u8], width: usize) -> String {
    const BARS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if preview.is_empty() {
        return String::new();
    }
    (0..width)
        .map(|c| {
            let lo = c * preview.len() / width;
            let hi = ((c + 1) * preview.len() / width).max(lo + 1);
            let h = preview[lo..hi].iter().map(|b| b & 0x1f).max().unwrap_or(0);
            BARS[(h as usize * 8 + 15) / 31]
        })
        .collect()
}

fn analyze_track(lib: &Library, id: i64, path: &str) -> Result<analysis::Analysis> {
    let audio = decode::decode_file(Path::new(path))?;
    let a = analysis::analyze(&audio.mono(), audio.sample_rate);
    lib.save_analysis(id, &a, (audio.duration_secs() * 1000.0) as i64, audio.sample_rate)?;
    Ok(a)
}

fn load_deck(lib: &Library, id: i64) -> Result<engine::Track> {
    let row = lib.track(id)?;
    let audio = decode::decode_file(Path::new(&row.path))?;
    let (bpm, first_beat_ms) = match (row.bpm, row.first_beat_ms) {
        (Some(b), Some(f)) => (b, f),
        _ => {
            let a = analysis::analyze(&audio.mono(), audio.sample_rate);
            lib.save_analysis(id, &a, (audio.duration_secs() * 1000.0) as i64, audio.sample_rate)?;
            (a.bpm, a.first_beat_ms)
        }
    };
    println!("loaded {} ({bpm:.2} BPM)", row.title.as_deref().unwrap_or(&row.path));
    Ok(engine::Track { sample_rate: audio.sample_rate, samples: Arc::new(audio.samples), bpm, first_beat_ms })
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let open = || -> Result<Library> { Library::open(&cli.db.clone().map_or_else(library::default_path, Ok)?) };

    match cli.command {
        Command::Import { ref dir, analyze } => {
            let lib = open()?;
            let mut count = 0;
            for entry in walkdir::WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
                let path = entry.path();
                if !entry.file_type().is_file() || !decode::is_audio_file(path) {
                    continue;
                }
                let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                let path_str = canonical.to_string_lossy().trim_start_matches(r"\\?\").to_string();
                let tags = decode::read_tags(path).unwrap_or_default();
                let title = tags.title.or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()));
                let id = lib.add_track(&path_str, title.as_deref(), tags.artist.as_deref())?;
                count += 1;
                if analyze {
                    match analyze_track(&lib, id, &path_str) {
                        Ok(a) => println!("{id:>5}  {:>6.2} BPM  {path_str}", a.bpm),
                        Err(e) => eprintln!("{id:>5}  analysis failed: {path_str}: {e:#}"),
                    }
                }
            }
            println!("imported {count} audio files");
        }
        Command::Analyze { ref ids, force } => {
            let lib = open()?;
            let targets: Vec<_> = if ids.is_empty() {
                lib.tracks("")?.into_iter().filter(|t| force || t.bpm.is_none()).collect()
            } else {
                ids.iter().map(|&id| lib.track(id)).collect::<Result<_>>()?
            };
            for t in targets {
                match analyze_track(&lib, t.id, &t.path) {
                    Ok(a) => println!("{:>5}  {:>6.2} BPM  first beat {:>7.1} ms  {}", t.id, a.bpm, a.first_beat_ms, t.path),
                    Err(e) => eprintln!("{:>5}  failed: {e:#}", t.id),
                }
            }
        }
        Command::List { ref filter } => {
            for t in open()?.tracks(filter.as_deref().unwrap_or(""))? {
                println!(
                    "{:>5}  {:>6}  {:>5}  {} — {}",
                    t.id,
                    t.bpm.map_or("-".into(), |b| format!("{b:.2}")),
                    fmt_duration(t.duration_ms),
                    t.artist.as_deref().unwrap_or("?"),
                    t.title.as_deref().unwrap_or(&t.path),
                );
            }
        }
        Command::Show { id } => {
            let lib = open()?;
            let t = lib.track(id)?;
            println!("{} — {}", t.artist.as_deref().unwrap_or("?"), t.title.as_deref().unwrap_or("?"));
            println!("path      {}", t.path);
            println!("duration  {}", fmt_duration(t.duration_ms));
            match (t.bpm, t.first_beat_ms) {
                (Some(b), Some(f)) => println!("tempo     {b:.2} BPM, first beat {f:.1} ms"),
                _ => println!("tempo     not analyzed"),
            }
            if let Some(p) = &t.preview {
                println!("[{}]", waveform_text(p, 100));
            }
            for c in lib.cues(id)? {
                let kind = c.hot_slot.map_or("memory".to_string(), |s| format!("hot {}", (b'A' + s) as char));
                println!("cue {kind:<7} {:>10.1} ms  {}", c.position_ms, c.label);
            }
        }
        Command::Cue { id, position_ms, hot, ref label } => {
            let lib = open()?;
            lib.track(id)?;
            lib.set_cue(id, &Cue { hot_slot: hot, position_ms, label: label.clone() })?;
        }
        Command::Playlist(ref cmd) => {
            let lib = open()?;
            match cmd {
                PlaylistCommand::New { name, parent, folder } => {
                    println!("{}", lib.create_playlist(name, *parent, *folder)?);
                }
                PlaylistCommand::Add { playlist, tracks } => {
                    for &t in tracks {
                        lib.add_to_playlist(*playlist, t)?;
                    }
                }
                PlaylistCommand::List => {
                    let all = lib.playlists()?;
                    fn walk(all: &[library::PlaylistRow], parent: Option<i64>, depth: usize) {
                        for p in all.iter().filter(|p| p.parent_id == parent) {
                            let icon = if p.is_folder { "▸" } else { "♪" };
                            println!("{:>5}  {}{icon} {}", p.id, "  ".repeat(depth), p.name);
                            walk(all, Some(p.id), depth + 1);
                        }
                    }
                    walk(&all, None, 0);
                }
                PlaylistCommand::Show { id } => {
                    for (i, t) in lib.playlist_tracks(*id)?.iter().enumerate() {
                        println!("{:>3}. [{}] {}", i + 1, t.id, t.title.as_deref().unwrap_or(&t.path));
                    }
                }
            }
        }
        Command::Anlz { ref file } => {
            let bytes = std::fs::read(file)?;
            for s in anlz::parse(&bytes)? {
                match s {
                    anlz::Section::Path(p) => println!("PPTH  {p}"),
                    anlz::Section::BeatGrid(b) => {
                        let first = b.first();
                        println!(
                            "PQTZ  {} beats, first at {} ms, {:.2} BPM",
                            b.len(),
                            first.map_or(0, |b| b.time_ms),
                            first.map_or(0.0, |b| b.tempo_centi_bpm as f64 / 100.0)
                        );
                    }
                    anlz::Section::WaveformPreview(w) => {
                        println!("PWAV  {} columns\n[{}]", w.len(), waveform_text(&w, 100))
                    }
                    anlz::Section::Other { tag, header_len, len } => {
                        println!("{tag}  {len} bytes (header {header_len}), not decoded")
                    }
                }
            }
        }
        Command::ExportAnlz { id, ref out } => {
            let lib = open()?;
            let t = lib.track(id)?;
            let (Some(bpm), Some(first), Some(preview)) = (t.bpm, t.first_beat_ms, t.preview.as_ref()) else {
                anyhow::bail!("track {id} is not analyzed; run `innuto analyze {id}`");
            };
            let duration = t.duration_ms.unwrap_or(0) as f64;
            let beats = analysis::beat_grid(bpm, first, duration);
            std::fs::write(out, anlz::write_dat(&t.path, bpm, &beats, preview))?;
            println!("wrote {} ({} beats)", out.display(), beats.len());
        }
        Command::ImportAnlz { ref dir } => {
            let lib = open()?;
            let by_name: HashMap<String, i64> = lib
                .tracks("")?
                .into_iter()
                .filter_map(|t| {
                    let name = Path::new(&t.path).file_name()?.to_string_lossy().to_lowercase();
                    Some((name, t.id))
                })
                .collect();
            let (mut matched, mut seen) = (0, 0);
            for entry in walkdir::WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
                if entry.path().extension().and_then(|e| e.to_str()) != Some("DAT") {
                    continue;
                }
                seen += 1;
                let Ok(sections) = std::fs::read(entry.path()).map_err(anyhow::Error::from).and_then(|b| anlz::parse(&b))
                else {
                    continue;
                };
                let path = sections.iter().find_map(|s| match s {
                    anlz::Section::Path(p) => Some(p.clone()),
                    _ => None,
                });
                let grid = sections.iter().find_map(|s| match s {
                    anlz::Section::BeatGrid(b) => b.first().copied(),
                    _ => None,
                });
                let (Some(path), Some(first)) = (path, grid) else { continue };
                let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_lowercase();
                if let Some(&id) = by_name.get(&name) {
                    lib.set_beat_grid(id, first.tempo_centi_bpm as f64 / 100.0, first.time_ms as f64)?;
                    matched += 1;
                }
            }
            println!("{seen} analysis files, {matched} beat grids applied by file name");
        }
        Command::Midimap { ref csv } => {
            let m = midimap::Mapping::parse(&std::fs::read_to_string(csv)?)?;
            println!("device {}", m.device.as_deref().unwrap_or("?"));
            for b in &m.bindings {
                let deck = b.deck.map_or("-".into(), |d| (d + 1).to_string());
                println!("{:02X} {:02X}  deck {deck}  {:<16} {}", b.status, b.data1, b.control_type, b.function);
            }
        }
        Command::MidiInputs => {
            for (i, name) in player::list_midi_inputs()?.iter().enumerate() {
                println!("{i}: {name}");
            }
        }
        Command::Play { a, b, ref midi_map, midi_port } => {
            let lib = open()?;
            let mut mixer = engine::Mixer::new();
            mixer.decks[0].load(load_deck(&lib, a)?);
            if let Some(b) = b {
                mixer.decks[1].load(load_deck(&lib, b)?);
            }
            let midi = match midi_map {
                Some(p) => Some((midimap::Mapping::parse(&std::fs::read_to_string(p)?)?, midi_port)),
                None => None,
            };
            player::run(mixer, midi)?;
        }
    }
    Ok(())
}
