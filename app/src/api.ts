// Typed wrappers over the Tauri commands in src-tauri/src/main.rs.
import { invoke } from "@tauri-apps/api/core";

export type DeckId = 0 | 1;

export interface TrackDto {
  id: number;
  path: string;
  title: string | null;
  artist: string | null;
  duration_ms: number | null;
  bpm: number | null;
  first_beat_ms: number | null;
}

export interface PlaylistDto {
  id: number;
  parent_id: number | null;
  name: string;
  is_folder: boolean;
}

export interface CueDto {
  slot: number | null;
  position_ms: number;
  label: string;
}

export interface DeckLoaded {
  deck: DeckId;
  track: TrackDto;
  duration_s: number;
  bpm: number;
  first_beat_ms: number;
  cues: CueDto[];
  waveform_columns_per_second: number;
}

export interface DeckState {
  track_id: number | null;
  position_s: number;
  duration_s: number;
  playing: boolean;
  rate: number;
  pitch_pct: number;
  bpm: number | null;
  cue_s: number;
  gain: number;
  eq: [number, number, number];
  filter: number;
  peak: [number, number];
  sync: boolean;
  sync_lost: boolean;
}

export interface MixerState {
  decks: [DeckState, DeckState];
  crossfader: number;
  master_peak: [number, number];
  master_bpm: number | null;
  cpu: number;
  recording: string | null;
}

export interface JobEvent {
  kind: "import" | "analyze" | "analyze-done" | "error";
  done: number;
  total: number;
  message: string;
}

export interface AudioStatus {
  sample_rate: number | null;
  error: string | null;
  library_path: string;
}

export const api = {
  audioStatus: () => invoke<AudioStatus>("audio_status"),
  tracks: (filter: string, playlist: number | null) => invoke<TrackDto[]>("tracks", { filter, playlist }),
  playlists: () => invoke<PlaylistDto[]>("playlists"),
  createPlaylist: (name: string, parent: number | null, folder: boolean) =>
    invoke<number>("create_playlist", { name, parent, folder }),
  addToPlaylist: (playlist: number, trackIds: number[]) => invoke<void>("add_to_playlist", { playlist, trackIds }),
  history: () => invoke<TrackDto[]>("history"),
  importFolder: (path: string) => invoke<void>("import_folder", { path }),
  analyzeTracks: (ids: number[]) => invoke<void>("analyze_tracks", { ids }),
  loadDeck: (deck: DeckId, trackId: number) => invoke<DeckLoaded>("load_deck", { deck, trackId }),
  loadedDeck: (deck: DeckId) => invoke<DeckLoaded | null>("loaded_deck", { deck }),
  deckWaveform: (deck: DeckId) => invoke<ArrayBuffer>("deck_waveform", { deck }),
  play: (deck: DeckId) => invoke<void>("deck_play", { deck }),
  cue: (deck: DeckId, down: boolean) => invoke<void>("deck_cue", { deck, down }),
  sync: (deck: DeckId, engaged: boolean) => invoke<boolean>("deck_sync", { deck, engaged }),
  pitch: (deck: DeckId, pct: number) => invoke<void>("deck_pitch", { deck, pct }),
  seek: (deck: DeckId, seconds: number) => invoke<void>("deck_seek", { deck, seconds }),
  gain: (deck: DeckId, value: number) => invoke<void>("deck_gain", { deck, value }),
  eq: (deck: DeckId, band: number, gain: number) => invoke<void>("deck_eq", { deck, band, gain }),
  filter: (deck: DeckId, value: number) => invoke<void>("deck_filter", { deck, value }),
  crossfader: (value: number) => invoke<void>("set_crossfader", { value }),
  hotCue: (deck: DeckId, slot: number, del: boolean) => invoke<CueDto[]>("hot_cue", { deck, slot, delete: del }),
  toggleRecording: () => invoke<string | null>("toggle_recording"),
  exportPlaylistAnalysis: (playlist: number, folder: string) =>
    invoke<number>("export_playlist_analysis", { playlist, folder }),
};

export const DECK_LETTER = ["A", "B"] as const;
export const DECK_COLOR_VAR = ["--deck-a", "--deck-b"] as const;

export function trackLabel(t: { title: string | null; path: string }): string {
  return t.title ?? t.path.split(/[\\/]/).pop() ?? t.path;
}

export function fmtTime(seconds: number, signed = false): string {
  const s = Math.max(0, seconds);
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${signed ? "-" : ""}${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`;
}

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
