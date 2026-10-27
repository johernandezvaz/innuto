// Custom title bar (DESIGN.md §6): drag region, menus, master BPM, CPU, record, window controls.
import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Circle, Copy, Minus, Square, X } from "lucide-react";
import { addFolder, toggleRecording } from "../actions";
import { setSettings, updateApp, useApp, useMixer, type View } from "../store";
import { ui } from "../shortcuts";

type MenuItem = { label: string; hint?: string; checked?: boolean; action: () => void } | "sep";

function Menu({ label, items }: { label: string; items: MenuItem[] }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);
  return (
    <div className="menu" ref={ref}>
      <button className={`ghost menu-button ${open ? "open" : ""}`} onClick={() => setOpen(!open)} aria-haspopup="menu" aria-expanded={open}>
        {label}
      </button>
      {open && (
        <div className="menu-pop" role="menu">
          {items.map((it, i) =>
            it === "sep" ? (
              <div key={i} className="menu-sep" />
            ) : (
              <button
                key={i}
                role="menuitemcheckbox"
                aria-checked={it.checked}
                className="menu-item"
                onClick={() => {
                  setOpen(false);
                  it.action();
                }}
              >
                <span className="check">{it.checked ? "✓" : ""}</span>
                <span>{it.label}</span>
                {it.hint && <kbd>{it.hint}</kbd>}
              </button>
            ),
          )}
        </div>
      )}
    </div>
  );
}

export function TitleBar() {
  const app = useApp();
  const mixer = useMixer();
  const [maximized, setMaximized] = useState(false);
  const s = app.settings;
  const win = getCurrentWindow();

  useEffect(() => {
    void win.isMaximized().then(setMaximized);
    const un = win.onResized(() => void win.isMaximized().then(setMaximized));
    return () => void un.then((f) => f());
  }, []);

  const view = (v: View, label: string, hint: string): MenuItem => ({ label, hint, checked: s.view === v, action: () => setSettings({ view: v }) });
  const cpu = Math.round((mixer?.cpu ?? 0) * 100);
  const recording = mixer?.recording ?? null;

  return (
    <header className="titlebar" data-tauri-drag-region>
      <img src="/innuto-logo-dark.svg" alt="innuto" className="logo" data-tauri-drag-region />
      <Menu
        label="Archivo"
        items={[
          { label: "Agregar carpeta…", action: () => void addFolder() },
          "sep",
          { label: "Mapa de atajos", hint: "?", action: () => updateApp(() => (ui.showShortcuts = true)) },
        ]}
      />
      <Menu
        label="Ver"
        items={[
          view("performance", "Actuación", "F1"),
          view("prep", "Preparación", "F2"),
          view("export", "Exportar", "F3"),
          "sep",
          { label: "Forma de onda monocromo", checked: s.mono, action: () => setSettings({ mono: !s.mono }) },
          { label: "Luz de cabina", checked: s.booth, action: () => setSettings({ booth: !s.booth }) },
          { label: "Filas cómodas (36 px)", checked: s.rows === "comfortable", action: () => setSettings({ rows: s.rows === "comfortable" ? "compact" : "comfortable" }) },
          "sep",
          ...[0.9, 1, 1.15, 1.3, 1.5].map((v): MenuItem => ({ label: `Escala ${Math.round(v * 100)} %`, checked: Math.abs(s.scale - v) < 0.01, action: () => setSettings({ scale: v }) })),
        ]}
      />
      <div className="titlebar-fill" data-tauri-drag-region />
      <div className="status num" data-tauri-drag-region>
        {app.job && (
          <span title="Trabajo en segundo plano">
            <label>{app.job.kind === "import" ? "Importando" : "Analizando"}</label> {app.job.done}/{app.job.total}
          </span>
        )}
        <span title="Tempo maestro">
          <label>BPM maestro</label> {mixer?.master_bpm ? mixer.master_bpm.toFixed(2) : "---.--"}
        </span>
        <span title="Carga del motor de audio" className={cpu > 70 ? "warn" : ""}>
          <label>CPU</label> {cpu}%
        </span>
      </div>
      <button
        className={`record ${recording ? "on" : ""}`}
        onClick={() => void toggleRecording()}
        title={recording ? `Detener grabación (${recording})` : "Grabar la mezcla en Música/innuto"}
        aria-pressed={!!recording}
      >
        <Circle size={12} strokeWidth={0} fill="currentColor" /> {recording ? "Grabando" : "Grabar"}
      </button>
      <div className="window-controls">
        <button className="ghost icon" onClick={() => void win.minimize()} title="Minimizar" aria-label="Minimizar">
          <Minus size={16} strokeWidth={1.75} />
        </button>
        <button className="ghost icon" onClick={() => void win.toggleMaximize()} title={maximized ? "Restaurar" : "Maximizar"} aria-label="Maximizar">
          {maximized ? <Copy size={14} strokeWidth={1.75} /> : <Square size={14} strokeWidth={1.75} />}
        </button>
        <button className="ghost icon close" onClick={() => void win.close()} title="Cerrar" aria-label="Cerrar">
          <X size={16} strokeWidth={1.75} />
        </button>
      </div>
    </header>
  );
}
