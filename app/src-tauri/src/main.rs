#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tauri shell: owns the library and the audio engine; the web UI only sends
//! commands and renders the state events this process emits.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use innuto::analysis;
use innuto::anlz;
use innuto::decode;
use innuto::engine::{Mixer, Track};
use innuto::library::{self, Cue, Library, PlaylistRow, TrackRow};
use innuto::output::{self, Output};
use serde::Serialize;
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, Manager, State};

/// Fixed rate of `mixer-state` events; the UI interpolates between them.
const STATE_HZ: u64 = 30;

struct LoadedDeck {
    track_id: i64,
    waveform: Arc<Vec<u8>>,
}

struct App {
    db_path: PathBuf,
    lib: Mutex<Library>,
    mixer: Arc<Mutex<Mixer>>,
    output: Result<Output, String>,
    loaded: Mutex<[Option<LoadedDeck>; 2]>,
    history: Mutex<Vec<i64>>,
}

type Cmd<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn lock<T>(m: &Mutex<T>) -> Cmd<MutexGuard<'_, T>> {
    m.lock().map_err(|_| "internal lock poisoned".to_string())
}

fn deck_index(deck: usize) -> Cmd<usize> {
    if deck < 2 {
        Ok(deck)
    } else {
        Err(format!("no deck {deck}"))
    }
}

#[derive(Serialize, Clone)]
struct TrackDto {
    id: i64,
    path: String,
    title: Option<String>,
    artist: Option<String>,
    duration_ms: Option<i64>,
    bpm: Option<f64>,
    first_beat_ms: Option<f64>,
}

impl From<TrackRow> for TrackDto {
    fn from(t: TrackRow) -> Self {
        TrackDto {
            id: t.id,
            path: t.path,
            title: t.title,
            artist: t.artist,
            duration_ms: t.duration_ms,
            bpm: t.bpm,
            first_beat_ms: t.first_beat_ms,
        }
    }
}

#[derive(Serialize)]
struct PlaylistDto {
    id: i64,
    parent_id: Option<i64>,
    name: String,
    is_folder: bool,
}

impl From<PlaylistRow> for PlaylistDto {
    fn from(p: PlaylistRow) -> Self {
        PlaylistDto { id: p.id, parent_id: p.parent_id, name: p.name, is_folder: p.is_folder }
    }
}

#[derive(Serialize)]
struct CueDto {
    slot: Option<u8>,
    position_ms: f64,
    label: String,
}

impl From<Cue> for CueDto {
    fn from(c: Cue) -> Self {
        CueDto { slot: c.hot_slot, position_ms: c.position_ms, label: c.label }
    }
}

#[derive(Serialize)]
struct DeckLoaded {
    deck: usize,
    track: TrackDto,
    duration_s: f64,
    bpm: f64,
    first_beat_ms: f64,
    cues: Vec<CueDto>,
    waveform_columns_per_second: u32,
}

#[derive(Serialize, Clone)]
struct DeckState {
    track_id: Option<i64>,
    position_s: f64,
    duration_s: f64,
    playing: bool,
    rate: f64,
    pitch_pct: f64,
    bpm: Option<f64>,
    cue_s: f64,
    gain: f32,
    eq: [f32; 3],
    filter: f32,
    peak: [f32; 2],
    sync: bool,
    sync_lost: bool,
}

#[derive(Serialize, Clone)]
struct MixerState {
    decks: [DeckState; 2],
    crossfader: f32,
    master_peak: [f32; 2],
    master_bpm: Option<f64>,
    cpu: f32,
    recording: Option<String>,
}

#[derive(Serialize, Clone)]
struct JobEvent {
    kind: &'static str,
    done: usize,
    total: usize,
    message: String,
}

#[derive(Serialize)]
struct AudioStatus {
    sample_rate: Option<u32>,
    error: Option<String>,
    library_path: String,
}

#[tauri::command]
fn audio_status(st: State<App>) -> AudioStatus {
    AudioStatus {
        sample_rate: st.output.as_ref().ok().map(|o| o.sample_rate),
        error: st.output.as_ref().err().cloned(),
        library_path: st.db_path.display().to_string(),
    }
}

#[tauri::command]
fn tracks(st: State<App>, filter: String, playlist: Option<i64>) -> Cmd<Vec<TrackDto>> {
    let lib = lock(&st.lib)?;
    let rows = match playlist {
        Some(p) => {
            let f = filter.to_lowercase();
            lib.playlist_tracks(p)
                .map_err(err)?
                .into_iter()
                .filter(|t| {
                    f.is_empty()
                        || [&t.title, &t.artist].iter().any(|s| s.as_deref().is_some_and(|s| s.to_lowercase().contains(&f)))
                        || t.path.to_lowercase().contains(&f)
                })
                .collect()
        }
        None => lib.tracks(&filter).map_err(err)?,
    };
    Ok(rows.into_iter().map(Into::into).collect())
}

#[tauri::command]
fn playlists(st: State<App>) -> Cmd<Vec<PlaylistDto>> {
    Ok(lock(&st.lib)?.playlists().map_err(err)?.into_iter().map(Into::into).collect())
}

#[tauri::command]
fn create_playlist(st: State<App>, name: String, parent: Option<i64>, folder: bool) -> Cmd<i64> {
    lock(&st.lib)?.create_playlist(&name, parent, folder).map_err(err)
}

#[tauri::command]
fn add_to_playlist(st: State<App>, playlist: i64, track_ids: Vec<i64>) -> Cmd<()> {
    let lib = lock(&st.lib)?;
    for id in track_ids {
        lib.add_to_playlist(playlist, id).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
fn history(st: State<App>) -> Cmd<Vec<TrackDto>> {
    let ids = lock(&st.history)?.clone();
    let lib = lock(&st.lib)?;
    Ok(ids.iter().filter_map(|&id| lib.track(id).ok()).map(Into::into).collect())
}

fn emit_job(app: &AppHandle, kind: &'static str, done: usize, total: usize, message: String) {
    let _ = app.emit("job", JobEvent { kind, done, total, message });
}

/// Analyzes tracks on a separate connection so the UI's queries never wait.
fn analyze_in_background(app: AppHandle, db_path: PathBuf, ids: Option<Vec<i64>>) {
    thread::spawn(move || {
        let lib = match Library::open(&db_path) {
            Ok(l) => l,
            Err(e) => return emit_job(&app, "error", 0, 0, format!("{e:#}")),
        };
        let targets: Vec<TrackRow> = match ids {
            Some(ids) => ids.into_iter().filter_map(|id| lib.track(id).ok()).collect(),
            None => lib.tracks("").map(|t| t.into_iter().filter(|t| t.bpm.is_none()).collect()).unwrap_or_default(),
        };
        let total = targets.len();
        for (i, t) in targets.into_iter().enumerate() {
            let label = t.title.clone().unwrap_or_else(|| t.path.clone());
            let result = decode::decode_file(Path::new(&t.path)).and_then(|audio| {
                let a = analysis::analyze(&audio.mono(), audio.sample_rate);
                lib.save_analysis(t.id, &a, (audio.duration_secs() * 1000.0) as i64, audio.sample_rate)
            });
            match result {
                Ok(()) => emit_job(&app, "analyze", i + 1, total, label),
                Err(e) => emit_job(&app, "error", i + 1, total, format!("{label}: {e:#}")),
            }
            let _ = app.emit("library-changed", ());
        }
        emit_job(&app, "analyze-done", total, total, String::new());
    });
}

#[tauri::command]
fn import_folder(app: AppHandle, st: State<App>, path: String) -> Cmd<()> {
    let db_path = st.db_path.clone();
    thread::spawn(move || {
        let lib = match Library::open(&db_path) {
            Ok(l) => l,
            Err(e) => return emit_job(&app, "error", 0, 0, format!("{e:#}")),
        };
        let files: Vec<PathBuf> = walkdir(&path);
        let total = files.len();
        for (i, f) in files.iter().enumerate() {
            let tags = decode::read_tags(f).unwrap_or_default();
            let title = tags.title.or_else(|| f.file_stem().map(|s| s.to_string_lossy().into_owned()));
            let canonical = f.canonicalize().unwrap_or_else(|_| f.clone());
            let p = canonical.to_string_lossy().trim_start_matches(r"\\?\").to_string();
            if let Err(e) = lib.add_track(&p, title.as_deref(), tags.artist.as_deref()) {
                emit_job(&app, "error", i + 1, total, format!("{p}: {e:#}"));
            } else if (i + 1) % 25 == 0 || i + 1 == total {
                emit_job(&app, "import", i + 1, total, String::new());
            }
        }
        let _ = app.emit("library-changed", ());
        analyze_in_background(app, db_path, None);
    });
    Ok(())
}

/// Audio files under `root`, or `root` itself when it is an audio file.
fn walkdir(root: &str) -> Vec<PathBuf> {
    let root_path = Path::new(root);
    if root_path.is_file() {
        return if decode::is_audio_file(root_path) { vec![root_path.to_path_buf()] } else { Vec::new() };
    }
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() && decode::is_audio_file(&p) => out.push(p),
                _ => {}
            }
        }
    }
    out.sort();
    out
}

#[tauri::command]
fn analyze_tracks(app: AppHandle, st: State<App>, ids: Vec<i64>) {
    analyze_in_background(app, st.db_path.clone(), Some(ids));
}

fn load_deck_blocking(app: &AppHandle, deck: usize, track_id: i64) -> Cmd<DeckLoaded> {
    let st = app.state::<App>();
    let row = lock(&st.lib)?.track(track_id).map_err(err)?;
    let audio = decode::decode_file(Path::new(&row.path)).map_err(|e| format!("{e:#}"))?;
    let mono = audio.mono();
    let (bpm, first_beat_ms) = match (row.bpm, row.first_beat_ms) {
        (Some(b), Some(f)) => (b, f),
        _ => {
            let a = analysis::analyze(&mono, audio.sample_rate);
            lock(&st.lib)?
                .save_analysis(track_id, &a, (audio.duration_secs() * 1000.0) as i64, audio.sample_rate)
                .map_err(err)?;
            (a.bpm, a.first_beat_ms)
        }
    };
    let waveform = Arc::new(analysis::band_waveform(&mono, audio.sample_rate));
    let cues = lock(&st.lib)?.cues(track_id).map_err(err)?;
    let duration_s = audio.duration_secs();
    let sample_rate = audio.sample_rate;
    let track = Track { sample_rate, samples: Arc::new(audio.samples), bpm, first_beat_ms };
    {
        let mut m = lock(&st.mixer)?;
        let d = &mut m.decks[deck];
        d.load(track);
        if let Some(memory) = cues.iter().find(|c| c.hot_slot.is_none()) {
            d.cue = memory.position_ms / 1000.0 * sample_rate as f64;
            d.position = d.cue;
        }
    }
    lock(&st.loaded)?[deck] = Some(LoadedDeck { track_id, waveform });
    let row = lock(&st.lib)?.track(track_id).map_err(err)?;
    Ok(DeckLoaded {
        deck,
        track: row.into(),
        duration_s,
        bpm,
        first_beat_ms,
        cues: cues.into_iter().map(Into::into).collect(),
        waveform_columns_per_second: analysis::BAND_COLUMNS_PER_SECOND,
    })
}

#[tauri::command]
async fn load_deck(app: AppHandle, deck: usize, track_id: i64) -> Cmd<DeckLoaded> {
    let deck = deck_index(deck)?;
    tauri::async_runtime::spawn_blocking(move || load_deck_blocking(&app, deck, track_id))
        .await
        .map_err(err)?
}

/// What the engine holds on a deck, so a reloaded UI can rebuild its view
/// without decoding again.
#[tauri::command]
fn loaded_deck(st: State<App>, deck: usize) -> Cmd<Option<DeckLoaded>> {
    let deck = deck_index(deck)?;
    let Some(track_id) = lock(&st.loaded)?[deck].as_ref().map(|l| l.track_id) else {
        return Ok(None);
    };
    let (duration_s, bpm, first_beat_ms) = {
        let m = lock(&st.mixer)?;
        let d = &m.decks[deck];
        let Some(t) = d.track.as_ref() else { return Ok(None) };
        (d.duration_seconds(), t.bpm, t.first_beat_ms)
    };
    let lib = lock(&st.lib)?;
    Ok(Some(DeckLoaded {
        deck,
        track: lib.track(track_id).map_err(err)?.into(),
        duration_s,
        bpm,
        first_beat_ms,
        cues: lib.cues(track_id).map_err(err)?.into_iter().map(Into::into).collect(),
        waveform_columns_per_second: analysis::BAND_COLUMNS_PER_SECOND,
    }))
}

/// Detailed three-band waveform of the loaded track, as raw bytes.
#[tauri::command]
fn deck_waveform(st: State<App>, deck: usize) -> Cmd<Response> {
    let deck = deck_index(deck)?;
    let loaded = lock(&st.loaded)?;
    let w = loaded[deck].as_ref().ok_or("deck is empty")?;
    Ok(Response::new(w.waveform.as_ref().clone()))
}

fn with_mixer<T>(st: &State<App>, f: impl FnOnce(&mut Mixer) -> T) -> Cmd<T> {
    Ok(f(&mut *lock(&st.mixer)?))
}

#[tauri::command]
fn deck_play(st: State<App>, deck: usize) -> Cmd<()> {
    let deck = deck_index(deck)?;
    with_mixer(&st, |m| m.decks[deck].toggle_play())
}

#[tauri::command]
fn deck_cue(st: State<App>, deck: usize, down: bool) -> Cmd<()> {
    let deck = deck_index(deck)?;
    with_mixer(&st, |m| if down { m.decks[deck].cue_down() } else { m.decks[deck].cue_up() })
}

#[tauri::command]
fn deck_sync(st: State<App>, deck: usize, engaged: bool) -> Cmd<bool> {
    let deck = deck_index(deck)?;
    with_mixer(&st, |m| m.set_sync(deck, engaged))
}

#[tauri::command]
fn deck_pitch(st: State<App>, deck: usize, pct: f64) -> Cmd<()> {
    let deck = deck_index(deck)?;
    with_mixer(&st, |m| m.decks[deck].set_pitch_percent(pct))
}

#[tauri::command]
fn deck_seek(st: State<App>, deck: usize, seconds: f64) -> Cmd<()> {
    let deck = deck_index(deck)?;
    with_mixer(&st, |m| m.decks[deck].seek_seconds(seconds))
}

#[tauri::command]
fn deck_gain(st: State<App>, deck: usize, value: f32) -> Cmd<()> {
    let deck = deck_index(deck)?;
    with_mixer(&st, |m| m.decks[deck].gain = value.clamp(0.0, 1.0))
}

#[tauri::command]
fn deck_eq(st: State<App>, deck: usize, band: usize, gain: f32) -> Cmd<()> {
    let deck = deck_index(deck)?;
    if band > 2 {
        return Err(format!("no EQ band {band}"));
    }
    with_mixer(&st, |m| m.decks[deck].eq[band] = gain.clamp(0.0, 2.0))
}

#[tauri::command]
fn deck_filter(st: State<App>, deck: usize, value: f32) -> Cmd<()> {
    let deck = deck_index(deck)?;
    with_mixer(&st, |m| m.decks[deck].filter = value.clamp(-1.0, 1.0))
}

#[tauri::command]
fn set_crossfader(st: State<App>, value: f32) -> Cmd<()> {
    with_mixer(&st, |m| m.crossfader = value.clamp(-1.0, 1.0))
}

/// Jumps to a hot cue, or stores one at the nearest beat when the slot is empty.
#[tauri::command]
fn hot_cue(st: State<App>, deck: usize, slot: u8, delete: bool) -> Cmd<Vec<CueDto>> {
    let deck = deck_index(deck)?;
    if slot > 7 {
        return Err(format!("no hot cue slot {slot}"));
    }
    let track_id = lock(&st.loaded)?[deck].as_ref().map(|l| l.track_id).ok_or("deck is empty")?;
    let lib = lock(&st.lib)?;
    if delete {
        lib.delete_hot_cue(track_id, slot).map_err(err)?;
    } else {
        let existing = lib.cues(track_id).map_err(err)?.into_iter().find(|c| c.hot_slot == Some(slot));
        let mut m = lock(&st.mixer)?;
        let d = &mut m.decks[deck];
        match existing {
            Some(c) => d.seek_seconds(c.position_ms / 1000.0),
            None => {
                let t = d.track.as_ref().ok_or("deck is empty")?;
                let beat = 60.0 / t.bpm;
                let first = t.first_beat_ms / 1000.0;
                let snapped = first + ((d.seconds() - first) / beat).round() * beat;
                let position_ms = snapped.max(0.0) * 1000.0;
                lib.set_cue(track_id, &Cue { hot_slot: Some(slot), position_ms, label: String::new() })
                    .map_err(err)?;
            }
        }
    }
    Ok(lib.cues(track_id).map_err(err)?.into_iter().map(Into::into).collect())
}

/// Starts or stops recording the master output; returns the file being written.
#[tauri::command]
fn toggle_recording(st: State<App>) -> Cmd<Option<String>> {
    let out = st.output.as_ref().map_err(Clone::clone)?;
    if out.recording_path().is_some() {
        out.stop_recording().map_err(|e| format!("{e:#}"))?;
        return Ok(None);
    }
    let dir = dirs_music().join("innuto");
    std::fs::create_dir_all(&dir).map_err(err)?;
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_err(err)?.as_secs();
    let path = dir.join(format!("innuto-session-{secs}.wav"));
    out.start_recording(&path).map_err(|e| format!("{e:#}"))?;
    Ok(Some(path.display().to_string()))
}

fn dirs_music() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join("Music"))
        .unwrap_or_else(std::env::temp_dir)
}

/// Writes an ANLZ `.DAT` per analyzed track of a playlist, one folder per track id.
#[tauri::command]
fn export_playlist_analysis(st: State<App>, playlist: i64, folder: String) -> Cmd<usize> {
    let lib = lock(&st.lib)?;
    let mut written = 0;
    for t in lib.playlist_tracks(playlist).map_err(err)? {
        let (Some(bpm), Some(first), Some(preview)) = (t.bpm, t.first_beat_ms, t.preview.as_ref()) else {
            continue;
        };
        let beats = analysis::beat_grid(bpm, first, t.duration_ms.unwrap_or(0) as f64);
        let dir = Path::new(&folder).join(t.id.to_string());
        std::fs::create_dir_all(&dir).map_err(err)?;
        std::fs::write(dir.join("ANLZ0000.DAT"), anlz::write_dat(&t.path, bpm, &beats, preview)).map_err(err)?;
        written += 1;
    }
    Ok(written)
}

fn snapshot(st: &App) -> Option<MixerState> {
    let m = st.mixer.lock().ok()?;
    let loaded = st.loaded.lock().ok()?;
    let deck_state = |i: usize| {
        let d = &m.decks[i];
        DeckState {
            track_id: loaded[i].as_ref().map(|l| l.track_id),
            position_s: d.seconds(),
            duration_s: d.duration_seconds(),
            playing: d.playing,
            rate: d.rate,
            pitch_pct: d.pitch_percent(),
            bpm: d.current_bpm(),
            cue_s: d.cue_seconds(),
            gain: d.gain,
            eq: d.eq,
            filter: d.filter,
            peak: d.peak,
            sync: d.sync_engaged,
            sync_lost: m.sync_lost(i),
        }
    };
    let decks = [deck_state(0), deck_state(1)];
    // The tempo everyone follows: a synced deck's master, else the playing deck.
    let master = (0..2)
        .find(|&i| m.decks[1 - i].sync_engaged)
        .or_else(|| (0..2).find(|&i| m.decks[i].playing))
        .and_then(|i| m.decks[i].current_bpm());
    let output = st.output.as_ref().ok();
    Some(MixerState {
        decks,
        crossfader: m.crossfader,
        master_peak: m.master_peak,
        master_bpm: master,
        cpu: output.map_or(0.0, |o| o.load()),
        recording: output.and_then(|o| o.recording_path()).map(|p| p.display().to_string()),
    })
}

fn start_state_events(app: AppHandle) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(1000 / STATE_HZ));
        let st = app.state::<App>();
        let Some(state) = snapshot(&st) else { continue };
        for d in &state.decks {
            if let (true, Some(id)) = (d.playing, d.track_id) {
                if let Ok(mut h) = st.history.lock() {
                    if h.last() != Some(&id) && !h.contains(&id) {
                        h.push(id);
                        let _ = app.emit("history-changed", ());
                    }
                }
            }
        }
        let _ = app.emit("mixer-state", state);
    });
}

fn main() {
    // INNUTO_DB points the app at another library, e.g. for testing.
    let db_path = std::env::var_os("INNUTO_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| library::default_path().expect("no data directory for the library"));
    let lib = Library::open(&db_path).expect("cannot open the innuto library");
    let mixer = Arc::new(Mutex::new(Mixer::new()));
    let output = output::start(Arc::clone(&mixer)).map_err(|e| format!("{e:#}"));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(App {
            db_path,
            lib: Mutex::new(lib),
            mixer,
            output,
            loaded: Mutex::new([None, None]),
            history: Mutex::new(Vec::new()),
        })
        .setup(|app| {
            start_state_events(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            audio_status,
            tracks,
            playlists,
            create_playlist,
            add_to_playlist,
            history,
            import_folder,
            analyze_tracks,
            load_deck,
            loaded_deck,
            deck_waveform,
            deck_play,
            deck_cue,
            deck_sync,
            deck_pitch,
            deck_seek,
            deck_gain,
            deck_eq,
            deck_filter,
            set_crossfader,
            hot_cue,
            toggle_recording,
            export_playlist_analysis,
        ])
        .run(tauri::generate_context!())
        .expect("error while running innuto");
}
