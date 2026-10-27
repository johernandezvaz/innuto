// User actions shared by components and keyboard shortcuts.
import { open } from "@tauri-apps/plugin-dialog";
import { api, errorText, trackLabel, type DeckId, type DeckLoaded } from "./api";
import { store, toast, updateApp } from "./store";

async function showLoaded(loaded: DeckLoaded) {
  const buf = await api.deckWaveform(loaded.deck);
  updateApp(() => {
    store.decks[loaded.deck] = {
      track: loaded.track,
      durationS: loaded.duration_s,
      bpm: loaded.bpm,
      firstBeatMs: loaded.first_beat_ms,
      cues: loaded.cues,
      waveform: new Uint8Array(buf),
      colsPerSec: loaded.waveform_columns_per_second,
    };
    store.libraryVersion++;
  });
}

/** Rebuilds deck views from the engine, e.g. after the window reloads. */
export async function restoreDecks() {
  for (const deck of [0, 1] as DeckId[]) {
    const loaded = await api.loadedDeck(deck).catch(() => null);
    if (loaded) await showLoaded(loaded);
  }
}

export async function loadDeck(deck: DeckId, trackId: number) {
  if (store.mixer?.decks[deck].playing) {
    toast("warning", `El deck ${deck === 0 ? "A" : "B"} está al aire; páusalo antes de cargar otra pista.`);
    return;
  }
  updateApp(() => (store.loading[deck] = true));
  try {
    const loaded = await api.loadDeck(deck, trackId);
    await showLoaded(loaded);
    toast("info", `${trackLabel(loaded.track)} → deck ${deck === 0 ? "A" : "B"}`);
  } catch (e) {
    toast("danger", `No se pudo cargar la pista: ${errorText(e)}`);
  } finally {
    updateApp(() => (store.loading[deck] = false));
  }
}

export async function hotCue(deck: DeckId, slot: number, del = false) {
  if (!store.decks[deck]) return;
  try {
    const cues = await api.hotCue(deck, slot, del);
    updateApp(() => {
      const d = store.decks[deck];
      if (d) store.decks[deck] = { ...d, cues };
    });
  } catch (e) {
    toast("danger", errorText(e));
  }
}

export async function toggleSync(deck: DeckId) {
  const engaged = !store.mixer?.decks[deck].sync;
  const ok = await api.sync(deck, engaged);
  if (!ok) {
    // Toasts are muted in performance view, so the button itself flashes too.
    updateApp(() => (store.syncDenied[deck] = performance.now()));
    toast("warning", "Sync necesita una pista cargada en ambos decks.");
  }
}

export async function addFolder() {
  const picked = await open({ directory: true, multiple: true, title: "Agregar carpeta de música" });
  const paths = picked === null ? [] : Array.isArray(picked) ? picked : [picked];
  for (const p of paths) await importPath(p);
}

export async function importPath(path: string) {
  try {
    await api.importFolder(path);
    toast("info", `Importando ${path}`);
  } catch (e) {
    toast("danger", errorText(e));
  }
}

export async function toggleRecording() {
  try {
    const path = await api.toggleRecording();
    toast(path ? "warning" : "success", path ? `Grabando en ${path}` : "Grabación guardada");
  } catch (e) {
    toast("danger", `Grabación: ${errorText(e)}`, true);
  }
}
