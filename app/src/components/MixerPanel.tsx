// Center mixer (DESIGN.md §6): EQ, filter, channel faders, meters, crossfader.
import { useCallback } from "react";
import { api, DECK_COLOR_VAR, DECK_LETTER, type DeckId } from "../api";
import { store, useMixer } from "../store";
import { Fader, Knob, Meter } from "./controls";

/** EQ knobs run in dB; the bottom of the travel is a full kill. */
const EQ_MIN_DB = -26;
const EQ_MAX_DB = 6;

const dbToGain = (db: number) => (db <= EQ_MIN_DB ? 0 : Math.pow(10, db / 20));
const gainToDb = (g: number) => (g <= 0 ? EQ_MIN_DB : Math.max(EQ_MIN_DB, 20 * Math.log10(g)));
const fmtDb = (db: number) => (db <= EQ_MIN_DB ? "kill" : `${db >= 0 ? "+" : ""}${db.toFixed(1)} dB`);
const fmtFilter = (v: number) => (Math.abs(v) < 0.01 ? "abierto" : v < 0 ? `LPF ${Math.round(-v * 100)}%` : `HPF ${Math.round(v * 100)}%`);

function Channel({ deck }: { deck: DeckId }) {
  const mixer = useMixer();
  const d = mixer?.decks[deck];
  const color = DECK_COLOR_VAR[deck];
  const readPeak = useCallback(() => store.mixer?.decks[deck].peak ?? [0, 0], [deck]) as () => [number, number];
  const bands = ["HI", "MID", "LOW"] as const;
  // Faders sit toward the centre of the mixer: A is knobs|fader, B is fader|knobs.
  return (
    <div className={`channel ${deck === 1 ? "mirrored" : ""}`} aria-label={`Canal ${DECK_LETTER[deck]}`}>
      <div className="channel-knobs">
        <span className="channel-letter" style={{ color: `var(${color})` }}>{DECK_LETTER[deck]}</span>
        {bands.map((label, i) => {
          const band = 2 - i;
          return (
            <Knob
              key={label}
              size={34}
              label={label}
              value={gainToDb(d?.eq[band] ?? 1)}
              min={EQ_MIN_DB}
              max={EQ_MAX_DB}
              defaultValue={0}
              bipolar
              color={color}
              format={fmtDb}
              onChange={(db) => void api.eq(deck, band, dbToGain(db))}
            />
          );
        })}
        <Knob
          size={34}
          label="FILTER"
          value={d?.filter ?? 0}
          min={-1}
          max={1}
          defaultValue={0}
          bipolar
          color={color}
          format={fmtFilter}
          onChange={(v) => void api.filter(deck, v)}
        />
      </div>
      <div className="channel-fader">
        <Meter read={readPeak} height={150} />
        <Fader
          orientation="vertical"
          length={150}
          value={d?.gain ?? 1}
          min={0}
          max={1}
          defaultValue={1}
          color={color}
          label="VOL"
          format={(v) => (v <= 0 ? "-∞ dB" : `${(20 * Math.log10(v)).toFixed(1)} dB`)}
          onChange={(v) => void api.gain(deck, v)}
        />
      </div>
    </div>
  );
}

export function MixerPanel() {
  const mixer = useMixer();
  const readMaster = useCallback(() => store.mixer?.master_peak ?? [0, 0], []) as () => [number, number];
  return (
    <section className="mixer" aria-label="Mixer">
      <div className="mixer-channels">
        <Channel deck={0} />
        <div className="master">
          <Meter read={readMaster} height={150} />
          <span className="control-label">MASTER</span>
        </div>
        <Channel deck={1} />
      </div>
      <Fader
        orientation="horizontal"
        length={180}
        value={mixer?.crossfader ?? 0}
        min={-1}
        max={1}
        defaultValue={0}
        bipolar
        color="--text-secondary"
        label="CROSSFADER (← →)"
        format={(v) => (Math.abs(v) < 0.01 ? "centro" : v < 0 ? `A ${Math.round(-v * 100)}%` : `B ${Math.round(v * 100)}%`)}
        onChange={(v) => void api.crossfader(v)}
      />
    </section>
  );
}
