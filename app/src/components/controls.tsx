// Knobs, faders, and level meters (DESIGN.md §7 "Perillas y faders", "Medidores").
import { useEffect, useRef, useState } from "react";
import { cssVar } from "../store";

interface ValueControlProps {
  value: number;
  min: number;
  max: number;
  defaultValue: number;
  onChange: (v: number) => void;
  label: string;
  format: (v: number) => string;
  /** Token name, e.g. "--deck-a". */
  color: string;
  /** Arc/fill starts from the default (bipolar) instead of the minimum. */
  bipolar?: boolean;
  title?: string;
}

/** Shared drag / wheel / keyboard / double-click behaviour. */
function useValueGesture(p: ValueControlProps, axis: "x" | "y", travelPx: number) {
  const [dragging, setDragging] = useState(false);
  const range = p.max - p.min;
  const clamp = (v: number) => Math.min(p.max, Math.max(p.min, v));

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const start = axis === "y" ? e.clientY : e.clientX;
    const startValue = p.value;
    setDragging(true);
    const move = (ev: PointerEvent) => {
      const delta = axis === "y" ? start - ev.clientY : ev.clientX - start;
      const fine = ev.shiftKey ? 0.2 : 1;
      p.onChange(clamp(startValue + (delta / travelPx) * range * fine));
    };
    const up = () => {
      setDragging(false);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const onWheel = (e: React.WheelEvent) => {
    const step = range / (e.shiftKey ? 500 : 100);
    p.onChange(clamp(p.value + (e.deltaY < 0 ? step : -step)));
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const step = range / (e.shiftKey ? 200 : 40);
    const up = axis === "y" ? "ArrowUp" : "ArrowRight";
    const down = axis === "y" ? "ArrowDown" : "ArrowLeft";
    if (e.key === up) p.onChange(clamp(p.value + step));
    else if (e.key === down) p.onChange(clamp(p.value - step));
    else if (e.key === "Home" || e.key === "0") p.onChange(p.defaultValue);
    else return;
    e.preventDefault();
    e.stopPropagation();
  };

  const handlers = {
    onPointerDown,
    onWheel,
    onKeyDown,
    onDoubleClick: () => p.onChange(p.defaultValue),
    tabIndex: 0,
    role: "slider",
    "aria-label": p.label,
    "aria-valuemin": p.min,
    "aria-valuemax": p.max,
    "aria-valuenow": Number(p.value.toFixed(3)),
    "aria-valuetext": p.format(p.value),
    title: p.title ?? `${p.label}: ${p.format(p.value)} (doble clic: restablecer)`,
  };
  return { dragging, handlers };
}

const ARC_START = -135;
const ARC_SWEEP = 270;

function polar(cx: number, cy: number, r: number, deg: number) {
  const rad = ((deg - 90) * Math.PI) / 180;
  return [cx + r * Math.cos(rad), cy + r * Math.sin(rad)];
}

function arcPath(cx: number, cy: number, r: number, a0: number, a1: number) {
  const [x0, y0] = polar(cx, cy, r, a0);
  const [x1, y1] = polar(cx, cy, r, a1);
  const large = Math.abs(a1 - a0) > 180 ? 1 : 0;
  const sweep = a1 > a0 ? 1 : 0;
  return `M ${x0} ${y0} A ${r} ${r} 0 ${large} ${sweep} ${x1} ${y1}`;
}

export function Knob(p: ValueControlProps & { size?: number }) {
  const size = p.size ?? 40;
  const { dragging, handlers } = useValueGesture(p, "y", 160);
  const norm = (v: number) => (v - p.min) / (p.max - p.min);
  const angle = ARC_START + norm(p.value) * ARC_SWEEP;
  const origin = ARC_START + norm(p.bipolar ? p.defaultValue : p.min) * ARC_SWEEP;
  const c = size / 2;
  const r = c - 4;
  const [px, py] = polar(c, c, r - 6, angle);
  const [tx, ty] = polar(c, c, r + 2, ARC_START + norm(p.defaultValue) * ARC_SWEEP);
  const [tx2, ty2] = polar(c, c, r - 3, ARC_START + norm(p.defaultValue) * ARC_SWEEP);
  return (
    <div className="control knob" {...handlers}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden>
        <path d={arcPath(c, c, r, ARC_START, ARC_START + ARC_SWEEP)} className="knob-track" />
        {Math.abs(angle - origin) > 0.5 && (
          <path d={arcPath(c, c, r, Math.min(origin, angle), Math.max(origin, angle))} style={{ stroke: `var(${p.color})` }} className="knob-value" />
        )}
        <line x1={tx} y1={ty} x2={tx2} y2={ty2} className="knob-center" />
        <circle cx={c} cy={c} r={r - 5} className="knob-cap" />
        <line x1={c} y1={c} x2={px} y2={py} className="knob-pointer" />
      </svg>
      <span className="control-label">{p.label}</span>
      {dragging && <span className="control-tip num">{p.format(p.value)}</span>}
    </div>
  );
}

export function Fader(p: ValueControlProps & { orientation: "vertical" | "horizontal"; length: number }) {
  const axis = p.orientation === "vertical" ? "y" : "x";
  const { dragging, handlers } = useValueGesture(p, axis, p.length);
  const norm = (p.value - p.min) / (p.max - p.min);
  const pct = `${norm * 100}%`;
  const vertical = p.orientation === "vertical";
  // Bipolar fills grow from the centre (0) instead of from the minimum.
  const start = `${(p.bipolar ? Math.min(norm, 0.5) : 0) * 100}%`;
  const size = `${(p.bipolar ? Math.abs(norm - 0.5) : norm) * 100}%`;
  return (
    <div className={`control fader ${p.orientation}`} {...handlers}>
      <div className="fader-track" style={vertical ? { height: p.length } : { width: p.length }}>
        <div
          className="fader-fill"
          style={
            vertical
              ? { bottom: start, height: size, background: `var(${p.color})` }
              : { left: start, width: size, background: `var(${p.color})` }
          }
        />
        <div className="fader-cap" style={vertical ? { bottom: pct } : { left: pct }} />
      </div>
      <span className="control-label">{p.label}</span>
      {dragging && <span className="control-tip num">{p.format(p.value)}</span>}
    </div>
  );
}

const METER_SEGMENTS = 18;
const METER_FLOOR_DB = -42;
const PEAK_HOLD_MS = 1500;

/** Segmented stereo meter: green to -6 dB, amber to 0 dB, red when clipping. */
export function Meter({ read, height }: { read: () => [number, number]; height: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = ref.current!;
    const ctx = canvas.getContext("2d")!;
    const dpr = window.devicePixelRatio || 1;
    const w = 14;
    canvas.width = w * dpr;
    canvas.height = height * dpr;
    ctx.scale(dpr, dpr);
    const colors = {
      ok: cssVar("--success"),
      warn: cssVar("--warning"),
      clip: cssVar("--danger"),
      off: cssVar("--border-subtle"),
    };
    const hold = [{ db: -Infinity, at: 0 }, { db: -Infinity, at: 0 }];
    let raf = 0;
    const segH = height / METER_SEGMENTS;
    const draw = () => {
      const now = performance.now();
      const peaks = read();
      ctx.clearRect(0, 0, w, height);
      for (let ch = 0; ch < 2; ch++) {
        const db = peaks[ch] > 0 ? 20 * Math.log10(peaks[ch]) : -Infinity;
        if (db >= hold[ch].db || now - hold[ch].at > PEAK_HOLD_MS) hold[ch] = { db, at: now };
        const x = ch * 7;
        for (let s = 0; s < METER_SEGMENTS; s++) {
          // Top segment is the clip light; the rest span the floor to 0 dB.
          const segDb = s === METER_SEGMENTS - 1 ? 0.01 : METER_FLOOR_DB + (s / (METER_SEGMENTS - 2)) * -METER_FLOOR_DB;
          const lit = db >= segDb || (hold[ch].db >= segDb && hold[ch].db < segDb + -METER_FLOOR_DB / (METER_SEGMENTS - 2));
          const zone = s === METER_SEGMENTS - 1 ? colors.clip : segDb > -6 ? colors.warn : colors.ok;
          ctx.fillStyle = lit ? zone : colors.off;
          ctx.fillRect(x, height - (s + 1) * segH + 1, 6, segH - 2);
        }
      }
      raf = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(raf);
  }, [read, height]);
  return <canvas ref={ref} className="meter" style={{ width: 14, height }} aria-hidden />;
}
