// One deck (DESIGN.md §6, §7 "Controles de transporte").
import { useEffect, useRef } from "react";
import { Pause, Play } from "lucide-react";
import { api, DECK_COLOR_VAR, DECK_LETTER, fmtTime, trackLabel, type DeckId } from "../api";
import { hotCue, loadDeck, toggleSync } from "../actions";
import { positionNow, reducedMotion, store, useApp, useMixer } from "../store";
import { Fader } from "./controls";
import { OverviewWaveform } from "./waveforms";
import { DECK_KEYS } from "../shortcuts";

export const TRACK_DRAG_TYPE = "application/x-innuto-track";
const PITCH_RANGE = 8;

export function Deck({ deck }: { deck: DeckId }) {
  const app = useApp();
  const mixer = useMixer();
  const media = app.decks[deck];
  const state = mixer?.decks[deck];
  const color = DECK_COLOR_VAR[deck];
  const keys = DECK_KEYS[deck];
  const playRef = useRef<HTMLButtonElement>(null);

  // Play button pulse, phase-locked to the beat grid.
  useEffect(() => {
    if (reducedMotion()) return;
    let raf = 0;
    const tick = () => {
      const m = store.decks[deck];
      const s = store.mixer?.decks[deck];
      const el = playRef.current;
      if (el) {
        let pulse = 0;
        if (m && s?.playing) {
          const beat = 60 / m.bpm;
          const phase = ((positionNow(deck) - m.firstBeatMs / 1000) / beat) % 1;
          pulse = Math.max(0, 1 - (phase < 0 ? phase + 1 : phase) * 4);
        }
        el.style.setProperty("--pulse", pulse.toFixed(3));
      }
      raf = requestAnimationFrame(tick);
    };
    tick();
    return () => cancelAnimationFrame(raf);
  }, [deck]);

  const remaining = state && media ? Math.max(0, media.durationS - state.position_s) : 0;
  const ending = !!state?.playing && remaining < 30;
  const hot = new Map((media?.cues ?? []).filter((c) => c.slot !== null).map((c) => [c.slot!, c]));

  const transport = (
    <div className="transport">
      <button
        className="round cue"
        onPointerDown={() => void api.cue(deck, true)}
        onPointerUp={() => void api.cue(deck, false)}
        onPointerLeave={(e) => e.buttons === 1 && void api.cue(deck, false)}
        disabled={!media}
        title={`Cue: mantener para previsualizar (${keys.cue})`}
      >
        CUE
      </button>
      <button
        ref={playRef}
        className={`round play ${state?.playing ? "on" : ""}`}
        onClick={() => void api.play(deck)}
        disabled={!media}
        title={`${state?.playing ? "Pausa" : "Play"} (${keys.play})`}
        aria-pressed={!!state?.playing}
      >
        {state?.playing ? <Pause size={20} strokeWidth={1.75} /> : <Play size={20} strokeWidth={1.75} />}
      </button>
      <button
        className={`pill sync ${state?.sync ? "on" : ""} ${state?.sync_lost ? "lost" : ""} ${performance.now() - app.syncDenied[deck] < 800 ? "denied" : ""}`}
        style={{ ["--deck-color" as string]: `var(${color})` }}
        onClick={() => void toggleSync(deck)}
        disabled={!media}
        title={`Sync: sigue el tempo del otro deck (${keys.sync})${state?.sync_lost ? " · fase perdida" : ""}`}
        aria-pressed={!!state?.sync}
      >
        SYNC{state?.sync_lost ? " !" : ""}
      </button>
    </div>
  );

  const pads = (
    <div className="pads" role="group" aria-label={`Hot cues del deck ${DECK_LETTER[deck]}`}>
      {Array.from({ length: 8 }, (_, slot) => {
        const cue = hot.get(slot);
        return (
          <button
            key={slot}
            className={`pad ${cue ? "set" : ""}`}
            style={cue ? { background: `var(--cue-${slot + 1})` } : undefined}
            disabled={!media}
            onClick={(e) => void hotCue(deck, slot, e.altKey)}
            onContextMenu={(e) => {
              e.preventDefault();
              if (cue) void hotCue(deck, slot, true);
            }}
            title={`Hot cue ${slot + 1} (${keys.hotCue(slot)}${cue ? " · Alt+clic o clic derecho: borrar" : " · guarda en el beat más cercano"})`}
          >
            {slot + 1}
          </button>
        );
      })}
    </div>
  );

  return (
    <section
      className={`deck deck-${DECK_LETTER[deck].toLowerCase()}`}
      style={{ ["--deck-color" as string]: `var(${color})` }}
      aria-label={`Deck ${DECK_LETTER[deck]}`}
      onDragOver={(e) => e.dataTransfer.types.includes(TRACK_DRAG_TYPE) && e.preventDefault()}
      onDrop={(e) => {
        const id = Number(e.dataTransfer.getData(TRACK_DRAG_TYPE));
        if (id) void loadDeck(deck, id);
      }}
    >
      <div className="deck-main">
        <header className="deck-head">
          <span className="deck-badge">{DECK_LETTER[deck]}</span>
          <div className="deck-title">
            {media ? (
              <>
                <div className="title">{trackLabel(media.track)}</div>
                <div className="artist">{media.track.artist ?? "Artista desconocido"}</div>
              </>
            ) : (
              <div className="artist">{app.loading[deck] ? "Cargando…" : "Arrastra una pista aquí o pulsa Enter en la biblioteca"}</div>
            )}
          </div>
        </header>
        <OverviewWaveform deck={deck} />
        <div className="readouts">
          <div className={`readout big ${ending ? "ending" : ""}`} title="Tiempo restante">
            <span className="num">{media ? fmtTime(remaining, true) : "--:--"}</span>
            <label>restante</label>
          </div>
          <div className="readout big" title="BPM actual">
            <span className="num">{state?.bpm ? state.bpm.toFixed(2) : "---.--"}</span>
            <label>bpm</label>
          </div>
          <div className="readout" title="Tonalidad (Camelot). La detección de tonalidad aún no está implementada.">
            <span className="num">—</span>
            <label>tono</label>
          </div>
          <div className="readout" title="Pitch">
            <span className="num">{state ? `${state.pitch_pct >= 0 ? "+" : ""}${state.pitch_pct.toFixed(2)}%` : "+0.00%"}</span>
            <label>pitch</label>
          </div>
          <div className="readout" title="Tiempo transcurrido">
            <span className="num">{fmtTime(state?.position_s ?? 0)}</span>
            <label>transcurrido</label>
          </div>
        </div>
        <div className={`deck-controls ${deck === 1 ? "mirrored" : ""}`}>
          {transport}
          {pads}
        </div>
      </div>
      <Fader
        orientation="vertical"
        length={150}
        value={state?.pitch_pct ?? 0}
        min={-PITCH_RANGE}
        max={PITCH_RANGE}
        defaultValue={0}
        bipolar
        color={color}
        label="PITCH"
        format={(v) => `${v >= 0 ? "+" : ""}${v.toFixed(2)}%`}
        onChange={(v) => void api.pitch(deck, v)}
      />
    </section>
  );
}
