import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link, useNavigate } from '@tanstack/react-router';
import { overviewQuery, type Daily } from '@/lib/api';
import { chrome, row } from '@/lib/chart';
import { duration, hm, num, weekdayShort } from '@/lib/format';
import { mean, rolling, type MetricKey } from '@/lib/stats';
import { useResolvedTheme } from '@/lib/theme';
import { EChart } from '@/components/EChart';
import { Legend } from '@/components/DayTimeline';
import { STAGES } from '@/components/SleepCard';
import { Card, CardBody, CardHeader } from '@/components/ui/card';
import { Segmented } from '@/components/ui/segmented';
import { Skeleton } from '@/components/ui/skeleton';
import { ErrorCard, withData } from './day';

export type Range = '7' | '30' | '90' | 'all';

type Metric = { key: MetricKey; title: string; unit: string; digits?: number; kind: 'line' | 'bar'; color: string; max?: number };

const METRICS: Metric[] = [
  { key: 'resting_hr', title: 'Klidový tep', unit: 'bpm', kind: 'line', color: 's5' },
  { key: 'hrv_ms', title: 'HRV', unit: 'ms', digits: 1, kind: 'line', color: 's1' },
  { key: 'steps', title: 'Kroky', unit: 'kroků', kind: 'bar', color: 's3' },
  { key: 'azm', title: 'Aktivní zóny', unit: 'min', kind: 'bar', color: 's2' },
  { key: 'spo2_avg', title: 'SpO₂ v noci', unit: '%', digits: 1, kind: 'line', color: 's1', max: 100 },
  { key: 'respiratory_rate', title: 'Dech ve spánku', unit: 'dechů/min', digits: 1, kind: 'line', color: 's3' },
];

export function TrendsPage({ range }: { range: Range }) {
  const navigate = useNavigate();
  const overview = useQuery(overviewQuery);
  const all = withData(overview.data?.days ?? []);
  const days = range === 'all' ? all : all.slice(-Number(range));

  if (overview.isError) return <ErrorCard onRetry={() => overview.refetch()} />;

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Trendy</h1>
          <p className="text-sm text-muted">
            {days.length ? `${weekdayShort(days[0].date)} – ${weekdayShort(days.at(-1)!.date)}` : ''}
            {days.length > 0 && days.length < 7 && ' · klouzavý průměr se ukáže od 7 dnů dat'}
          </p>
        </div>
        <Segmented<Range>
          label="Období"
          value={range}
          onChange={(r) => navigate({ to: '/trends', search: { range: r } })}
          options={[
            { value: '7', label: '7 dní' },
            { value: '30', label: '30 dní' },
            { value: '90', label: '90 dní' },
            { value: 'all', label: 'Vše' },
          ]}
        />
      </div>

      {overview.isPending ? (
        <div className="grid gap-4 md:grid-cols-2">
          {Array.from({ length: 6 }, (_, i) => <Skeleton key={i} className="h-[260px] rounded-xl" />)}
        </div>
      ) : (
        <>
          <SleepTrend days={days} />
          <div className="grid gap-4 md:grid-cols-2">
            {METRICS.map((m) => <TrendCard key={m.key} metric={m} days={days} />)}
          </div>
          <DaysTable days={days} />
        </>
      )}
    </div>
  );
}

/** Bounds and step snapped to 1/2/5 × 10ⁿ, so the edge labels sit on grid lines (about 3 steps). */
function niceRange(lo: number, hi: number, cap = Infinity) {
  const raw = (hi - lo) / 3 || 1;
  const p = 10 ** Math.floor(Math.log10(raw));
  const interval = [1, 2, 5, 10].map((f) => f * p).find((s) => s >= raw)!;
  return { min: Math.floor(lo / interval) * interval, max: Math.min(cap, Math.ceil(hi / interval) * interval), interval };
}

function TrendCard({ metric: m, days }: { metric: Metric; days: Daily[] }) {
  const theme = useResolvedTheme();
  const values = days.map((d) => d[m.key]);
  const avg7 = rolling(values, 7);
  const showAvg = days.length >= 7;
  const present = values.filter((v): v is number => v != null);
  const option = useMemo(() => {
    const t = chrome();
    const color = t.c[m.color as 's1'];
    // Levels get a y-range of at least ±10 % of the mean so noise doesn't read as a trend.
    const mu = mean(present) ?? 0;
    const pad = Math.abs(mu) * 0.1;
    const yRange = m.kind === 'line' && present.length
      ? niceRange(Math.min(...present, mu - pad), Math.max(...present, mu + pad), m.max)
      : { min: 0 };
    return {
      animation: false,
      textStyle: t.textStyle,
      grid: { left: 44, right: 12, top: 12, bottom: 26 },
      tooltip: {
        ...t.tooltip, trigger: 'axis', axisPointer: { type: m.kind === 'bar' ? 'shadow' : 'line', shadowStyle: { color: t.c.shade } },
        formatter: (ps: { dataIndex: number }[]) => {
          const i = ps[0].dataIndex;
          let html = `<div style="font-weight:600;margin-bottom:2px">${weekdayShort(days[i].date)}</div>`;
          html += row(color, m.title, values[i] == null ? '–' : `${num(values[i], m.digits ?? 0)} ${m.unit}`);
          if (showAvg && avg7[i] != null) html += row(t.c.ink2, 'Ø 7 dní', `${num(avg7[i], m.digits ?? 0)} ${m.unit}`);
          return html;
        },
      },
      xAxis: {
        type: 'category', data: days.map((d) => weekdayShort(d.date)), axisLabel: { ...t.axisLabel, hideOverlap: true },
        axisLine: { lineStyle: { color: t.c.axis } }, axisTick: { show: false },
      },
      yAxis: { ...t.valueAxis, ...yRange, axisLabel: { ...t.axisLabel, formatter: (v: number) => (v >= 10000 ? `${v / 1000}k` : num(v, 'interval' in yRange && yRange.interval < 1 ? 1 : 0)) } },
      series: [
        m.kind === 'bar'
          ? { type: 'bar', data: values, barMaxWidth: 26, itemStyle: { color, borderRadius: [4, 4, 0, 0] } }
          : {
              type: 'line', data: values, connectNulls: false, symbol: 'circle', symbolSize: days.length > 45 ? 0 : 7,
              lineStyle: { width: showAvg ? 1 : 2, color, opacity: showAvg ? 0.5 : 1 },
              itemStyle: { color, borderColor: t.c.surface, borderWidth: 1.5 },
            },
        ...(showAvg
          ? [{ type: 'line', data: avg7, symbol: 'none', smooth: 0.3, lineStyle: { width: 2.5, color: t.c.ink2 }, z: 3 }]
          : []),
      ],
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [days, theme]);

  const last = present.at(-1);
  return (
    <Card>
      <CardHeader
        title={m.title}
        subtitle={m.unit}
        action={
          <div className="text-right">
            <div className="text-lg font-semibold tabular-nums leading-tight">{num(mean(present), m.digits ?? 0)}</div>
            <div className="text-[11px] text-muted">Ø za období{last != null ? ` · poslední ${num(last, m.digits ?? 0)}` : ''}</div>
          </div>
        }
      />
      <CardBody className="pt-1">
        <EChart option={option} className="h-[200px]" label={`Graf: ${m.title} po dnech`} />
        {showAvg && (
          <div className="mt-1 flex gap-4 text-xs text-ink-2">
            <Legend color={`var(--${m.color})`} label="Denně" />
            <Legend color="var(--ink-2)" label="Ø 7 dní" />
          </div>
        )}
      </CardBody>
    </Card>
  );
}

function SleepTrend({ days }: { days: Daily[] }) {
  const theme = useResolvedTheme();
  const keys: Record<string, MetricKey> = { DEEP: 'sleep_deep_minutes', LIGHT: 'sleep_light_minutes', REM: 'sleep_rem_minutes', AWAKE: 'sleep_awake_minutes' };
  const nights = days.filter((d) => d.sleep_minutes != null);
  const option = useMemo(() => {
    const t = chrome();
    return {
      animation: false,
      textStyle: t.textStyle,
      grid: { left: 44, right: 12, top: 12, bottom: 26 },
      tooltip: {
        ...t.tooltip, trigger: 'axis', axisPointer: { type: 'shadow', shadowStyle: { color: t.c.shade } },
        formatter: (ps: { dataIndex: number }[]) => {
          const d = nights[ps[0].dataIndex];
          return `<div style="font-weight:600;margin-bottom:2px">${weekdayShort(d.date)} · ${duration(d.sleep_minutes)}</div>` +
            STAGES.map((s) => row(t.c[s.color.slice(2) as 's1'], s.label, duration(d[keys[s.key]] as number | null))).join('');
        },
      },
      xAxis: {
        type: 'category', data: nights.map((d) => weekdayShort(d.date)), axisLabel: { ...t.axisLabel, hideOverlap: true },
        axisLine: { lineStyle: { color: t.c.axis } }, axisTick: { show: false },
      },
      yAxis: { ...t.valueAxis, min: 0, axisLabel: { ...t.axisLabel, formatter: (v: number) => `${v / 60} h` }, interval: 120 },
      series: STAGES.map((s, i) => ({
        type: 'bar', stack: 'sleep', name: s.label, barMaxWidth: 34,
        data: nights.map((d) => d[keys[s.key]] ?? 0),
        itemStyle: {
          color: t.c[s.color.slice(2) as 's1'], borderColor: t.c.surface, borderWidth: 1,
          borderRadius: i === STAGES.length - 1 ? [4, 4, 0, 0] : 0,
        },
      })),
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nights, theme]);
  const avg = mean(nights.map((d) => d.sleep_minutes!));
  return (
    <Card>
      <CardHeader
        title="Spánek"
        subtitle="Fáze spánku po nocích (den = den probuzení)"
        action={
          <div className="text-right">
            <div className="text-lg font-semibold tabular-nums leading-tight">{avg != null ? `${hm(avg)} h` : '–'}</div>
            <div className="text-[11px] text-muted">Ø spánku za období</div>
          </div>
        }
      />
      <CardBody className="pt-1">
        {nights.length ? (
          <>
            <EChart option={option} className="h-[230px]" label="Graf fází spánku po nocích" />
            <div className="mt-1 flex flex-wrap gap-4 text-xs text-ink-2">
              {STAGES.map((s) => <Legend key={s.key} color={`var(${s.color})`} label={s.label} />)}
            </div>
          </>
        ) : (
          <p className="py-12 text-center text-sm text-muted">V tomto období nejsou data o spánku.</p>
        )}
      </CardBody>
    </Card>
  );
}

function DaysTable({ days }: { days: Daily[] }) {
  const navigate = useNavigate();
  const cols: [string, (d: Daily) => string][] = [
    ['Spánek', (d) => hm(d.sleep_minutes)],
    ['Klidový tep', (d) => num(d.resting_hr)],
    ['HRV', (d) => num(d.hrv_ms, 1)],
    ['Kroky', (d) => num(d.steps)],
    ['AZM', (d) => num(d.azm)],
    ['Aktivní kcal', (d) => num(d.active_kcal)],
    ['SpO₂', (d) => num(d.spo2_avg, 1)],
    ['Dech', (d) => num(d.respiratory_rate, 1)],
    ['Tep min–max', (d) => (d.hr_min == null ? '–' : `${num(d.hr_min)}–${num(d.hr_max)}`)],
  ];
  return (
    <Card>
      <CardHeader title="Všechny dny" subtitle="Kliknutím otevřeš detail dne" />
      <CardBody className="overflow-x-auto px-0 pb-1">
        <table className="w-full text-sm tabular-nums">
          <thead>
            <tr className="border-b text-xs text-ink-2">
              <th className="sticky left-0 bg-surface px-5 py-2 text-left font-medium">Den</th>
              {cols.map(([h]) => <th key={h} className="whitespace-nowrap px-3 py-2 text-right font-medium">{h}</th>)}
            </tr>
          </thead>
          <tbody>
            {[...days].reverse().map((d) => (
              <tr
                key={d.date}
                className="group cursor-pointer border-b last:border-0 hover:bg-surface-2"
                onClick={() => navigate({ to: '/', search: { date: d.date } })}
              >
                <td className="sticky left-0 whitespace-nowrap bg-surface px-5 py-2 group-hover:bg-surface-2">
                  <Link to="/" search={{ date: d.date }} className="font-medium hover:text-s1">{weekdayShort(d.date)}</Link>
                </td>
                {cols.map(([h, f]) => <td key={h} className="whitespace-nowrap px-3 py-2 text-right">{f(d)}</td>)}
              </tr>
            ))}
          </tbody>
        </table>
      </CardBody>
    </Card>
  );
}
