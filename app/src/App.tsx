import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, type JobEvent, type MixerState } from "./api";
import { importPath, restoreDecks } from "./actions";
import { setMixer, setSettings, store, toast, updateApp, useApp } from "./store";
import { installShortcuts } from "./shortcuts";
import { TitleBar } from "./components/TitleBar";
import { ZoomWaveforms } from "./components/waveforms";
import { Deck } from "./components/Deck";
import { MixerPanel } from "./components/MixerPanel";
import { Browser } from "./components/Browser";
import { ExportView, ShortcutMap, Toasts } from "./components/Overlays";

function useBackendEvents() {
  useEffect(() => {
    const unlisten = [
      listen<MixerState>("mixer-state", (e) => setMixer(e.payload)),
      listen("library-changed", () => updateApp(() => store.libraryVersion++)),
      listen("history-changed", async () => {
        const h = await api.history();
        updateApp(() => (store.played = new Set(h.map((t) => t.id))));
      }),
      listen<JobEvent>("job", ({ payload: j }) => {
        if (j.kind === "error") toast("danger", j.message);
        else if (j.kind === "analyze-done") {
          updateApp(() => (store.job = null));
          if (j.total) toast("success", `Análisis terminado: ${j.total} pista${j.total === 1 ? "" : "s"}`);
        } else updateApp(() => (store.job = { kind: j.kind as "import" | "analyze", done: j.done, total: j.total }));
      }),
      getCurrentWebview().onDragDropEvent((e) => {
        if (e.payload.type === "drop") e.payload.paths.forEach((p) => void importPath(p));
      }),
    ];
    void restoreDecks();
    void api.audioStatus().then((s) => {
      if (s.error) toast("danger", `Sin salida de audio: ${s.error}`, true);
    });
    const uninstall = installShortcuts();
    return () => {
      uninstall();
      unlisten.forEach((u) => void u.then((f) => f()));
    };
  }, []);
}

function HorizontalDivider() {
  return (
    <div
      className="divider horizontal"
      role="separator"
      aria-orientation="horizontal"
      title="Arrastra para cambiar la altura de las formas de onda"
      onPointerDown={(e) => {
        const start = e.clientY;
        const h0 = store.settings.waveHeight;
        const move = (ev: PointerEvent) =>
          setSettings({ waveHeight: Math.min(320, Math.max(80, h0 + (ev.clientY - start) / store.settings.scale)) });
        const up = () => {
          window.removeEventListener("pointermove", move);
          window.removeEventListener("pointerup", up);
        };
        window.addEventListener("pointermove", move);
        window.addEventListener("pointerup", up);
      }}
    />
  );
}

export function App() {
  const app = useApp();
  const s = app.settings;
  useBackendEvents();

  useEffect(() => {
    const root = document.documentElement;
    root.dataset.booth = s.booth ? "on" : "off";
    root.dataset.rows = s.rows;
  }, [s.booth, s.rows]);

  return (
    <div
      className={`app view-${s.view}`}
      style={{ zoom: s.scale, width: `calc(100vw / ${s.scale})`, height: `calc(100vh / ${s.scale})` }}
    >
      <TitleBar />
      {s.view === "export" ? (
        <main className="export-layout">
          <ExportView />
        </main>
      ) : (
        <main
          className="performance-layout"
          style={{ gridTemplateRows: `${s.view === "prep" ? Math.min(s.waveHeight, 110) : s.waveHeight}px 4px auto minmax(140px, 1fr)` }}
        >
          <ZoomWaveforms />
          <HorizontalDivider />
          <div className="decks-row">
            <Deck deck={0} />
            {s.view === "performance" && (
              <>
                <MixerPanel />
                <Deck deck={1} />
              </>
            )}
          </div>
          <Browser />
        </main>
      )}
      <Toasts />
      <ShortcutMap />
    </div>
  );
}
