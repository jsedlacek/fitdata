// Trends: full-width rows with labels on the side, grouped like the day page into Night and Day.
import { useMemo, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link, useNavigate } from '@tanstack/react-router';
import { overviewQuery, type Daily } from '@/lib/api';
import { DAY, dayValue } from '@/lib/chart';
import { hhmm, hm, num } from '@/lib/format';
import { prefs } from '@/lib/prefs';
import { mean as meanOf, rolling } from '@/lib/stats';
import { useStore } from '@/lib/store';
import { SyncGroup, TimeAxis, Track, barsPath, dateW, linePath, niceTicks, useGroup, type Range, type Scale } from '@/components/chart/kit';
import { Help } from '@/components/Help';
import { InlineBar, Section } from '@/components/ui/section';
import { Segmented } from '@/components/ui/segmented';
import { Skeleton } from '@/components/ui/skeleton';
import type { HelpKey } from '@/lib/help';
import { cn } from '@/lib/utils';
import { ErrorCard, withData } from './day';

/** Days shown. A number, so the router writes `?range=90` (search params are JSON: the string '90' would be `"90"`). */
export type Period = 7 | 30 | 90 | 'all';

/** One day as the charts use it: x value, sleep as minutes from the wake day's midnight. */
type Row = {
  t: number;
  date: string;
  sleep: number | null;
  deep: number | null;
  light: number | null;
  rem: number | null;
  awake: number | null;
  /** Minutes from midnight; bedtime is negative when before midnight. */
  bedtime: number | null;
  wake: number | null;
  rhr: number | null;
  hrv: number | null;
  spo2: number | null;
  breath: number | null;
  /** Change from the mean of all nights, in the user's units. */
  temp: number | null;
  steps: number | null;
  azm: number | null;
  kcal: number | null;
};

const mean = (vs: (number | null)[]) => meanOf(vs.filter((v): v is number => v != null));

/** A local time from the API ("2026-10-24T23:14:00+02:00") as minutes from midnight of `date`. */
function minutesFrom(date: string, at: string | null) {
  if (!at) return null;
  const m = Number(at.slice(11, 13)) * 60 + Number(at.slice(14, 16));
  return (dayValue(at.slice(0, 10)) - dayValue(date)) / 60_000 + m;
}

function toRows(days: Daily[]): Row[] {
  const tempMean = mean(days.map((d) => d.nightly_temp_c));
  const f = prefs().units === 'imperial' ? 1.8 : 1;
  return days.map((d) => ({
    t: dayValue(d.date),
    date: d.date,
    sleep: d.sleep_minutes,
    deep: d.sleep_deep_minutes,
    light: d.sleep_light_minutes,
    rem: d.sleep_rem_minutes,
    awake: d.sleep_awake_minutes,
    bedtime: minutesFrom(d.date, d.sleep_start),
    wake: minutesFrom(d.date, d.sleep_end),
    rhr: d.resting_hr,
    hrv: d.hrv_ms,
    spo2: d.spo2_avg,
    breath: d.respiratory_rate,
    temp: d.nightly_temp_c != null && tempMean != null ? (d.nightly_temp_c - tempMean) * f : null,
    steps: d.steps,
    azm: d.azm,
    kcal: d.active_kcal,
  }));
}

const STAGES = [
  { key: 'awake', label: 'Awake', color: 'var(--stage-awake)' },
  { key: 'rem', label: 'REM', color: 'var(--stage-rem)' },
  { key: 'light', label: 'Light', color: 'var(--stage-light)' },
  { key: 'deep', label: 'Deep', color: 'var(--stage-deep)' },
] as const;

/** Minutes from midnight as a wall-clock time, in the user's clock. */
const clock = (m: number) => {
  const x = ((Math.round(m) % 1440) + 1440) % 1440;
  return hhmm(`${String(Math.floor(x / 60)).padStart(2, '0')}:${String(x % 60).padStart(2, '0')}`);
};

type Key = 'rhr' | 'hrv' | 'spo2' | 'breath' | 'temp' | 'steps' | 'azm' | 'kcal';
type Metric = { key: Key; title: string; help: HelpKey; unit: string; digits?: number; kind: 'line' | 'bar' | 'dev'; color: string; goal?: number; height?: number };

const M: Record<Key, Metric> = {
  rhr: { key: 'rhr', title: 'Resting heart rate', help: 'restingHr', unit: 'bpm', kind: 'line', color: 'var(--s5)' },
  hrv: { key: 'hrv', title: 'HRV', help: 'hrv', unit: 'ms', digits: 1, kind: 'line', color: 'var(--s7)' },
  spo2: { key: 'spo2', title: 'SpO₂', help: 'spo2', unit: '%', digits: 1, kind: 'line', color: 'var(--s1)', height: 48 },
  breath: { key: 'breath', title: 'Breathing rate', help: 'breathing', unit: '/min', digits: 1, kind: 'line', color: 'var(--s3)', height: 48 },
  temp: { key: 'temp', title: 'Skin temp. change', help: 'skinTempChange', unit: '°', digits: 1, kind: 'dev', color: 'var(--s2)', height: 48 },
  steps: { key: 'steps', title: 'Steps', help: 'steps', unit: '', kind: 'bar', color: 'var(--s3)', goal: 10000 },
  // 150 Active Zone Minutes a week, the WHO's and Fitbit's default goal.
  azm: { key: 'azm', title: 'Zone minutes', help: 'azm', unit: 'min', kind: 'bar', color: 'var(--s2)', height: 48, goal: 22 },
  kcal: { key: 'kcal', title: 'Active kcal', help: 'kcal', unit: 'kcal', kind: 'bar', color: 'var(--s4)', height: 48 },
};

const k = (v: number) => (v >= 1000 ? `${num(v / 1000, v % 1000 ? 1 : 0)}k` : num(v));
const Muted = ({ children }: { children: ReactNode }) => <span className="text-muted">{children}</span>;

/** Data extent with a little room (at least ±3 % of the mean), ticks only inside it. */
function tight(vs: number[], mu: number) {
  const lo = Math.min(...vs), hi = Math.max(...vs);
  const pad = Math.max((hi - lo) * 0.12, Math.abs(mu) * 0.03);
  const y: Range = [lo - pad, hi + pad];
  return { y, ticks: niceTicks(y[0], y[1], 2).ticks.filter((t) => t > y[0] && t < y[1]) };
}

function useRows(days: Row[]) {
  return useMemo(() => {
    const idx = new Map(days.map((d, i) => [d.t, i]));
    const domain: Range = [days[0].t - DAY / 2, days.at(-1)!.t + DAY / 2];
    return { idx, domain, ts: days.map((d) => d.t) };
  }, [days]);
}

const barW = (s: Scale, gap = 0.3) => Math.max(1, (s.x(DAY) - s.x(0)) * (1 - gap));

function metricTrack(m: Metric, days: Row[], idx: Map<number, number>, avgLabel: string) {
  const ts = days.map((d) => d.t);
  const vs = days.map((d) => d[m.key]);
  const avg7 = rolling(vs, 7);
  const present = vs.filter((v): v is number => v != null);
  const mu = mean(present) ?? 0;
  const { y, ticks } =
    m.kind === 'line'
      ? tight(present, mu)
      : m.kind === 'dev'
        ? { y: [-1.2, 1.2] as Range, ticks: [-1, 0, 1] }
        : niceTicks(0, Math.max(...present, m.goal ?? 0), 2);
  const draw = (s: Scale) =>
    m.kind === 'line' ? (
      <>
        <path d={linePath(s, ts, vs, DAY * 1.5)} fill="none" stroke={m.color} strokeOpacity={0.45} strokeWidth={1} />
        {days.length <= 45 && ts.map((t, i) => (vs[i] == null ? null : <circle key={t} cx={s.x(t)} cy={s.y(vs[i]!)} r={2} fill={m.color} />))}
        <path d={linePath(s, ts, avg7)} fill="none" stroke={m.color} strokeWidth={2} strokeLinejoin="round" />
      </>
    ) : m.kind === 'dev' ? (
      <>
        <path d={barsPath(s, ts, vs.map((v) => (v != null && v > 0 ? v : null)), barW(s), 0, 1)} fill="var(--s2)" />
        <path d={barsPath(s, ts, vs.map((v) => (v != null && v < 0 ? v : null)), barW(s), 0, 1)} fill="var(--s1)" />
      </>
    ) : (
      <>
        <path d={barsPath(s, ts, vs, barW(s), 0, days.length > 60 ? 1 : 2)} fill={m.color} />
        {m.goal && <line x1={0} x2={s.w} y1={s.y(m.goal)} y2={s.y(m.goal)} stroke="var(--ink-2)" strokeDasharray="3 3" strokeOpacity={0.6} />}
      </>
    );
  const at = (t: number | null) => (t == null ? undefined : idx.get(t));
  const fmt = (v: number | null | undefined) => (m.kind === 'dev' && v != null ? `${v >= 0 ? '+' : '−'}${num(Math.abs(v), 1)}` : num(v, m.digits ?? 0));
  return {
    y,
    ticks,
    draw,
    tickFmt: m.kind === 'bar' ? k : (v: number) => num(v, ticks.some((x) => x % 1) ? 1 : 0),
    marks: (t: number) => {
      const i = at(t);
      return i == null ? [] : [{ v: m.kind === 'line' ? (avg7[i] ?? vs[i]) : vs[i], color: m.color }];
    },
    readout: (t: number | null) => {
      const i = at(t);
      if (i == null) {
        // Changes average out to zero, so the latest one says more.
        if (m.kind === 'dev') {
          const last = days.findLastIndex((d) => d[m.key] != null);
          return last < 0 ? <Muted>No data</Muted> : <><b>{fmt(vs[last])}</b>{m.unit} <Muted>{dateW(days[last].t)}</Muted></>;
        }
        return (
          <>
            <b>{fmt(mu)}</b> {m.unit} <Muted>{avgLabel}</Muted>
          </>
        );
      }
      return (
        <>
          <b>{fmt(vs[i])}</b>{m.kind === 'dev' ? '' : ' '}{m.unit}
          {m.kind === 'line' && avg7[i] != null && <Muted> · 7-day {fmt(avg7[i])}</Muted>}
        </>
      );
    },
  };
}

function sleepTracks(days: Row[], idx: Map<number, number>, avgLabel: string) {
  const ts = days.map((d) => d.t);
  const at = (t: number | null) => (t == null ? undefined : idx.get(t));
  // Cumulative bars deep → +light → +REM → +awake, the tallest painted first so the lower ones cover it.
  const stack = (s: Scale) => {
    const w = barW(s, days.length > 60 ? 0.2 : 0.3);
    return STAGES.slice()
      .reverse()
      .map((st, si, arr) => {
        const keys = arr.slice(0, si + 1).map((x) => x.key);
        const top = days.map((d) => (d.sleep == null ? null : keys.reduce((a, kk) => a + (d[kk] ?? 0), 0)));
        return <path key={st.key} d={barsPath(s, ts, top, w, 0, si === arr.length - 1 ? 2 : 0)} fill={st.color} />;
      })
      .reverse();
  };
  const window = (s: Scale) => {
    const w = barW(s, 0.3);
    return (
      <>
        {days.map((d) =>
          d.bedtime == null ? null : (
            <rect key={d.t} x={s.x(d.t) - w / 2} width={w} y={s.y(-d.bedtime)} height={s.y(-d.wake!) - s.y(-d.bedtime)} rx={Math.min(2, w / 2)} fill="var(--s7)" opacity={0.55} />
          ),
        )}
      </>
    );
  };
  const avgSleep = mean(days.map((d) => d.sleep));
  const avgBed = mean(days.map((d) => d.bedtime));
  const avgWake = mean(days.map((d) => d.wake));
  // Earlier is higher; ticks every 4 hours of the clock.
  const top = -Math.min(...days.map((d) => d.bedtime ?? Infinity), 0) + 30;
  const bottom = -Math.max(...days.map((d) => d.wake ?? -Infinity), 420) - 30;
  const winTicks: number[] = [];
  for (let v = Math.floor(top / 240) * 240; v > bottom; v -= 240) if (v < top) winTicks.push(v);
  return {
    stages: {
      ...niceTicksHours(Math.max(...days.map((d) => (d.sleep ?? 0) + (d.awake ?? 0)), 480)),
      tickFmt: (v: number) => `${v / 60} h`,
      draw: stack,
      readout: (t: number | null) => {
        const i = at(t);
        if (i == null)
          return (
            <>
              <b>{hm(avgSleep)}</b> h <Muted>{avgLabel}</Muted>
            </>
          );
        const d = days[i];
        if (d.sleep == null) return <Muted>No data</Muted>;
        return (
          <>
            <b>{hm(d.sleep)}</b> h{' '}
            <Muted>
              · deep {hm(d.deep)} · REM {hm(d.rem)}
            </Muted>
          </>
        );
      },
    },
    window: {
      y: [bottom, top] as Range,
      ticks: winTicks,
      tickFmt: (v: number) => clock(-v),
      draw: window,
      marks: () => [],
      readout: (t: number | null) => {
        const i = at(t);
        if (i == null)
          return (
            <>
              <b>{clock(avgBed ?? 0)}</b>–<b>{clock(avgWake ?? 0)}</b> <Muted>avg</Muted>
            </>
          );
        const d = days[i];
        return d.bedtime == null ? <Muted>No data</Muted> : (
          <>
            <b>{clock(d.bedtime)}</b>–<b>{clock(d.wake!)}</b>
          </>
        );
      },
    },
  };
}

/** A y-range from 0 in whole hours, labelled every 2 or 4 hours. */
function niceTicksHours(maxMin: number) {
  const step = maxMin > 600 ? 240 : 120;
  const top = Math.ceil(maxMin / 60) * 60;
  const ticks: number[] = [];
  for (let v = 0; v <= top; v += step) ticks.push(v);
  return { y: [0, top] as Range, ticks };
}

export function TrendsPage({ range }: { range: Period }) {
  const navigate = useNavigate();
  const overview = useQuery(overviewQuery);
  const all = withData(overview.data?.days ?? []);
  const picked = range === 'all' ? all : all.slice(-Number(range));
  // Temperature is relative to all nights, so its baseline doesn't move with the period.
  const rows = useMemo(() => toRows(all), [overview.data]); // eslint-disable-line react-hooks/exhaustive-deps
  const days = rows.slice(rows.length - picked.length);

  if (overview.isError) return <ErrorCard onRetry={() => overview.refetch()} />;

  return (
    <div className="space-y-10">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Trends</h1>
          <p className="text-sm text-muted">{days.length ? `${dateW(days[0].t)} – ${dateW(days.at(-1)!.t)}` : ''}</p>
        </div>
        <Segmented
          label="Period"
          value={String(range)}
          onChange={(r) => navigate({ to: '/trends', search: { range: r === 'all' ? 'all' : (Number(r) as Period) } })}
          options={[
            { value: '7', label: '7 days' },
            { value: '30', label: '30 days' },
            { value: '90', label: '90 days' },
            { value: 'all', label: 'All' },
          ]}
        />
      </div>
      {overview.isPending ? (
        <div className="space-y-3">
          {Array.from({ length: 6 }, (_, i) => <Skeleton key={i} className="h-16 rounded-xl" />)}
        </div>
      ) : days.length ? (
        <Rows key={range} days={days} label={range === 'all' ? 'all-time avg' : `${range}-day avg`} />
      ) : (
        <p className="py-16 text-center text-sm text-muted">No data in this period.</p>
      )}
    </div>
  );
}

function Rows({ days, label }: { days: Row[]; label: string }) {
  const { idx, domain } = useRows(days);
  const sl = sleepTracks(days, idx, label);
  const row = (key: Key) => {
    const m = M[key];
    if (days.every((d) => d[key] == null)) return null;
    return <Track key={key} side title={m.title} help={<Help topic={m.help} />} height={m.height ?? 64} {...metricTrack(m, days, idx, label)} />;
  };
  return (
    <SyncGroup domain={domain} mode="date">
      <div className="space-y-10">
        <Section
          title="Night"
          note={
            <span className="inline-flex items-center gap-1">
              Each night is dated by the morning it ended. Thin line: daily values, thick line: 7-day average.
              <Help topic="trendLines" />
            </span>
          }
          action={<StageLegend />}
        >
          <div className="space-y-3">
            <Track side title="Sleep stages" help={<Help topic="stages" />} height={96} {...sl.stages} />
            <Track side title="Bedtime – wake" help={<Help topic="schedule" />} height={64} {...sl.window} />
            {row('rhr')}
            {row('hrv')}
            {row('spo2')}
            {row('breath')}
            {row('temp')}
            <TimeAxis side />
          </div>
        </Section>
        <Section title="Day" note="Dashed lines: 10,000 steps and 22 zone minutes a day (150 a week)">
          <div className="space-y-3">
            {row('steps')}
            {row('azm')}
            {row('kcal')}
            <TimeAxis side />
          </div>
        </Section>
        <Section title="All days" note="Hovering a row moves the cursor in the charts; click a day to open it">
          <DaysTable days={days} />
        </Section>
      </div>
    </SyncGroup>
  );
}

function StageLegend() {
  return (
    <div className="flex gap-3 text-xs text-ink-2">
      {STAGES.slice().reverse().map((s) => (
        <span key={s.key} className="inline-flex items-center gap-1.5">
          <i className="size-2 rounded-[2px]" style={{ background: s.color }} />
          {s.label}
        </span>
      ))}
    </div>
  );
}

function DaysTable({ days }: { days: Row[] }) {
  const [all, setAll] = useState(false);
  const shown = [...days].reverse().slice(0, all ? undefined : 10);
  const maxSleep = Math.max(...days.map((d) => d.sleep ?? 0));
  const maxSteps = Math.max(...days.map((d) => d.steps ?? 0));
  const g = useGroup();
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-[13px] tabular-nums md:min-w-[640px]" onMouseLeave={() => g.cursor.set(null)}>
        <thead>
          <tr className="border-b text-xs text-ink-2">
            <th className="py-2 text-left font-medium">Day</th>
            <th className="hidden w-[22%] py-2 pl-4 text-left font-medium md:table-cell">Sleep</th>
            <th className="py-2 text-right font-medium md:hidden">Sleep</th>
            <th className="hidden md:table-cell" />
            <th className="py-2 pl-2 text-right font-medium">RHR</th>
            <th className="py-2 text-right font-medium">HRV</th>
            <th className="hidden w-[22%] py-2 pl-6 text-left font-medium md:table-cell">Steps</th>
            <th className="py-2 text-right font-medium md:hidden">Steps</th>
            <th className="hidden md:table-cell" />
            <th className="py-2 text-right font-medium">AZM</th>
          </tr>
        </thead>
        <tbody>
          {shown.map((d) => (
            <DayRow key={d.t} d={d} maxSleep={maxSleep} maxSteps={maxSteps} />
          ))}
        </tbody>
      </table>
      {!all && days.length > 10 && (
        <button className="mt-3 text-[13px] font-medium text-s1 hover:underline" onClick={() => setAll(true)}>
          Show more
        </button>
      )}
    </div>
  );
}

function DayRow({ d, maxSleep, maxSteps }: { d: Row; maxSleep: number; maxSteps: number }) {
  const g = useGroup();
  const navigate = useNavigate();
  const hot = useStore(g.cursor) === d.t;
  return (
    <tr
      className={cn('cursor-pointer border-b last:border-0', hot && 'bg-surface-2')}
      onMouseEnter={() => g.cursor.set(d.t)}
      onClick={() => navigate({ to: '/', search: { date: d.date } })}
    >
      <td className="whitespace-nowrap py-2">
        <Link to="/" search={{ date: d.date }} className="font-medium hover:text-s1">
          {dateW(d.t)}
        </Link>
      </td>
      <td className="hidden py-2 pl-4 pr-3 md:table-cell">{d.sleep != null && <InlineBar value={d.sleep} max={maxSleep} color="var(--stage-deep)" />}</td>
      <td className="w-14 py-2 text-right">{hm(d.sleep)}</td>
      <td className="py-2 text-right">{num(d.rhr)}</td>
      <td className="py-2 text-right">{num(d.hrv, 1)}</td>
      <td className="hidden py-2 pl-6 pr-3 md:table-cell">{d.steps != null && <InlineBar value={d.steps} max={maxSteps} color="var(--s3)" />}</td>
      <td className="w-16 py-2 text-right">{num(d.steps)}</td>
      <td className="py-2 text-right">{num(d.azm)}</td>
    </tr>
  );
}
