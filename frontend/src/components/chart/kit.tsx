// A small SVG chart kit: rows ("tracks") on one shared time axis with an instant, shared crosshair.
// Hover state lives outside React (`@/lib/store`); only the crosshair and readouts re-render on
// pointer moves, the plotted paths are memoized and redrawn only on resize or zoom.
import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { DAY, MIN, localMinutes, wallClock } from '@/lib/chart';
import { num } from '@/lib/format';
import { firstWeekday } from '@/lib/prefs';
import { createStore, useStore, type Store } from '@/lib/store';
import { cn } from '@/lib/utils';

export type Range = [number, number];
export type Mode = 'time' | 'date';

type Group = {
  cursor: Store<number | null>;
  view: Store<Range>;
  sel: Store<Range | null>;
  full: Range;
  mode: Mode;
  /** Time zone of a time axis; dates are UTC midnights (`dayValue`). */
  tz: string;
};

const Ctx = createContext<Group | null>(null);
export const useGroup = () => useContext(Ctx)!;

/** Tracks inside one group share the crosshair and the zoom. */
export function SyncGroup({ domain, mode, tz = 'UTC', children }: { domain: Range; mode: Mode; tz?: string; children: ReactNode }) {
  const g = useMemo<Group>(
    () => ({ cursor: createStore<number | null>(null), view: createStore(domain), sel: createStore<Range | null>(null), full: domain, mode, tz }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [domain[0], domain[1], mode, tz],
  );
  return <Ctx.Provider value={g}>{children}</Ctx.Provider>;
}

/** The cursor position, for readouts outside a track. */
export function useCursor() {
  return useStore(useGroup().cursor);
}

// ── Axis labels ──
const fDate = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'short', timeZone: 'UTC' });
const fDateW = new Intl.DateTimeFormat('en-GB', { weekday: 'short', day: 'numeric', month: 'short', timeZone: 'UTC' });
/** "9 Aug" for an x value of a date axis. */
export const date = (t: number) => fDate.format(t);
/** "Sun 9 Aug" for an x value of a date axis. */
export const dateW = (t: number) => fDateW.format(t);

/**
 * Tick positions: on whole local hours of `tz` (also across a clock change), or on
 * days, weekly ones on the first day of the user's week.
 */
export function timeTicks(d: Range, w: number, mode: Mode, tz: string): number[] {
  const span = d[1] - d[0];
  const max = Math.max(2, Math.floor(w / (mode === 'time' ? 60 : 80)));
  const out: number[] = [];
  if (mode === 'time') {
    const stepMin = [15, 30, 60, 120, 180, 240, 360, 720].find((m) => span / (m * MIN) <= max) ?? 720;
    const local = localMinutes(tz);
    const q = 15 * MIN;
    for (let t = Math.ceil(d[0] / q) * q; t <= d[1]; t += q) if (local(t) % stepMin === 0) out.push(t);
    return out;
  }
  const step = [1, 2, 7, 14, 28, 56].map((x) => x * DAY).find((s) => span / s <= max) ?? 56 * DAY;
  // 1970-01-01 was a Thursday (4); weekly ticks start on the user's first weekday.
  const t0 = step >= 7 * DAY ? (((firstWeekday() - 4) % 7) + 7) % 7 * DAY : 0;
  for (let t = Math.ceil((d[0] - t0) / step) * step + t0; t <= d[1]; t += step) out.push(t);
  return out;
}

// ── Scales ──
export type Scale = { x: (t: number) => number; y: (v: number) => number; t: (px: number) => number; w: number; h: number; d: Range; yd: Range };

export const M = { l: 36, r: 10 };
export const SIDE = 168;

function mkScale(d: Range, yd: Range, w: number, h: number, padTop = 6): Scale {
  const kx = w / (d[1] - d[0] || 1);
  const ky = (h - padTop) / (yd[1] - yd[0] || 1);
  return { x: (t) => (t - d[0]) * kx, y: (v) => h - (v - yd[0]) * ky, t: (px) => d[0] + px / kx, w, h, d, yd };
}

function useWidth() {
  const ref = useRef<HTMLDivElement>(null);
  const [w, setW] = useState(0);
  useEffect(() => {
    const ro = new ResizeObserver(([e]) => setW(Math.round(e.contentRect.width)));
    ro.observe(ref.current!);
    return () => ro.disconnect();
  }, []);
  return [ref, w] as const;
}

/** Index of the point nearest to `t` within `tol`, or -1. */
export function nearest(ts: number[], t: number, tol: number) {
  let lo = 0, hi = ts.length - 1;
  if (hi < 0) return -1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (ts[mid] < t) lo = mid + 1;
    else hi = mid;
  }
  const i = lo > 0 && Math.abs(ts[lo - 1] - t) < Math.abs(ts[lo] - t) ? lo - 1 : lo;
  return Math.abs(ts[i] - t) <= tol ? i : -1;
}

// ── Path builders ──
export function linePath(s: Scale, ts: number[], vs: (number | null)[], gap = Infinity) {
  let d = '';
  let pen = false;
  for (let i = 0; i < ts.length; i++) {
    const v = vs[i];
    if (v == null || (i && ts[i] - ts[i - 1] > gap)) pen = false;
    if (v == null) continue;
    d += `${pen ? 'L' : 'M'}${s.x(ts[i]).toFixed(1)},${s.y(v).toFixed(1)}`;
    pen = true;
  }
  return d;
}

export function bandPath(s: Scale, ts: number[], lo: (number | null)[], hi: (number | null)[], gap = Infinity) {
  let d = '';
  let run: number[] = [];
  const flush = () => {
    if (run.length > 1) {
      d += run.map((i, k) => `${k ? 'L' : 'M'}${s.x(ts[i]).toFixed(1)},${s.y(hi[i]!).toFixed(1)}`).join('');
      d += [...run].reverse().map((i) => `L${s.x(ts[i]).toFixed(1)},${s.y(lo[i]!).toFixed(1)}`).join('') + 'Z';
    }
    run = [];
  };
  for (let i = 0; i < ts.length; i++) {
    if (lo[i] == null || hi[i] == null || (run.length && ts[i] - ts[run.at(-1)!] > gap)) flush();
    if (lo[i] != null && hi[i] != null) run.push(i);
  }
  flush();
  return d;
}

/** Rounded-top bar path (one path for all bars is much cheaper than many rects). */
export function barsPath(s: Scale, ts: number[], vs: (number | null)[], width: number, base = 0, r = 2) {
  let d = '';
  const y0 = s.y(base);
  for (let i = 0; i < ts.length; i++) {
    const v = vs[i];
    if (v == null || v === base) continue;
    const x = s.x(ts[i]) - width / 2;
    const y = s.y(v);
    const h = y0 - y;
    const rr = Math.min(r, Math.abs(h), width / 2);
    if (h > 0) d += `M${x},${y0}V${y + rr}Q${x},${y} ${x + rr},${y}H${x + width - rr}Q${x + width},${y} ${x + width},${y + rr}V${y0}Z`;
    else d += `M${x},${y0}V${y - rr}Q${x},${y} ${x + rr},${y}H${x + width - rr}Q${x + width},${y} ${x + width},${y - rr}V${y0}Z`;
  }
  return d;
}

/** A dot on the crosshair; `t` puts it on its own data point instead of the cursor. */
export type Mark = { v: number | null | undefined; color: string; t?: number };

/**
 * One chart row. `draw` renders the static layers, `marks` the dots on the crosshair,
 * `readout` the text that follows the cursor (null when the pointer is elsewhere).
 * `points` (sorted) makes a chart of discrete values: the cursor jumps from one to the next.
 */
export function Track({
  title,
  height,
  y,
  ticks,
  tickFmt = (v) => num(v),
  domain,
  draw,
  marks,
  readout,
  side,
  className,
  grid = true,
  padTop = 6,
  overlay,
  points,
  help,
}: {
  title?: ReactNode;
  height: number;
  y: Range;
  ticks?: number[];
  tickFmt?: (v: number) => string;
  domain?: Range;
  draw: (s: Scale) => ReactNode;
  marks?: (t: number) => Mark[];
  readout?: (t: number | null) => ReactNode;
  side?: boolean;
  className?: string;
  grid?: boolean;
  /** Room above the plot, e.g. for markers that must not cover the data. */
  padTop?: number;
  /** Drawn with the crosshair, so it can follow the cursor. */
  overlay?: (s: Scale, t: number | null) => ReactNode;
  /** Shown after the title, e.g. a (?) explaining the chart. */
  help?: ReactNode;
  points?: number[];
}) {
  const g = useGroup();
  const view = useStore(g.view);
  const d = domain ?? view;
  const [ref, w] = useWidth();
  const pw = Math.max(0, w - M.l - M.r);
  const s = useMemo(() => mkScale(d, y, pw, height, padTop), [d, y[0], y[1], pw, height, padTop]); // eslint-disable-line react-hooks/exhaustive-deps
  const body = useMemo(() => (pw > 0 ? draw(s) : null), [s]); // eslint-disable-line react-hooks/exhaustive-deps
  const dragFrom = useRef<number | null>(null);
  const zoomable = !domain;
  const clipId = useMemo(() => `c${Math.random().toString(36).slice(2)}`, []);

  const at = (e: React.PointerEvent<SVGSVGElement>) => {
    const px = e.clientX - e.currentTarget.getBoundingClientRect().left - M.l;
    return { px, t: s.t(Math.min(pw, Math.max(0, px))) };
  };
  const pts = useMemo(() => points?.filter((t) => t >= d[0] && t <= d[1]), [points, d]);
  const snap = (t: number) => (pts?.length ? pts[nearest(pts, t, Infinity)] : g.mode === 'date' ? Math.round(t / DAY) * DAY : t);

  const header = (title || readout) && (
    <div className={cn('flex min-w-0 items-baseline gap-3', side ? 'mb-1 justify-between sm:mb-0 sm:flex-col sm:items-start sm:justify-start sm:gap-0.5 sm:pt-1 sm:pr-3' : 'mb-1 h-5 justify-between pl-[36px] pr-[10px]')}>
      <div className="flex items-center gap-1 text-[13px] font-medium text-ink-2">
        {title}
        {help}
      </div>
      {readout && <Readout fn={readout} big={side} d={d} />}
    </div>
  );

  return (
    <div className={cn(side && 'sm:grid sm:grid-cols-[var(--side)_1fr]', className)} style={{ ['--side' as string]: `${SIDE}px` }}>
      {header}
      <div ref={ref} className="min-w-0">
        <svg
          width={w}
          height={height}
          className="block touch-pan-y select-none overflow-visible rounded-sm"
          role="group"
          tabIndex={0}
          aria-label={typeof title === 'string' ? `${title}: arrow keys move the cursor` : undefined}
          onKeyDown={(e) => {
            if (e.key === 'Escape') return g.cursor.set(null);
            const dir = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
            if (!dir) return;
            e.preventDefault();
            const cur = g.cursor.get();
            if (pts?.length) {
              const i = cur == null ? -1 : nearest(pts, cur, Infinity);
              const j = cur == null || cur < pts[0] || cur > pts.at(-1)! ? (dir > 0 ? 0 : pts.length - 1) : pts[i] === cur ? i + dir : dir > 0 ? (pts[i] > cur ? i : i + 1) : pts[i] < cur ? i : i - 1;
              return g.cursor.set(pts[Math.min(pts.length - 1, Math.max(0, j))]);
            }
            const step = g.mode === 'date' ? DAY : (e.shiftKey ? 60 : 5) * MIN;
            // Days sit on whole-day marks inside the padded domain.
            const [lo, hi] = g.mode === 'date' ? [Math.ceil(d[0] / DAY) * DAY, Math.floor(d[1] / DAY) * DAY] : d;
            const next = cur == null || cur < lo || cur > hi ? (dir > 0 ? lo : hi) : cur + dir * step;
            g.cursor.set(Math.min(hi, Math.max(lo, next)));
          }}
          onBlur={() => g.cursor.set(null)}
          onPointerMove={(e) => {
            const { px, t } = at(e);
            g.cursor.set(px < -2 || px > pw + 2 ? null : snap(t));
            if (dragFrom.current != null) g.sel.set([Math.min(dragFrom.current, t), Math.max(dragFrom.current, t)]);
          }}
          onPointerLeave={() => dragFrom.current == null && g.cursor.set(null)}
          onPointerDown={(e) => {
            if (!zoomable || e.button !== 0) return;
            dragFrom.current = at(e).t;
            e.currentTarget.setPointerCapture(e.pointerId);
          }}
          onPointerUp={() => {
            const sel = g.sel.get();
            if (sel && s.x(sel[1]) - s.x(sel[0]) > 8) g.view.set(sel);
            g.sel.set(null);
            dragFrom.current = null;
          }}
          onDoubleClick={() => zoomable && g.view.set(g.full)}
        >
          <defs>
            <clipPath id={clipId}>
              <rect x={0} y={-2} width={pw} height={height + 4} />
            </clipPath>
          </defs>
          <g transform={`translate(${M.l},0)`}>
            {grid &&
              ticks?.map((v) => (
                <g key={v}>
                  <line x1={0} x2={pw} y1={s.y(v)} y2={s.y(v)} stroke="var(--grid)" strokeWidth={1} shapeRendering="crispEdges" />
                  <text x={-6} y={s.y(v)} dy="0.32em" textAnchor="end" className="fill-muted text-[10.5px] tabular-nums">
                    {tickFmt(v)}
                  </text>
                </g>
              ))}
            <g clipPath={`url(#${clipId})`}>{body}</g>
            <Crosshair s={s} marks={marks} overlay={overlay} />
          </g>
        </svg>
      </div>
    </div>
  );
}

/** The row's value at the cursor; a cursor outside the row's time range (e.g. in the night, on a day chart) reads as none. */
function Readout({ fn, big, d }: { fn: (t: number | null) => ReactNode; big?: boolean; d: Range }) {
  const c = useCursor();
  const t = c != null && c >= d[0] && c <= d[1] ? c : null;
  return <div className={cn('truncate tabular-nums text-ink', big ? 'text-[13px] [&_b]:text-lg [&_b]:font-semibold' : 'text-[13px] leading-5')}>{fn(t)}</div>;
}

function Crosshair({ s, marks, overlay }: { s: Scale; marks?: (t: number) => Mark[]; overlay?: (s: Scale, t: number | null) => ReactNode }) {
  const g = useGroup();
  const t = useStore(g.cursor);
  const sel = useStore(g.sel);
  const inside = t != null && t >= s.d[0] && t <= s.d[1];
  const x = inside ? Math.round(s.x(t)) + 0.5 : 0;
  return (
    <g pointerEvents="none">
      {overlay?.(s, inside ? t : null)}
      {sel && <rect x={s.x(sel[0])} width={s.x(sel[1]) - s.x(sel[0])} y={0} height={s.h} fill="var(--ink)" opacity={0.07} />}
      {inside && (
        <>
          <line x1={x} x2={x} y1={0} y2={s.h} stroke="var(--ink)" strokeOpacity={0.35} strokeWidth={1} />
          {marks?.(t).map((m, i) =>
            m.v == null ? null : <circle key={i} cx={m.t == null ? x : s.x(m.t)} cy={s.y(m.v)} r={3.5} fill={m.color} stroke="var(--page)" strokeWidth={1.5} />,
          )}
        </>
      )}
    </g>
  );
}

/** X-axis labels for the group; the cursor's time shows as a pill on it. */
export function TimeAxis({ domain, side, fmt }: { domain?: Range; side?: boolean; fmt?: (t: number) => string }) {
  const g = useGroup();
  const view = useStore(g.view);
  const t = useStore(g.cursor);
  const d = domain ?? view;
  const [ref, w] = useWidth();
  const pw = Math.max(0, w - M.l - M.r);
  const s = mkScale(d, [0, 1], pw, 20);
  const clock = useMemo(() => wallClock(g.tz), [g.tz]);
  const label = fmt ?? (g.mode === 'time' ? clock : date);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const all = useMemo(() => timeTicks(d, pw, g.mode, g.tz), [d[0], d[1], pw, g.mode, g.tz]);
  // Labels that would touch the previous one are left out (e.g. "02:00 CEST" and "02:00 CET" on a narrow axis).
  const ticks: number[] = [];
  for (const v of all) {
    const prev = ticks.at(-1);
    const room = prev == null ? 0 : ((label(prev).length + label(v).length) / 2) * 6.2 + 8;
    if (s.x(v) <= pw - 14 && (prev == null || s.x(v) - s.x(prev) >= room)) ticks.push(v);
  }
  const inside = t != null && t >= d[0] && t <= d[1];
  const zoomed = !domain && (view[0] !== g.full[0] || view[1] !== g.full[1]);
  return (
    <div className={cn(side && 'sm:grid sm:grid-cols-[var(--side)_1fr]')} style={{ ['--side' as string]: `${SIDE}px` }}>
      {side && (
        <div className="hidden pt-1 text-[11px] text-muted sm:block">
          {zoomed && (
            <button className="underline-offset-2 hover:text-ink hover:underline" onClick={() => g.view.set(g.full)}>
              Reset zoom
            </button>
          )}
        </div>
      )}
      <div ref={ref} className="relative h-6 min-w-0">
        <svg width={w} height={24} className="block">
          <g transform={`translate(${M.l},0)`}>
            <line x1={0} x2={pw} y1={0.5} y2={0.5} stroke="var(--axis)" />
            {ticks.map((v) => (
              <text key={v} x={s.x(v)} y={15} textAnchor="middle" className="fill-muted text-[10.5px] tabular-nums" opacity={inside && Math.abs(s.x(v) - s.x(t)) < 40 ? 0 : 1}>
                {label(v)}
              </text>
            ))}
          </g>
        </svg>
        {inside && (
          <div
            className="pointer-events-none absolute top-[3px] -translate-x-1/2 rounded-md bg-ink px-1.5 py-px text-[10.5px] font-medium tabular-nums text-page"
            style={{ left: M.l + s.x(t) }}
          >
            {g.mode === 'time' ? clock(t) : dateW(t)}
          </div>
        )}
        {!side && zoomed && (
          <button className="absolute right-[10px] top-6 text-[11px] text-muted underline-offset-2 hover:text-ink hover:underline" onClick={() => g.view.set(g.full)}>
            Reset zoom
          </button>
        )}
      </div>
    </div>
  );
}

/** Nice ticks for a value range (about `n` steps). */
export function niceTicks(lo: number, hi: number, n = 3): { y: Range; ticks: number[] } {
  const raw = (hi - lo) / n || 1;
  const p = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((f) => f * p).find((s) => s >= raw)!;
  const a = Math.floor(lo / step) * step;
  const b = Math.ceil(hi / step) * step;
  const ticks: number[] = [];
  for (let v = a; v <= b + step / 1e6; v += step) ticks.push(Math.round(v * 1e6) / 1e6);
  return { y: [a, b], ticks };
}

