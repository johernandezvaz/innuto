// Canvas waveforms (DESIGN.md §3.5, §10): three-band colors computed in Rust,
// drawn with requestAnimationFrame, playhead interpolated between state events.
import { useEffect, useRef } from "react";
import { api, DECK_COLOR_VAR, type DeckId } from "../api";
import { cssVar, positionNow, store, appChannel } from "../store";

/** Beats visible on each side of the playhead in the zoomed view. */
const ZOOM_BEATS = 8;

interface Palette {
  low: string;
  mid: string;
  high: string;
  playhead: string;
  grid: string;
  downbeat: string;
  deck: [string, string];
  cues: string[];
  bg: string;
  gain: number;
}

function readPalette(): Palette {
  return {
    low: cssVar("--wave-low"),
    mid: cssVar("--wave-mid"),
    high: cssVar("--wave-high"),
    playhead: cssVar("--wave-playhead"),
    grid: cssVar("--border-strong"),
    downbeat: cssVar("--text-muted"),
    deck: [cssVar("--deck-a"), cssVar("--deck-b")],
    cues: Array.from({ length: 8 }, (_, i) => cssVar(`--cue-${i + 1}`)),
    bg: cssVar("--bg-base"),
    gain: Number(cssVar("--wave-gain")) || 1,
  };
}

/** Keeps a canvas sized to its box at device resolution. */
function useCanvas(draw: (ctx: CanvasRenderingContext2D, w: number, h: number, pal: Palette) => void) {
  const ref = useRef<HTMLCanvasElement>(null);
  const drawRef = useRef(draw);
  drawRef.current = draw;
  useEffect(() => {
    const canvas = ref.current!;
    const ctx = canvas.getContext("2d")!;
    let pal = readPalette();
    let w = 0;
    let h = 0;
    const resize = () => {
      const dpr = window.devicePixelRatio || 1;
      const r = canvas.getBoundingClientRect();
      w = r.width;
      h = r.height;
      canvas.width = Math.max(1, Math.round(w * dpr));
      canvas.height = Math.max(1, Math.round(h * dpr));
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    resize();
    // Settings such as booth light change tokens; re-read them on app updates.
    const unsub = appChannel.subscribe(() => (pal = readPalette()));
    let raf = 0;
    const frame = () => {
      ctx.clearRect(0, 0, w, h);
      drawRef.current(ctx, w, h, pal);
      raf = requestAnimationFrame(frame);
    };
    frame();
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      unsub();
    };
  }, []);
  return ref;
}

/** Max of each band over columns [c0, c1). */
function bandsAt(wf: Uint8Array, c0: number, c1: number): [number, number, number] {
  const cols = wf.length / 3;
  let lo = 0, mi = 0, hi = 0;
  const a = Math.max(0, Math.floor(c0));
  const b = Math.min(cols, Math.max(a + 1, Math.ceil(c1)));
  for (let c = a; c < b; c++) {
    lo = Math.max(lo, wf[c * 3]);
    mi = Math.max(mi, wf[c * 3 + 1]);
    hi = Math.max(hi, wf[c * 3 + 2]);
  }
  return [lo / 255, mi / 255, hi / 255];
}

/** Draws one mirrored column; `alpha` dims the already-played part. */
function drawColumn(
  ctx: CanvasRenderingContext2D, x: number, width: number, mid: number, half: number,
  b: [number, number, number], pal: Palette, mono: string | null, alpha: number,
) {
  ctx.globalAlpha = alpha;
  const layers: [number, string][] = mono
    ? [[Math.max(...b), mono]]
    : [[b[0], pal.low], [b[1] * 0.8, pal.mid], [b[2] * 0.6, pal.high]];
  for (const [v, color] of layers) {
    const hh = Math.min(1, v * pal.gain) * half;
    if (hh < 0.5) continue;
    ctx.fillStyle = color;
    ctx.fillRect(x, mid - hh, width, hh * 2);
  }
  ctx.globalAlpha = 1;
}

function cueColor(pal: Palette, slot: number | null) {
  return slot === null ? pal.playhead : pal.cues[slot % 8];
}

/** Both decks' zoomed waveforms, scaled so one beat has the same width on
 *  each lane: beats line up visually when the decks are in sync. */
export function ZoomWaveforms() {
  const ref = useCanvas((ctx, w, h, pal) => {
    const laneH = h / 2;
    const pxPerBeat = w / (ZOOM_BEATS * 2);
    const cx = w / 2;
    for (const deck of [0, 1] as DeckId[]) {
      const media = store.decks[deck];
      const top = deck * laneH;
      const mid = top + laneH / 2;
      if (!media?.waveform) {
        ctx.fillStyle = pal.grid;
        ctx.fillRect(0, mid, w, 1);
        continue;
      }
      const beatS = 60 / media.bpm;
      const pos = positionNow(deck);
      const secPerPx = beatS / pxPerBeat;
      const mono = store.settings.mono ? pal.deck[deck] : null;
      const firstBeat = media.firstBeatMs / 1000;
      // Beat grid first, under the waveform.
      const kStart = Math.floor((pos - cx * secPerPx - firstBeat) / beatS);
      const kEnd = Math.ceil((pos + cx * secPerPx - firstBeat) / beatS);
      for (let k = kStart; k <= kEnd; k++) {
        const x = cx + (firstBeat + k * beatS - pos) / secPerPx;
        ctx.fillStyle = k % 4 === 0 ? pal.downbeat : pal.grid;
        ctx.fillRect(Math.round(x), top + 2, 1, laneH - 4);
      }
      const step = 2;
      for (let x = 0; x < w; x += step) {
        const t0 = pos + (x - cx) * secPerPx;
        if (t0 < 0 || t0 > media.durationS) continue;
        const b = bandsAt(media.waveform, t0 * media.colsPerSec, (t0 + step * secPerPx) * media.colsPerSec);
        drawColumn(ctx, x, step - 0.5, mid, laneH / 2 - 4, b, pal, mono, x < cx ? 0.35 : 1);
      }
      for (const c of media.cues) {
        const x = cx + (c.position_ms / 1000 - pos) / secPerPx;
        if (x < -10 || x > w + 10) continue;
        ctx.fillStyle = cueColor(pal, c.slot);
        ctx.fillRect(x - 1, top + 2, 2, laneH - 4);
        if (c.slot !== null) {
          ctx.font = "700 10px JetBrains Mono";
          ctx.fillRect(x, top + 2, 14, 13);
          ctx.fillStyle = pal.bg;
          ctx.fillText(String(c.slot + 1), x + 4, top + 12);
        }
      }
      ctx.fillStyle = pal.deck[deck];
      ctx.font = "700 11px JetBrains Mono";
      ctx.fillText(deck === 0 ? "A" : "B", 6, top + 14);
    }
    ctx.fillStyle = pal.playhead;
    ctx.fillRect(cx - 1, 0, 2, h);
  });
  return <canvas ref={ref} className="zoom-waveforms" aria-label="Formas de onda ampliadas de los decks A y B" />;
}

/** Whole-track waveform of one deck; click or drag to seek. */
export function OverviewWaveform({ deck }: { deck: DeckId }) {
  const ref = useCanvas((ctx, w, h, pal) => {
    const media = store.decks[deck];
    if (!media?.waveform) return;
    const cols = media.waveform.length / 3;
    const pos = positionNow(deck);
    const playX = (pos / media.durationS) * w;
    const mono = store.settings.mono ? pal.deck[deck] : null;
    for (let x = 0; x < w; x += 1) {
      const b = bandsAt(media.waveform, (x / w) * cols, ((x + 1) / w) * cols);
      drawColumn(ctx, x, 1, h / 2, h / 2 - 2, b, pal, mono, x < playX ? 0.35 : 1);
    }
    for (const c of media.cues) {
      const x = (c.position_ms / 1000 / media.durationS) * w;
      ctx.fillStyle = cueColor(pal, c.slot);
      ctx.fillRect(x - 1, 0, 2, h);
    }
    const remaining = media.durationS - pos;
    ctx.fillStyle = remaining < 30 && store.mixer?.decks[deck].playing ? cssVar("--hot") : pal.playhead;
    ctx.fillRect(playX - 1, 0, 2, h);
  });

  const seek = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const media = store.decks[deck];
    if (!media) return;
    const r = e.currentTarget.getBoundingClientRect();
    const frac = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    void api.seek(deck, frac * media.durationS);
  };

  return (
    <canvas
      ref={ref}
      className="overview-waveform"
      style={{ borderColor: `var(${DECK_COLOR_VAR[deck]})` }}
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture(e.pointerId);
        seek(e);
      }}
      onPointerMove={(e) => e.buttons === 1 && seek(e)}
      aria-label={`Forma de onda completa del deck ${deck === 0 ? "A" : "B"}; clic para buscar`}
    />
  );
}
