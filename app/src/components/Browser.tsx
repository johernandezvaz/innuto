// Navigator + library (DESIGN.md §6, §7 "Biblioteca", "Estados vacíos").
import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, ChevronRight, Disc3, FolderPlus, History, ListMusic, Plus, Search, Sparkles, Usb } from "lucide-react";
import { api, DECK_COLOR_VAR, errorText, fmtTime, trackLabel, type PlaylistDto, type TrackDto } from "../api";
import { addFolder, loadDeck } from "../actions";
import { setSettings, store, toast, updateApp, useApp, useMixer } from "../store";
import { ui } from "../shortcuts";
import { TRACK_DRAG_TYPE } from "./Deck";

export type Source = { kind: "collection" } | { kind: "playlist"; id: number } | { kind: "history" };

function sameSource(a: Source, b: Source) {
  return a.kind === b.kind && (a.kind !== "playlist" || (b.kind === "playlist" && a.id === b.id));
}

function NavItem(props: { icon: React.ReactNode; label: string; active: boolean; onClick: () => void; depth?: number; onDropTracks?: (ids: number[]) => void; title?: string }) {
  const [over, setOver] = useState(false);
  return (
    <button
      className={`nav-item ${props.active ? "active" : ""} ${over ? "drop" : ""}`}
      style={{ paddingLeft: 12 + (props.depth ?? 0) * 14 }}
      onClick={props.onClick}
      title={props.title}
      onDragOver={(e) => {
        if (props.onDropTracks && e.dataTransfer.types.includes(TRACK_DRAG_TYPE)) {
          e.preventDefault();
          setOver(true);
        }
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        setOver(false);
        const id = Number(e.dataTransfer.getData(TRACK_DRAG_TYPE));
        if (id && props.onDropTracks) props.onDropTracks([id]);
      }}
    >
      {props.icon}
      <span>{props.label}</span>
    </button>
  );
}

function Navigator({ source, setSource, playlists, reload }: { source: Source; setSource: (s: Source) => void; playlists: PlaylistDto[]; reload: () => void }) {
  const [collapsed, setCollapsed] = useState<Set<number>>(new Set());
  const [creating, setCreating] = useState<null | { folder: boolean; parent: number | null }>(null);
  const [name, setName] = useState("");

  const create = async () => {
    if (!creating || !name.trim()) return setCreating(null);
    try {
      await api.createPlaylist(name.trim(), creating.parent, creating.folder);
      reload();
    } catch (e) {
      toast("danger", errorText(e));
    }
    setName("");
    setCreating(null);
  };

  const addTo = async (playlist: number, ids: number[]) => {
    try {
      await api.addToPlaylist(playlist, ids);
      toast("success", "Pista agregada a la playlist");
      if (source.kind === "playlist" && source.id === playlist) updateApp(() => store.libraryVersion++);
    } catch (e) {
      toast("danger", errorText(e));
    }
  };

  const tree = (parent: number | null, depth: number): React.ReactNode =>
    playlists
      .filter((p) => p.parent_id === parent)
      .map((p) =>
        p.is_folder ? (
          <div key={p.id}>
            <NavItem
              depth={depth}
              icon={collapsed.has(p.id) ? <ChevronRight size={16} strokeWidth={1.75} /> : <ChevronDown size={16} strokeWidth={1.75} />}
              label={p.name}
              active={false}
              onClick={() => setCollapsed((c) => { const n = new Set(c); n.has(p.id) ? n.delete(p.id) : n.add(p.id); return n; })}
            />
            {!collapsed.has(p.id) && (
              <>
                {tree(p.id, depth + 1)}
                <button className="nav-add" style={{ paddingLeft: 26 + (depth + 1) * 14 }} onClick={() => setCreating({ folder: false, parent: p.id })}>
                  + playlist aquí
                </button>
              </>
            )}
          </div>
        ) : (
          <NavItem
            key={p.id}
            depth={depth}
            icon={<ListMusic size={16} strokeWidth={1.75} />}
            label={p.name}
            active={sameSource(source, { kind: "playlist", id: p.id })}
            onClick={() => setSource({ kind: "playlist", id: p.id })}
            onDropTracks={(ids) => void addTo(p.id, ids)}
            title="Suelta pistas aquí para agregarlas"
          />
        ),
      );

  return (
    <nav className="navigator" aria-label="Navegador">
      <div className="nav-section">Navegador</div>
      <NavItem icon={<Disc3 size={16} strokeWidth={1.75} />} label="Colección" active={source.kind === "collection"} onClick={() => setSource({ kind: "collection" })} />
      <div className="nav-section with-actions">
        Playlists
        <span>
          <button className="ghost icon" title="Nueva playlist" onClick={() => setCreating({ folder: false, parent: null })}><Plus size={16} strokeWidth={1.75} /></button>
          <button className="ghost icon" title="Nueva carpeta" onClick={() => setCreating({ folder: true, parent: null })}><FolderPlus size={16} strokeWidth={1.75} /></button>
        </span>
      </div>
      {creating && (
        <input
          className="nav-input"
          autoFocus
          placeholder={creating.folder ? "Nombre de la carpeta" : "Nombre de la playlist"}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void create();
            if (e.key === "Escape") setCreating(null);
            e.stopPropagation();
          }}
          onBlur={() => void create()}
        />
      )}
      {tree(null, 0)}
      {playlists.length === 0 && !creating && <div className="nav-empty">Aún no hay playlists</div>}
      <div className="nav-section">Sesión</div>
      <NavItem icon={<History size={16} strokeWidth={1.75} />} label="Historial" active={source.kind === "history"} onClick={() => setSource({ kind: "history" })} />
      <NavItem icon={<Usb size={16} strokeWidth={1.75} />} label="Exportar (F3)" active={false} onClick={() => setSettings({ view: "export" })} />
    </nav>
  );
}

function EmptyLibrary() {
  return (
    <div className="empty-state">
      <img src="/innuto-mark.svg" alt="" className="empty-mark" />
      <h2>Arrastra tu música aquí</h2>
      <p>Carpetas o archivos de audio: MP3, WAV, FLAC, AIFF, M4A, OGG.</p>
      <button className="primary" onClick={() => void addFolder()}>
        <FolderPlus size={16} strokeWidth={1.75} /> Agregar carpeta
      </button>
    </div>
  );
}

const OVERSCAN = 8;

export function Browser() {
  const app = useApp();
  const mixer = useMixer();
  const [source, setSource] = useState<Source>({ kind: "collection" });
  const [filter, setFilter] = useState("");
  const [tracks, setTracks] = useState<TrackDto[]>([]);
  const [total, setTotal] = useState<number | null>(null);
  const [playlists, setPlaylists] = useState<PlaylistDto[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewH, setViewH] = useState(400);
  const listRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const rowH = app.settings.rows === "comfortable" ? 36 : 28;

  const reloadPlaylists = () => void api.playlists().then(setPlaylists).catch((e) => toast("danger", errorText(e)));
  useEffect(reloadPlaylists, []);
  useEffect(() => {
    ui.focusSearch = () => searchRef.current?.focus();
  }, []);

  useEffect(() => {
    let cancelled = false;
    const handle = setTimeout(async () => {
      try {
        let rows: TrackDto[];
        if (source.kind === "history") {
          const f = filter.toLowerCase();
          rows = (await api.history()).filter((t) => !f || `${t.title} ${t.artist} ${t.path}`.toLowerCase().includes(f));
        } else {
          rows = await api.tracks(filter, source.kind === "playlist" ? source.id : null);
        }
        if (!cancelled) setTracks(rows);
        if (source.kind === "collection" && !filter && !cancelled) setTotal(rows.length);
      } catch (e) {
        toast("danger", errorText(e));
      }
    }, 120);
    return () => {
      cancelled = true;
      clearTimeout(handle);
    };
  }, [source, filter, app.libraryVersion]);

  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setViewH(el.clientHeight));
    ro.observe(el);
    return () => ro.disconnect();
  }, [total]);

  const selIndex = useMemo(() => tracks.findIndex((t) => t.id === selected), [tracks, selected]);

  // Library navigation works from anywhere, including the search box.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      const typing = t.tagName === "INPUT" && t !== searchRef.current;
      if (typing || e.ctrlKey || e.altKey || !tracks.length) return;
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        const next = Math.min(tracks.length - 1, Math.max(0, selIndex + (e.key === "ArrowDown" ? 1 : -1)));
        setSelected(tracks[next].id);
        const el = listRef.current;
        if (el) {
          const top = next * rowH;
          if (top < el.scrollTop) el.scrollTop = top;
          else if (top + rowH > el.scrollTop + el.clientHeight) el.scrollTop = top + rowH - el.clientHeight;
        }
        e.preventDefault();
      } else if (e.key === "Enter" && selIndex >= 0) {
        void loadDeck(e.shiftKey ? 1 : 0, tracks[selIndex].id);
        e.preventDefault();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [tracks, selIndex, rowH]);

  const analyzeSelection = async () => {
    const ids = tracks.filter((t) => t.bpm === null).map((t) => t.id);
    const target = ids.length ? ids : selected !== null ? [selected] : [];
    if (!target.length) return toast("info", "Todas las pistas visibles ya están analizadas");
    await api.analyzeTracks(target);
    toast("info", `Analizando ${target.length} pista${target.length === 1 ? "" : "s"}`);
  };

  if (total === 0 && source.kind === "collection") {
    return (
      <div className="browser">
        <Navigator source={source} setSource={setSource} playlists={playlists} reload={reloadPlaylists} />
        <EmptyLibrary />
      </div>
    );
  }

  const first = Math.max(0, Math.floor(scrollTop / rowH) - OVERSCAN);
  const last = Math.min(tracks.length, Math.ceil((scrollTop + viewH) / rowH) + OVERSCAN);
  const loadedIn = (id: number) => [0, 1].filter((d) => mixer?.decks[d].track_id === id);
  const heading =
    source.kind === "collection" ? "Colección" : source.kind === "history" ? "Historial de la sesión" : playlists.find((p) => p.id === source.id)?.name ?? "Playlist";

  return (
    <div className="browser" style={{ gridTemplateColumns: `${app.settings.navWidth}px 4px 1fr` }}>
      <Navigator source={source} setSource={setSource} playlists={playlists} reload={reloadPlaylists} />
      <div
        className="divider vertical"
        onPointerDown={(e) => {
          const start = e.clientX;
          const w0 = store.settings.navWidth;
          const move = (ev: PointerEvent) => setSettings({ navWidth: Math.min(360, Math.max(150, w0 + ev.clientX - start)) });
          const up = () => { window.removeEventListener("pointermove", move); window.removeEventListener("pointerup", up); };
          window.addEventListener("pointermove", move);
          window.addEventListener("pointerup", up);
        }}
        role="separator"
        aria-orientation="vertical"
      />
      <div className="library">
        <div className="library-bar">
          <span className="library-title">{heading}</span>
          <span className="library-count num">{tracks.length}</span>
          <label className="search">
            <Search size={16} strokeWidth={1.75} />
            <input ref={searchRef} value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Buscar título, artista o ruta (Ctrl+F)" aria-label="Buscar" />
          </label>
          <button className="secondary" onClick={() => void addFolder()} title="Agregar carpeta a la colección">
            <FolderPlus size={16} strokeWidth={1.75} /> Agregar
          </button>
          {app.settings.view === "prep" && (
            <button className="primary" onClick={() => void analyzeSelection()} title="Analiza BPM, rejilla y forma de onda de las pistas visibles sin analizar">
              <Sparkles size={16} strokeWidth={1.75} /> Analizar
            </button>
          )}
        </div>
        <div className="track-head" role="row">
          <span />
          <span>Título</span>
          <span>Artista</span>
          <span className="right">BPM</span>
          <span className="right">Tono</span>
          <span className="right">Duración</span>
        </div>
        <div className="track-list" ref={listRef} onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)} role="grid" aria-rowcount={tracks.length}>
          <div style={{ height: tracks.length * rowH, position: "relative" }}>
            {tracks.slice(first, last).map((t, i) => {
              const decks = loadedIn(t.id);
              const played = app.played.has(t.id) && !decks.length;
              return (
                <div
                  key={t.id}
                  role="row"
                  className={`track-row ${t.id === selected ? "selected" : ""} ${played ? "played" : ""}`}
                  style={{ top: (first + i) * rowH, height: rowH }}
                  onClick={() => setSelected(t.id)}
                  onDoubleClick={() => void loadDeck(0, t.id)}
                  draggable
                  onDragStart={(e) => {
                    e.dataTransfer.setData(TRACK_DRAG_TYPE, String(t.id));
                    e.dataTransfer.effectAllowed = "copy";
                  }}
                  title="Doble clic o Enter: deck A · Mayús+Enter: deck B · Arrastra a un deck o playlist"
                >
                  <span className="dots">
                    {decks.map((d) => (
                      <i key={d} style={{ background: `var(${DECK_COLOR_VAR[d]})` }} aria-label={`Cargada en deck ${d === 0 ? "A" : "B"}`} />
                    ))}
                  </span>
                  <span className="ellipsis">{trackLabel(t)}</span>
                  <span className="ellipsis text-secondary">{t.artist ?? ""}</span>
                  <span className="right num">{t.bpm !== null ? t.bpm.toFixed(2) : <em className="muted">—</em>}</span>
                  <span className="right num muted">—</span>
                  <span className="right num">{t.duration_ms !== null ? fmtTime(t.duration_ms / 1000) : "--:--"}</span>
                </div>
              );
            })}
          </div>
          {tracks.length === 0 && <div className="list-empty">{filter ? "Ninguna pista coincide con la búsqueda" : source.kind === "history" ? "Las pistas que toques aparecerán aquí" : "Arrastra pistas desde la colección"}</div>}
        </div>
      </div>
    </div>
  );
}
