// App-wide state. Mixer state arrives from Rust at a fixed rate on its own
// channel, so 30 Hz updates re-render only the components that read it;
// canvases read `positionNow` from requestAnimationFrame instead.
import { useSyncExternalStore } from "react";
import type { CueDto, DeckId, MixerState, TrackDto } from "./api";

export type View = "performance" | "prep" | "export";

export interface Settings {
  view: View;
  mono: boolean;
  booth: boolean;
  scale: number;
  rows: "compact" | "comfortable";
  waveHeight: number;
  navWidth: number;
}

export interface DeckMedia {
  track: TrackDto;
  durationS: number;
  bpm: number;
  firstBeatMs: number;
  cues: CueDto[];
  waveform: Uint8Array | null;
  colsPerSec: number;
}

export interface Toast {
  id: number;
  kind: "info" | "success" | "warning" | "danger";
  text: string;
  audio?: boolean;
}

const SETTINGS_KEY = "innuto.settings";
const defaults: Settings = {
  view: "performance",
  mono: false,
  booth: false,
  scale: 1,
  rows: "compact",
  waveHeight: 150,
  navWidth: 200,
};

function loadSettings(): Settings {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    return raw ? { ...defaults, ...JSON.parse(raw) } : defaults;
  } catch {
    return defaults;
  }
}

type Listener = () => void;

class Channel {
  private listeners = new Set<Listener>();
  version = 0;
  subscribe = (l: Listener) => {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  };
  notify() {
    this.version++;
    this.listeners.forEach((l) => l());
  }
}

export const mixerChannel = new Channel();
export const appChannel = new Channel();

export const store = {
  mixer: null as MixerState | null,
  receivedAt: 0,
  decks: [null, null] as [DeckMedia | null, DeckMedia | null],
  loading: [false, false] as [boolean, boolean],
  /** When a sync request was last refused, per deck (performance.now()). */
  syncDenied: [-Infinity, -Infinity] as [number, number],
  settings: loadSettings(),
  played: new Set<number>(),
  toasts: [] as Toast[],
  libraryVersion: 0,
  /** Background import/analysis progress, shown in the title bar. */
  job: null as null | { kind: "import" | "analyze"; done: number; total: number },
};

export function setMixer(state: MixerState) {
  store.mixer = state;
  store.receivedAt = performance.now();
  mixerChannel.notify();
}

export function updateApp(mutate: () => void) {
  mutate();
  appChannel.notify();
}

export function setSettings(patch: Partial<Settings>) {
  updateApp(() => {
    store.settings = { ...store.settings, ...patch };
    try {
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(store.settings));
    } catch {
      /* Settings stay in memory when storage is unavailable. */
    }
  });
}

let toastId = 0;
export function toast(kind: Toast["kind"], text: string, audio = false) {
  // Performance mode stays quiet, except for audio errors.
  if (store.settings.view === "performance" && !(audio && kind === "danger")) return;
  const t = { id: ++toastId, kind, text, audio };
  updateApp(() => {
    store.toasts = [...store.toasts.slice(-3), t];
  });
  setTimeout(() => updateApp(() => (store.toasts = store.toasts.filter((x) => x.id !== t.id))), 5000);
}

/** Playhead in track seconds, interpolated since the last state event. */
export function positionNow(deck: DeckId): number {
  const d = store.mixer?.decks[deck];
  if (!d) return 0;
  if (!d.playing) return d.position_s;
  const elapsed = (performance.now() - store.receivedAt) / 1000;
  return Math.min(d.duration_s, d.position_s + Math.min(elapsed, 0.25) * d.rate);
}

export function useMixer(): MixerState | null {
  useSyncExternalStore(mixerChannel.subscribe, () => mixerChannel.version);
  return store.mixer;
}

export function useApp(): typeof store {
  useSyncExternalStore(appChannel.subscribe, () => appChannel.version);
  return store;
}

/** Reads a design token, e.g. cssVar("--accent"). */
export function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

export const reducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;
