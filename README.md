# Innuto

An open DJ library manager, track analyzer and two-deck player, written in Rust.
Its data model was informed by a static study of rekordbox 7.2.18. See
[EVIDENCE.md](EVIDENCE.md) for what was observed, what was inferred, and what
Innuto chose to do differently. No rekordbox code or assets are included.

## Desktop app (Tauri)

The interface follows [app/design/DESIGN.md](app/design/DESIGN.md). Audio runs
in Rust on its own thread, so the UI only sends commands and draws state; if
the UI freezes, the music keeps playing.

```bash
cd app
npm install
npx tauri dev      # development
npx tauri build    # installer in target/release/bundle
```

Set `INNUTO_DB` to use a library other than `%APPDATA%\Innuto\library.db`.

| Area | What works |
|---|---|
| Title bar | Archivo/Ver menus, master BPM, audio engine load, record to WAV in `Music/innuto`, window controls |
| Waveforms | Both decks zoomed and aligned by beat, colored by band (low/mid/high) or monochrome, beat grid with downbeats, cues |
| Decks | Overview with click-to-seek, time remaining (magenta in the last 30 s), BPM, pitch ±8 %, CUE (hold to preview), PLAY with beat pulse, SYNC with drift warning, 8 hot cues snapped to the beat |
| Mixer | HI/MID/LOW EQ with kill, LPF/HPF filter, channel faders, meters with peak hold, master meter, constant-power crossfader |
| Library | Collection, playlists and folders, session history, search, virtualized list, drag to decks or playlists, drop folders from the system, background analysis |
| Views | Performance (F1), Preparation (F2), Export (F3), booth light, UI scale 90–150 %, `?` shortcut map |

## Build the CLI

```bash
cargo build --release -p innuto
```

## Use

```bash
innuto import "D:/Music" --analyze        # add audio files and detect BPM/grid/waveform
innuto list techno                        # search by title, artist or path
innuto show 12                            # waveform, tempo, cues
innuto cue 12 32500 --hot 0 --label Drop  # hot cue A at 32.5 s
innuto playlist new Gigs --folder
innuto playlist new Friday --parent 1
innuto playlist add 2 12 15 9
innuto playlist list
innuto play 12 15                         # deck A and B; type commands such as `p a`, `s b`, `x 0.3`
innuto play 12 15 --midi-map "C:/Program Files/rekordbox/rekordbox 7.2.18/MidiMappings/DDJ-400.midi.csv"
```

Interop with rekordbox analysis files:

```bash
innuto anlz path/to/ANLZ0000.DAT                       # decode PPTH/PQTZ/PWAV, list other tags
innuto import-anlz "%APPDATA%/Pioneer/rekordbox/share/PIONEER/USBANLZ"   # adopt rekordbox grids by file name
innuto export-anlz 12 out/ANLZ0000.DAT                 # write Innuto's analysis in ANLZ layout
```

The library lives at `%APPDATA%\Innuto\library.db` unless `--db` is given.

## Layout

| Module | Responsibility |
|---|---|
| `decode.rs` | Symphonia decoding to stereo f32, tag reading |
| `analysis.rs` | Coarse tempo autocorrelation, whole-track comb refinement, beat grid, waveform preview |
| `anlz.rs` | PMAI/ANLZ reader and `.DAT` writer |
| `midimap.rs` | rekordbox-layout `*.midi.csv` parser |
| `engine.rs` | Decks (pitch, cue, beat-phase sync) and constant-power crossfader |
| `library.rs` | SQLite collection: tracks, cues, playlist tree |
| `player.rs` | cpal output, console and MIDI control |

## Not yet implemented

Key detection, downbeat detection, FX, loops, recording, stems, lighting,
streaming services, Pro DJ Link/USB export, and a graphical UI.
