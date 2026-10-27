// Keyboard map (DESIGN.md §11): every live control has a key, shown in tooltips and the `?` map.
import { api, type DeckId } from "./api";
import { hotCue, toggleSync } from "./actions";
import { setSettings, store, updateApp } from "./store";

export const DECK_KEYS = [
  { play: "F", cue: "D", sync: "S", hotCue: (slot: number) => String(slot + 1), codes: { play: "KeyF", cue: "KeyD", sync: "KeyS" } },
  { play: "J", cue: "K", sync: "L", hotCue: (slot: number) => `Mayús+${slot + 1}`, codes: { play: "KeyJ", cue: "KeyK", sync: "KeyL" } },
] as const;

export const SHORTCUTS: [string, string][] = [
  ["F / J", "Play/pausa deck A / B"],
  ["D / K (mantener)", "Cue deck A / B"],
  ["S / L", "Sync deck A / B"],
  ["1–8", "Hot cues deck A (Alt: borrar)"],
  ["Mayús+1–8", "Hot cues deck B (Alt: borrar)"],
  ["← / →", "Mover el crossfader (Mayús: fino)"],
  ["↑ / ↓", "Seleccionar pista"],
  ["Enter / Mayús+Enter", "Cargar en deck A / B"],
  ["Ctrl+F", "Buscar en la biblioteca"],
  ["F1 / F2 / F3", "Vista Actuación / Preparación / Exportar"],
  ["Ctrl+= / Ctrl+-", "Escala de la interfaz"],
  ["?", "Mostrar este mapa"],
  ["Esc", "Cerrar menús y diálogos"],
];

export const ui = {
  showShortcuts: false,
  focusSearch: () => {},
};

function isTyping(e: KeyboardEvent) {
  const t = e.target as HTMLElement | null;
  return !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable);
}

const held = new Set<string>();

export function installShortcuts(): () => void {
  const down = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      updateApp(() => (ui.showShortcuts = false));
      return;
    }
    if (e.ctrlKey && e.code === "KeyF") {
      e.preventDefault();
      ui.focusSearch();
      return;
    }
    if (e.ctrlKey && (e.key === "=" || e.key === "+" || e.key === "-")) {
      e.preventDefault();
      const next = store.settings.scale + (e.key === "-" ? -0.1 : 0.1);
      setSettings({ scale: Math.round(Math.min(1.5, Math.max(0.9, next)) * 10) / 10 });
      return;
    }
    const views = { F1: "performance", F2: "prep", F3: "export" } as const;
    if (e.key in views) {
      e.preventDefault();
      setSettings({ view: views[e.key as keyof typeof views] });
      return;
    }
    if (isTyping(e) || e.ctrlKey || e.metaKey) return;
    if (e.key === "?") {
      updateApp(() => (ui.showShortcuts = !ui.showShortcuts));
      return;
    }
    if (e.code === "ArrowLeft" || e.code === "ArrowRight") {
      const step = (e.shiftKey ? 0.02 : 0.1) * (e.code === "ArrowLeft" ? -1 : 1);
      void api.crossfader(Math.min(1, Math.max(-1, (store.mixer?.crossfader ?? 0) + step)));
      e.preventDefault();
      return;
    }
    const digit = /^Digit([1-8])$/.exec(e.code);
    if (digit) {
      void hotCue(e.shiftKey ? 1 : 0, Number(digit[1]) - 1, e.altKey);
      e.preventDefault();
      return;
    }
    for (const deck of [0, 1] as DeckId[]) {
      const c = DECK_KEYS[deck].codes;
      if (e.code === c.play && !e.repeat) void api.play(deck);
      else if (e.code === c.sync && !e.repeat) void toggleSync(deck);
      else if (e.code === c.cue) {
        if (!held.has(e.code)) {
          held.add(e.code);
          void api.cue(deck, true);
        }
      } else continue;
      e.preventDefault();
      return;
    }
  };
  const up = (e: KeyboardEvent) => {
    for (const deck of [0, 1] as DeckId[]) {
      if (e.code === DECK_KEYS[deck].codes.cue && held.delete(e.code)) void api.cue(deck, false);
    }
  };
  window.addEventListener("keydown", down);
  window.addEventListener("keyup", up);
  return () => {
    window.removeEventListener("keydown", down);
    window.removeEventListener("keyup", up);
  };
}
