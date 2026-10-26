import { useEffect, useMemo, useRef, type ReactNode } from 'react';
import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { ArrowDownRight, ArrowUpRight, RefreshCw } from 'lucide-react';
import { dayQuery, overviewQuery, syncQuery, type Daily, type DayDetail, type Exercise, type Stage } from '@/lib/api';
import { MIN, wallClock } from '@/lib/chart';
import { clock, duration, hm, num, temperature } from '@/lib/format';
import { before, mean, type MetricKey } from '@/lib/stats';
import { createStore, useStore } from '@/lib/store';
import { ExIcon, exerciseLabel } from '@/components/Activity';
import { Help } from '@/components/Help';
import { DateNav } from '@/components/DateNav';
import { FirstSync } from '@/components/FirstSync';
import { SyncGroup, TimeAxis, Track, bandPath, barsPath, linePath, nearest, niceTicks, useCursor, type Range, type Scale } from '@/components/chart/kit';
import { Button } from '@/components/ui/button';
import { Card, CardBody } from '@/components/ui/card';
import { InlineBar, Section } from '@/components/ui/section';
import { Skeleton } from '@/components/ui/skeleton';
import type { HelpKey } from '@/lib/help';
import { cn } from '@/lib/utils';

/** Days that have any measurement (the daily table also holds empty rows). */
export const withData = (days: Daily[]) => days.filter((d) => d.hr_avg != null || d.sleep_minutes != null || d.steps != null);

export function DayPage({ date: wanted }: { date?: string }) {
  const navigate = useNavigate();
  const overview = useQuery(overviewQuery);
  const sync = useQuery(syncQuery);
  const days = withData(overview.data?.days ?? []);
  const date = wanted && days.some((d) => d.date === wanted) ? wanted : days.at(-1)?.date;
  // Keep the previous day on screen while the next one loads, so the page doesn't jump.
  const day = useQuery({ ...dayQuery(date ?? ''), enabled: !!date, placeholderData: keepPreviousData });
  const queryClient = useQueryClient();
  const idx = date ? days.findIndex((d) => d.date === date) : -1;
  const prev = idx > 0 ? days[idx - 1].date : undefined;
  const next = idx >= 0 && idx < days.length - 1 ? days[idx + 1].date : undefined;
  const go = (d: string) => navigate({ to: '/', search: { date: d } });

  useEffect(() => {
    for (const d of [prev, next]) if (d) void queryClient.prefetchQuery(dayQuery(d));
  }, [prev, next, queryClient]);

  // ← / → step between days with data, unless a control (e.g. a chart or the theme toggle) uses the arrows itself.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
      if ((e.target as HTMLElement).closest('input, textarea, select, [role="group"], [role="radiogroup"], [role="dialog"]')) return;
      const to = e.key === 'ArrowLeft' ? prev : e.key === 'ArrowRight' ? next : undefined;
      if (!to) return;
      e.preventDefault();
      void navigate({ to: '/', search: { date: to } });
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [prev, next, navigate]);

  // When the first sync ends, its card stays until the data has reloaded, rather than "No data yet" in between.
  const syncing = !!sync.data?.running;
  const wasSyncing = useRef(false);
  const endedAt = useRef(0);
  if (wasSyncing.current && !syncing) endedAt.current = Date.now();
  wasSyncing.current = syncing;
  const reloading = overview.dataUpdatedAt < endedAt.current;

  if (overview.isError) return <ErrorCard onRetry={() => overview.refetch()} />;
  if (overview.isPending) return <DaySkeleton />;
  if (!date && overview.data.user.connected && (syncing || reloading)) {
    return <FirstSync progress={syncing ? sync.data?.progress : { step: 'tables' }} />;
  }
  if (!date) {
    return (
      <Card>
        <CardBody className="py-16 text-center text-sm text-muted">
          {overview.data.user.connected
            ? 'No data yet. Start a sync with the button in the top right.'
            : 'No data yet. It will appear here once you connect your Fitbit.'}
        </CardBody>
      </Card>
    );
  }

  return (
    <div className="space-y-8">
      <DateNav date={date} available={days.map((d) => d.date)} onChange={go} />

      <KpiStrip days={days} date={date} />

      {day.isError ? (
        <ErrorCard onRetry={() => day.refetch()} />
      ) : !day.data ? (
        <>
          <Skeleton className="h-[380px] rounded-xl" />
          <Skeleton className="h-[320px] rounded-xl" />
        </>
      ) : (
        <div className={cn('transition-opacity', day.isPlaceholderData && 'opacity-60')} aria-busy={day.isPlaceholderData}>
          <DayCharts key={day.data.date} day={day.data} />
        </div>
      )}
    </div>
  );
}

const Muted = ({ children }: { children: ReactNode }) => <span className="text-muted">{children}</span>;

// ── KPIs, grouped like the sections below ──

/** `better`: which direction of change is good (1 = up, -1 = down); unset for neutral metrics. */
type Kpi = { key: MetricKey; label: string; help: HelpKey; unit?: string; digits?: number; time?: boolean; better?: 1 | -1 };

/** `tint`: the tiles' wash on phones, cool for the night and green for the day like their charts. */
const GROUPS: { title: string; tint: string; items: Kpi[] }[] = [
  {
    title: 'Night',
    tint: 'bg-[color-mix(in_srgb,var(--stage-light)_11%,var(--surface))]',
    items: [
      { key: 'sleep_minutes', label: 'Asleep', help: 'sleep', time: true, better: 1 },
      { key: 'resting_hr', label: 'Resting HR', help: 'restingHr', unit: 'bpm', better: -1 },
      { key: 'hrv_ms', label: 'HRV', help: 'hrv', unit: 'ms', digits: 1, better: 1 },
      { key: 'spo2_avg', label: 'SpO₂', help: 'spo2', unit: '%', digits: 1 },
      { key: 'respiratory_rate', label: 'Breathing', help: 'breathing', unit: '/min', digits: 1 },
    ],
  },
  {
    title: 'Day',
    tint: 'bg-[color-mix(in_srgb,var(--s3)_11%,var(--surface))]',
    items: [
      { key: 'steps', label: 'Steps', help: 'steps', better: 1 },
      { key: 'azm', label: 'Zone minutes', help: 'azm', unit: 'min', better: 1 },
      { key: 'active_kcal', label: 'Active kcal', help: 'kcal', unit: 'kcal' },
    ],
  },
];

/**
 * The day's key numbers as tiles, the first of each group (sleep, steps) twice
 * as wide. Phones: two to a row. Wider: the night in three columns and the day
 * in four, or beside the night in two thirds and one third, both two rows high.
 */
function KpiStrip({ days, date }: { days: Daily[]; date: string }) {
  const today = days.find((d) => d.date === date)!;
  return (
    <div className="grid gap-x-8 gap-y-6 lg:grid-cols-[2fr_1fr]">
      {GROUPS.map((gr) => (
        <div key={gr.title}>
          <div className="mb-2 flex items-center gap-1 text-[11px] font-medium uppercase tracking-wider text-muted">
            {gr.title}
            {gr.title === 'Night' && <Help topic="kpis" />}
          </div>
          {/* Night: 5 tiles, sleep twice as wide, fill three columns. Day: 3 fill a row of four, or two rows of two beside the night. */}
          <div className={cn('grid grid-cols-2 gap-2', gr.title === 'Night' ? 'sm:grid-cols-3' : 'sm:grid-cols-4 lg:grid-cols-2')}>
            {gr.items.map((k, i) => {
              const v = today[k.key];
              const fmt = (x: number) => (k.time ? hm(x) : num(x, k.digits ?? 0));
              const wide = i === 0;
              return (
                <div key={k.key} className={cn('rounded-2xl p-3', gr.tint, wide && 'col-span-2')}>
                  <div className="flex items-center gap-1 text-xs text-ink-2">
                    {k.label}
                    <Help topic={k.help} />
                  </div>
                  <div className="flex items-end justify-between gap-3">
                    <div className={cn('font-semibold leading-tight tracking-tight tabular-nums', wide ? 'text-[28px]' : 'text-[22px]')}>
                      {v == null ? '–' : fmt(v)}
                      {v != null && (k.time || k.unit) && <span className="ml-0.5 text-xs font-normal text-ink-2">{k.time ? 'h' : k.unit}</span>}
                    </div>
                    {k.key === 'sleep_minutes' && <SleepBar d={today} />}
                  </div>
                  <Delta k={k} v={v} avg={mean(before(days, date, k.key, 7))} fmt={fmt} />
                </div>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}

/** The night's stages as one bar, with bedtime and wake time, beside the hours asleep. */
function SleepBar({ d }: { d: Daily }) {
  const parts = [
    [d.sleep_deep_minutes, 'var(--stage-deep)'],
    [d.sleep_light_minutes, 'var(--stage-light)'],
    [d.sleep_rem_minutes, 'var(--stage-rem)'],
    [d.sleep_awake_minutes, 'var(--stage-awake)'],
  ] as const;
  const total = parts.reduce((a, [m]) => a + (m ?? 0), 0);
  if (!total || !d.sleep_start || !d.sleep_end) return null;
  return (
    <div className="mb-1.5 w-[55%] max-w-56" aria-hidden>
      <div className="flex h-2 gap-0.5 overflow-hidden rounded-full">
        {parts.map(([m, c], i) => (m ? <span key={i} style={{ width: `${(m / total) * 100}%`, background: c }} /> : null))}
      </div>
      <div className="mt-1 flex justify-between text-[11px] tabular-nums text-muted">
        <span>{clock(d.sleep_start)}</span>
        <span>{clock(d.sleep_end)}</span>
      </div>
    </div>
  );
}

function Delta({ k, v, avg, fmt }: { k: Kpi; v: number | null; avg: number | null; fmt: (x: number) => string }) {
  if (v == null || avg == null) return <div className="h-4" />;
  const diff = v - avg;
  // A change that rounds to nothing reads better as "= avg" than as "+0:00" or "−0".
  if (!/[1-9]/.test(fmt(Math.abs(diff)))) return <div className="h-4 text-[11.5px] text-muted">= avg</div>;
  // Changes within 5 % of the average are noise, not worth a color.
  const tone = !k.better || Math.abs(diff) < Math.abs(avg) * 0.05 ? 'text-muted' : Math.sign(diff) === k.better ? 'text-good' : 'text-critical';
  const Arrow = diff >= 0 ? ArrowUpRight : ArrowDownRight;
  return (
    <div
      className={cn('flex h-4 items-center gap-0.5 text-[11.5px] tabular-nums', tone)}
      title={tone === 'text-good' ? 'Better than average' : tone === 'text-critical' ? 'Worse than average' : undefined}
    >
      <Arrow className="size-3" />
      {diff >= 0 ? '+' : '−'}
      {fmt(Math.abs(diff))}
    </div>
  );
}

// ── Charts ──

/** Stages bottom to top; restless (short awakenings in light sleep, as in Google Health) gets a row when present. */
const LANES: { key: Stage | 'RESTLESS'; label: string; color: string }[] = [
  { key: 'DEEP', label: 'Deep', color: 'var(--stage-deep)' },
  { key: 'LIGHT', label: 'Light', color: 'var(--stage-light)' },
  { key: 'REM', label: 'REM', color: 'var(--stage-rem)' },
  { key: 'RESTLESS', label: 'Restless', color: 'color-mix(in srgb, var(--stage-awake) 55%, var(--page))' },
  { key: 'AWAKE', label: 'Awake', color: 'var(--stage-awake)' },
];

type Span = { start: number; end: number };
type Ex = Span & { e: Exercise; label: string };

/** The API's day in numbers: instants as ms, rows as columns. */
function prepare(day: DayDetail) {
  const P = Date.parse;
  const night = day.sleep && {
    ...day.sleep,
    span: { start: P(day.sleep.start), end: P(day.sleep.end) },
    stages: day.sleep.stages.map((s) => ({ start: P(s.start), end: P(s.end), key: s.stage as Stage | 'RESTLESS' })),
    restless: (day.sleep.restless ?? []).map((s) => ({ start: P(s.start), end: P(s.end), key: 'RESTLESS' as const })),
  };
  const midnight = P(day.midnight);
  const end = P(day.range[1]);
  return {
    clock: wallClock(day.timeZone),
    hr: { t: day.heartRate.map((r) => P(r[0])), lo: day.heartRate.map((r) => r[1]), avg: day.heartRate.map((r) => r[2]), hi: day.heartRate.map((r) => r[3]) },
    steps: { t: day.steps.map((s) => P(s[0])), v: day.steps.map((s) => s[1]) },
    spo2: { t: day.spo2.map((s) => P(s[0])), v: day.spo2.map((s) => s[1]) },
    hrv: { t: day.hrv.map((s) => P(s[0])), v: day.hrv.map((s) => s[1]) },
    night,
    nightRange: night ? ([night.span.start - 10 * MIN, night.span.end + 10 * MIN] as Range) : null,
    // The day starts when the night ends (or at midnight without one).
    dayRange: [night && night.span.end > midnight && night.span.end < end ? night.span.end : P(day.range[0]), end] as Range,
    exercises: day.exercises.map((e): Ex => ({ e, label: exerciseLabel(e), start: P(e.start), end: P(e.end) })),
  };
}
type Prepared = ReturnType<typeof prepare>;

/** Values between `a` and `b`, for the y-range of one part of the day. */
const within = (ts: number[], vs: (number | null)[], [a, b]: Range) => vs.filter((v, i): v is number => v != null && ts[i] >= a && ts[i] <= b);

function DayCharts({ day }: { day: DayDetail }) {
  const p = useMemo(() => prepare(day), [day]);
  return (
    <SyncGroup domain={[Date.parse(day.range[0]), Date.parse(day.range[1])]} mode="time" tz={day.timeZone}>
      <NightSection p={p} m={day.metrics} />
      <DaySection p={p} m={day.metrics} className="mt-10" />
    </SyncGroup>
  );
}

function seriesTrack(ts: number[], vs: number[], color: string, gap: number, dots = false) {
  return {
    draw: (s: Scale) =>
      dots ? (
        <>{ts.map((t, i) => <circle key={t} cx={s.x(t)} cy={s.y(vs[i])} r={1.6} fill={color} />)}</>
      ) : (
        <path d={linePath(s, ts, vs, gap)} fill="none" stroke={color} strokeWidth={1.25} strokeLinejoin="round" />
      ),
    marks: (t: number) => {
      const i = nearest(ts, t, gap);
      return [{ v: i < 0 ? null : vs[i], color, t: ts[i] }];
    },
    points: dots ? ts : undefined,
    value: (t: number | null) => {
      const i = t == null ? -1 : nearest(ts, t, gap);
      return i < 0 ? null : vs[i];
    },
  };
}

function Stat({ label, value, hint, help }: { label: string; value: ReactNode; hint?: string; help?: HelpKey }) {
  return (
    <div className="flex items-baseline justify-between gap-3 border-b py-2 text-[13px] last:border-0">
      <span className="flex items-center gap-1 text-ink-2">
        {label}
        {help && <Help topic={help} />}
      </span>
      <span className="text-right tabular-nums">
        {value}
        {hint && <span className="ml-1.5 text-xs text-muted">{hint}</span>}
      </span>
    </div>
  );
}

function NightSection({ p, m }: { p: Prepared; m: Daily | null }) {
  const { night, nightRange: range, clock } = p;
  if (!night || !range) {
    return (
      <Section title="Night">
        <p className="py-8 text-center text-sm text-muted">No sleep recorded for the night before this day.</p>
      </Section>
    );
  }
  const lanes = LANES.filter((l) => l.key !== 'RESTLESS' || night.restless.length);
  const lane = Object.fromEntries(lanes.map((l, i) => [l.key, i]));
  const color = Object.fromEntries(LANES.map((l) => [l.key, l.color]));
  const label = Object.fromEntries(LANES.map((l) => [l.key, l.label]));
  const blocks = [...night.stages, ...night.restless];
  const asleep = night.minutes_asleep ?? 0;
  const inBed = (night.span.end - night.span.start) / MIN;
  const spo2 = seriesTrack(p.spo2.t, p.spo2.v, 'var(--s1)', 5 * MIN);
  const hrv = seriesTrack(p.hrv.t, p.hrv.v, 'var(--s7)', 10 * MIN, true);
  const hrAsleep = within(p.hr.t, p.hr.avg, range);
  const hrY = niceTicks(Math.min(...hrAsleep, 45), Math.max(...hrAsleep, 65), 2);
  const spo2Y = niceTicks(Math.min(...p.spo2.v, 92), 100, 2);
  const hrvY = niceTicks(0, Math.max(...p.hrv.v, 40), 2);

  return (
    <Section title="Night" note={`${clock(night.span.start)} – ${clock(night.span.end)}, the night that ended this morning`}>
      <div className="grid grid-cols-[minmax(0,1fr)] gap-8 lg:grid-cols-[minmax(0,1fr)_280px]">
        <div className="space-y-4">
          <Track
            title="Sleep stages" help={<Help topic="stages" />} height={lanes.length * 20 + 12} y={[0, lanes.length]} ticks={lanes.map((_, i) => i + 0.5)} tickFmt={(v) => lanes[Math.floor(v)].label}
            domain={range}
            draw={(s) => {
              const h = s.y(0) - s.y(1);
              return blocks.map((b, i) => (
                <rect key={i} x={s.x(b.start)} y={s.y(lane[b.key] + 1) + h * 0.16} width={Math.max(1, s.x(b.end) - s.x(b.start) - 0.5)} height={h * 0.68} rx={1.5} fill={color[b.key]} />
              ));
            }}
            readout={(t) => {
              const b = t == null ? undefined : (night.restless.find((r) => r.start <= t && t < r.end) ?? night.stages.find((r) => r.start <= t && t < r.end));
              if (!b) return <><b>{duration(asleep)}</b> asleep</>;
              return (
                <>
                  <i className="mr-1.5 inline-block size-2 rounded-full align-[1px]" style={{ background: color[b.key] }} />
                  <span className="font-medium text-ink-2">{label[b.key]}</span> <Muted>{clock(b.start)}–{clock(b.end)}</Muted>
                </>
              );
            }}
          />
          <Track
            title="Heart rate" help={<Help topic="nightHr" />} height={70} {...hrY} domain={range}
            marks={(t) => {
              const i = nearest(p.hr.t, t, 2 * MIN);
              return [{ v: i < 0 ? null : p.hr.avg[i], color: 'var(--s5)' }];
            }}
            draw={(s) => <path d={linePath(s, p.hr.t, p.hr.avg, 5 * MIN)} fill="none" stroke="var(--s5)" strokeWidth={1.25} strokeLinejoin="round" />}
            readout={(t) => {
              const i = t == null ? -1 : nearest(p.hr.t, t, 2 * MIN);
              return i < 0 ? <><b>{num(m?.resting_hr)}</b> bpm resting</> : <><b>{num(p.hr.avg[i])}</b> bpm</>;
            }}
          />
          {p.spo2.v.length > 0 && (
            <Track
              title="SpO₂" help={<Help topic="spo2" />} height={50} {...spo2Y} domain={range} draw={spo2.draw} marks={spo2.marks}
              readout={(t) => (spo2.value(t) == null ? <><b>{num(m?.spo2_avg, 1)}</b> % avg</> : <><b>{num(spo2.value(t), 1)}</b> %</>)}
            />
          )}
          {p.hrv.v.length > 0 && (
            <Track
              title="HRV" help={<Help topic="hrv" />} height={50} {...hrvY} domain={range} draw={hrv.draw} marks={hrv.marks} points={hrv.points}
              readout={(t) => (hrv.value(t) == null ? <><b>{num(m?.hrv_ms, 1)}</b> ms avg</> : <><b>{num(hrv.value(t), 1)}</b> ms</>)}
            />
          )}
          <TimeAxis domain={range} />
        </div>
        {/* Beside the charts on wide screens; under them, in two columns, on tablets. */}
        <div className="grid items-start gap-x-8 md:grid-cols-2 lg:block">
          <div>
          <Stat label="Fell asleep" value={clock(night.span.start)} />
          <Stat label="Woke up" value={clock(night.span.end)} />
          <Stat label="Asleep" value={duration(asleep)} />
          <Stat label="Efficiency" help="efficiency" value={`${num((asleep / inBed) * 100)} %`} hint={`of ${duration(inBed)}`} />
          <Stat label="Heart rate asleep" help="hrAsleep" value={m?.nonrem_hr != null ? `${num(m.nonrem_hr)} bpm` : '–'} />
          <Stat label="Skin temperature" help="skinTemp" value={temperature(m?.nightly_temp_c)} />
          </div>
          <StageTable night={night} />
        </div>
      </div>
    </Section>
  );
}

function StageTable({ night }: { night: NonNullable<Prepared['night']> }) {
  const rows = LANES.filter((l) => l.key !== 'RESTLESS').reverse();
  const total = rows.reduce((a, l) => a + (night.stage_minutes[l.key as Stage] ?? 0), 0) || 1;
  const max = Math.max(...rows.map((l) => night.stage_minutes[l.key as Stage] ?? 0));
  return (
    <table className="mt-5 w-full text-[13px] tabular-nums md:mt-0 lg:mt-5">
      <thead>
        <tr className="border-b text-xs text-ink-2">
          <th className="py-2 text-left font-medium">Stage</th>
          <th className="w-[40%] px-3 py-2 text-left font-medium">Time</th>
          <th />
          <th className="py-2 text-right font-medium">Share</th>
        </tr>
      </thead>
      <tbody>
        {[...rows].reverse().map((l) => {
          const v = night.stage_minutes[l.key as Stage] ?? 0;
          return (
            <tr key={l.key} className="border-b last:border-0">
              <td className="py-2">{l.label}</td>
              <td className="px-3 py-2">
                <InlineBar value={v} max={max} color={l.color} />
              </td>
              <td className="whitespace-nowrap py-2 text-right">{hm(v)} h</td>
              <td className="w-12 py-2 text-right text-ink-2">{num((v / total) * 100)} %</td>
            </tr>
          );
        })}
        {night.restless_minutes != null && night.restless.length > 0 && (
          <tr title="Short awakenings during light sleep; counted as light sleep.">
            <td className="py-2">Restless</td>
            <td className="px-3 py-2" />
            <td className="whitespace-nowrap py-2 text-right">{hm(night.restless_minutes)} h</td>
            <td className="w-12 py-2 text-right text-ink-2">{night.restless.length}×</td>
          </tr>
        )}
      </tbody>
    </table>
  );
}

// ── Exercise: icons in a lane above the heart rate (so nothing covers the line), a faint band below ──

const LANE_H = 24;
/** The activity hovered in the list, which lights up its band in the charts. */
const exFocus = createStore<Ex | null>(null);

/** Icon centers: the middle of each activity, pushed right where two would touch. */
function iconXs(s: Scale, exs: Ex[]) {
  const xs: number[] = [];
  for (const e of exs) xs.push(Math.max((s.x(e.start) + s.x(e.end)) / 2, (xs.at(-1) ?? -Infinity) + 22));
  return xs;
}

function exBands(s: Scale, exs: Ex[], lane: boolean) {
  const top = lane ? LANE_H : 0;
  return (
    <>
      {exs.map((e) => (
        <rect key={e.start} x={s.x(e.start)} width={s.x(e.end) - s.x(e.start)} y={top} height={s.h - top} fill="var(--ex-band)" />
      ))}
      {lane && iconXs(s, exs).map((x, i) => <ExIcon key={i} e={exs[i].e} x={x - 9} y={LANE_H / 2 - 10} width={18} height={18} />)}
    </>
  );
}

/** The band under the cursor, or of the activity hovered in the list, darkens. */
function ExHighlight({ s, at, lane }: { s: Scale; at: Ex | undefined; lane: boolean }) {
  const e = useStore(exFocus) ?? at;
  const top = lane ? LANE_H : 0;
  return e ? <rect x={s.x(e.start)} width={s.x(e.end) - s.x(e.start)} y={top} height={s.h - top} fill="var(--ex-band)" /> : null;
}

/** An activity in the list: lit while the cursor is over it in a chart, and lights the chart when hovered. */
function ExRow({ x, clock, exs }: { x: Ex; clock: (t: number) => string; exs: Ex[] }) {
  const focus = useStore(exFocus);
  const t = useCursor();
  const lit = focus === x || (t != null && exs.find((e) => e.start <= t && t < e.end) === x);
  const mins = x.e.active_minutes ?? (x.end - x.start) / MIN;
  return (
    <div
      className="-mx-2 flex items-start gap-3 rounded-lg px-2 py-1.5 text-[13px]"
      style={{ background: lit ? 'linear-gradient(var(--ex-band), var(--ex-band)), linear-gradient(var(--ex-band), var(--ex-band))' : undefined }}
      onMouseEnter={() => exFocus.set(x)}
      onMouseLeave={() => exFocus.set(null)}
    >
      <ExIcon e={x.e} className="mt-0.5 size-5" />
      <div className="min-w-0 flex-1">
        <div className="flex justify-between gap-2">
          <span className="truncate font-medium">{x.label}</span>
          <span className="shrink-0 tabular-nums text-ink-2">{duration(mins)}</span>
        </div>
        <div className="text-xs tabular-nums text-muted">
          {clock(x.start)}–{clock(x.end)}
          {x.e.avg_hr != null && ` · avg ${num(x.e.avg_hr)} bpm`}
          {x.e.kcal != null && ` · ${num(x.e.kcal)} kcal`}
          {x.e.azm ? ` · ${num(x.e.azm)} AZM` : ''}
        </div>
      </div>
    </div>
  );
}

function DaySection({ p, m, className }: { p: Prepared; m: Daily | null; className?: string }) {
  const { dayRange: range, clock, exercises: exs } = p;
  useEffect(() => () => exFocus.set(null), []);
  const exAt = (t: number | null) => (t == null ? undefined : exs.find((e) => e.start <= t && t < e.end));
  const stepAt = (t: number) => p.steps.t.findIndex((b) => b <= t && t < b + 15 * MIN);
  const stepMids = useMemo(() => p.steps.t.map((t) => t + 7.5 * MIN), [p]);
  const hrIn = [...within(p.hr.t, p.hr.lo, range), ...within(p.hr.t, p.hr.hi, range)];
  const hrY = niceTicks(Math.min(...hrIn, 50), Math.max(...hrIn, 100), 3);
  const stepsY = niceTicks(0, Math.max(...within(p.steps.t, p.steps.v, range), 500), 1);
  const total = p.steps.v.reduce((a, b) => a + b, 0);

  return (
    <Section title="Day" note={`${clock(range[0])} – midnight`} className={className}>
      <div className="grid grid-cols-[minmax(0,1fr)] gap-8 lg:grid-cols-[minmax(0,1fr)_280px]">
        <div className="space-y-4">
          <Track
            title="Heart rate" help={<Help topic="dayHr" />} height={170 + LANE_H} padTop={LANE_H + 4} {...hrY} domain={range}
            draw={(s) => (
              <>
                {exBands(s, exs, true)}
                <path d={bandPath(s, p.hr.t, p.hr.lo, p.hr.hi, 5 * MIN)} fill="var(--band)" opacity={0.6} />
                <path d={linePath(s, p.hr.t, p.hr.avg, 5 * MIN)} fill="none" stroke="var(--s1)" strokeWidth={1.25} strokeLinejoin="round" />
              </>
            )}
            overlay={(s, t) => <ExHighlight s={s} at={exAt(t)} lane />}
            marks={(t) => {
              const i = nearest(p.hr.t, t, 2 * MIN);
              return [{ v: i < 0 ? null : p.hr.avg[i], color: 'var(--s1)' }];
            }}
            readout={(t) => {
              const i = t == null ? -1 : nearest(p.hr.t, t, 2 * MIN);
              if (i < 0) return <><b>{num(m?.hr_avg)}</b> bpm avg <Muted>· {num(m?.hr_min)}–{num(m?.hr_max)}</Muted></>;
              const e = exAt(t);
              return (
                <>
                  {e && <Muted>{e.label} {clock(e.start)}–{clock(e.end)} · </Muted>}
                  <b>{num(p.hr.avg[i])}</b> bpm <Muted>· {num(p.hr.lo[i])}–{num(p.hr.hi[i])}</Muted>
                </>
              );
            }}
          />
          <Track
            title="Steps per 15 min" help={<Help topic="steps" />} height={64} {...stepsY} tickFmt={(v) => (v >= 1000 ? `${num(v / 1000, v % 1000 ? 1 : 0)}k` : num(v))} domain={range}
            draw={(s) => (
              <>
                {exBands(s, exs, false)}
                <path d={barsPath(s, stepMids, p.steps.v, Math.max(1, (s.x(15 * MIN) - s.x(0)) * 0.7), 0, 1.5)} fill="var(--s3)" />
              </>
            )}
            overlay={(s, t) => <ExHighlight s={s} at={exAt(t)} lane={false} />}
            points={stepMids}
            marks={(t) => {
              const i = stepAt(t);
              return [{ v: i < 0 ? null : p.steps.v[i], color: 'var(--s3)', t: stepMids[i] }];
            }}
            readout={(t) => {
              const i = t == null ? -1 : stepAt(t);
              if (i < 0) return <><b>{num(m?.steps ?? total)}</b> steps</>;
              return <><b>{num(p.steps.v[i])}</b> <Muted>{clock(p.steps.t[i])}–{clock(p.steps.t[i] + 15 * MIN)}</Muted></>;
            }}
          />
          <TimeAxis domain={range} />
        </div>
        <div className="grid items-start gap-x-8 md:grid-cols-2 lg:block">
          <div>
          <div className="mb-1 text-xs text-muted">Heart rate, whole day</div>
          <Stat label="Lowest" value={m?.hr_min != null ? `${num(m.hr_min)} bpm` : '–'} />
          <Stat label="Average" value={m?.hr_avg != null ? `${num(m.hr_avg)} bpm` : '–'} />
          <Stat label="Highest" value={m?.hr_max != null ? `${num(m.hr_max)} bpm` : '–'} />
          </div>
          {exs.length > 0 && (
            <div className="mt-5 space-y-1 md:mt-0 lg:mt-5">
              {exs.map((x) => <ExRow key={x.start} x={x} clock={clock} exs={exs} />)}
            </div>
          )}
        </div>
      </div>
    </Section>
  );
}

export function ErrorCard({ onRetry }: { onRetry: () => void }) {
  return (
    <Card>
      <CardBody className="flex flex-col items-center gap-3 py-12 text-center">
        <p className="text-sm text-ink-2">Could not load the data.</p>
        <Button variant="outline" size="sm" onClick={onRetry}>
          <RefreshCw /> Try again
        </Button>
      </CardBody>
    </Card>
  );
}

function DaySkeleton() {
  return (
    <div className="space-y-10">
      <div className="flex justify-between">
        <Skeleton className="h-7 w-40" />
        <Skeleton className="h-8 w-64" />
      </div>
      <Skeleton className="h-[72px] rounded-xl" />
      <Skeleton className="h-[380px] rounded-xl" />
    </div>
  );
}
