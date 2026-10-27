// Toasts, the shortcut map, and the export view.
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Usb } from "lucide-react";
import { api, errorText, type PlaylistDto } from "../api";
import { toast, updateApp, useApp } from "../store";
import { SHORTCUTS, ui } from "../shortcuts";

export function Toasts() {
  const app = useApp();
  return (
    <div className="toasts" role="status" aria-live="polite">
      {app.toasts.map((t) => (
        <div key={t.id} className={`toast ${t.kind}`}>
          {t.text}
        </div>
      ))}
    </div>
  );
}

export function ShortcutMap() {
  useApp();
  if (!ui.showShortcuts) return null;
  return (
    <div className="dialog-backdrop" onClick={() => updateApp(() => (ui.showShortcuts = false))}>
      <div className="dialog" role="dialog" aria-label="Mapa de atajos" onClick={(e) => e.stopPropagation()}>
        <h2>Atajos de teclado</h2>
        <table className="shortcuts">
          <tbody>
            {SHORTCUTS.map(([k, d]) => (
              <tr key={k}>
                <td>
                  <kbd>{k}</kbd>
                </td>
                <td>{d}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <button className="secondary" onClick={() => updateApp(() => (ui.showShortcuts = false))} autoFocus>
          Cerrar (Esc)
        </button>
      </div>
    </div>
  );
}

export function ExportView() {
  const [playlists, setPlaylists] = useState<PlaylistDto[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => void api.playlists().then((p) => setPlaylists(p.filter((x) => !x.is_folder))), []);

  const run = async () => {
    if (selected === null) return;
    const folder = await open({ directory: true, title: "Carpeta de destino (por ejemplo, la raíz de una memoria USB)" });
    if (typeof folder !== "string") return;
    setBusy(true);
    try {
      const n = await api.exportPlaylistAnalysis(selected, folder);
      toast("success", `${n} archivo${n === 1 ? "" : "s"} de análisis escritos en ${folder}`);
    } catch (e) {
      toast("danger", errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="export-view">
      <h2>
        <Usb size={20} strokeWidth={1.75} /> Exportar análisis para reproductores de DJ
      </h2>
      <p className="text-secondary">
        Escribe la rejilla de beats y la vista previa de la forma de onda de cada pista analizada en formato ANLZ, una carpeta por pista.
        La exportación completa de USB (base de datos de dispositivo y copia de audio) aún no está implementada, y la compatibilidad con
        reproductores reales no está verificada.
      </p>
      <div className="export-list" role="listbox" aria-label="Playlists">
        {playlists.length === 0 && <div className="list-empty">Crea una playlist en el navegador para exportarla.</div>}
        {playlists.map((p) => (
          <button key={p.id} role="option" aria-selected={selected === p.id} className={`export-item ${selected === p.id ? "selected" : ""}`} onClick={() => setSelected(p.id)}>
            {p.name}
          </button>
        ))}
      </div>
      <button className="primary" disabled={selected === null || busy} onClick={() => void run()}>
        {busy ? "Exportando…" : "Exportar"}
      </button>
    </div>
  );
}
