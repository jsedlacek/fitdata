import { useEffect } from 'react';
import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { Activity, BedDouble, Flame, Footprints, HeartPulse, RefreshCw, Waves, Wind, Zap } from 'lucide-react';
import { dayQuery, overviewQuery, syncQuery, type Daily } from '@/lib/api';
import { duration, hm, num } from '@/lib/format';
import { before, mean, type MetricKey } from '@/lib/stats';
import { DateNav } from '@/components/DateNav';
import { DayTimeline } from '@/components/DayTimeline';
import { ExerciseList } from '@/components/ExerciseList';
import { FirstSync } from '@/components/FirstSync';
import { KpiCard } from '@/components/KpiCard';
import { NightVitals } from '@/components/NightVitals';
import { SleepCard } from '@/components/SleepCard';
import { Button } from '@/components/ui/button';
import { Card, CardBody, CardHeader } from '@/components/ui/card';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';

/** `better`: which direction of change is good (1 = up, -1 = down); unset for neutral metrics. */
type Kpi = { key: MetricKey; label: string; icon: typeof Activity; unit?: string; digits?: number; accent: string; time?: boolean; better?: 1 | -1 };

const KPIS: Kpi[] = [
  { key: 'sleep_minutes', label: 'Sleep', icon: BedDouble, accent: 'var(--s7)', time: true, better: 1 },
  { key: 'resting_hr', label: 'Resting HR', icon: HeartPulse, unit: 'bpm', accent: 'var(--s5)', better: -1 },
  { key: 'hrv_ms', label: 'HRV', icon: Activity, unit: 'ms', digits: 1, accent: 'var(--s1)', better: 1 },
  { key: 'steps', label: 'Steps', icon: Footprints, accent: 'var(--s3)', better: 1 },
  { key: 'azm', label: 'Active Zone Minutes', icon: Zap, unit: 'min', accent: 'var(--s2)', better: 1 },
  { key: 'spo2_avg', label: 'SpO₂', icon: Waves, unit: '%', digits: 1, accent: 'var(--s1)' },
  { key: 'respiratory_rate', label: 'Breathing rate', icon: Wind, unit: '/min', digits: 1, accent: 'var(--s3)' },
  { key: 'active_kcal', label: 'Active kcal', icon: Flame, unit: 'kcal', accent: 'var(--s2)' },
];

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

  // ← / → step between days with data, unless a control (e.g. the theme toggle) uses the arrows itself.
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

  if (overview.isError) return <ErrorCard onRetry={() => overview.refetch()} />;
  if (overview.isPending) return <DaySkeleton />;
  if (!date && overview.data.user.connected && sync.data?.running) {
    return <FirstSync progress={sync.data.progress} />;
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

  const metrics = days[idx];

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-xl font-semibold tracking-tight">Day overview</h1>
        <DateNav date={date} available={days.map((d) => d.date)} onChange={go} />
      </div>

      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        {KPIS.map((k) => {
          const v = metrics[k.key];
          const base = before(days, date, k.key, 7);
          const avg = mean(base);
          const diff = v != null && avg != null ? v - avg : null;
          const fmt = (x: number) => (k.time ? duration(x) : num(x, k.digits ?? 0));
          const delta = diff == null ? null : `${diff >= 0 ? '+' : '−'}${fmt(Math.abs(diff))}`;
          // Changes within 5 % of the average are noise, not worth a color.
          const tone = diff == null || avg == null || !k.better || Math.abs(diff) < Math.abs(avg) * 0.05 ? undefined : Math.sign(diff) === k.better ? 'good' : 'bad';
          const spark = days.filter((d) => d.date <= date).slice(-14).map((d) => d[k.key]);
          return (
            <KpiCard
              key={k.key}
              icon={k.icon}
              label={k.label}
              value={v == null ? '–' : k.time ? hm(v) : num(v, k.digits ?? 0)}
              unit={k.time ? 'h' : k.unit}
              delta={delta}
              tone={tone}
              baselineLabel={`vs. ${base.length}-day avg`}
              spark={spark}
              accent={k.accent}
            />
          );
        })}
      </div>

      {day.isError ? (
        <ErrorCard onRetry={() => day.refetch()} />
      ) : !day.data ? (
        <>
          <Skeleton className="h-[420px] rounded-xl" />
          <Skeleton className="h-[360px] rounded-xl" />
        </>
      ) : (
        <div className={cn('space-y-5 transition-opacity', day.isPlaceholderData && 'opacity-60')} aria-busy={day.isPlaceholderData}>
          <DayTimeline day={day.data} />
          <div className="grid gap-5 lg:grid-cols-5">
            <div className="lg:col-span-3">
              <SleepCard night={day.data.sleep} timeZone={day.data.timeZone} />
            </div>
            <div className="space-y-5 lg:col-span-2">
              <HeartSummary m={metrics} />
              <ExerciseList exercises={day.data.exercises} />
            </div>
          </div>
          <NightVitals day={day.data} />
        </div>
      )}
    </div>
  );
}

function HeartSummary({ m }: { m: Daily }) {
  const items = [
    ['Lowest', m.hr_min],
    ['Average', m.hr_avg],
    ['Highest', m.hr_max],
    ['Asleep (non-REM)', m.nonrem_hr],
  ] as const;
  return (
    <Card>
      <CardHeader title="Heart rate during the day" subtitle="beats per minute" />
      <CardBody className="grid grid-cols-2 gap-3">
        {items.map(([label, v]) => (
          <div key={label} className="rounded-lg bg-surface-2 px-3 py-2">
            <div className="text-xs text-ink-2">{label}</div>
            <div className="text-lg font-semibold tabular-nums">{num(v)}</div>
          </div>
        ))}
        <div className="col-span-2 text-xs text-muted">
          Exercise in total {duration(m.exercise_minutes ?? 0)} · nightly temperature {m.nightly_temp_c != null ? `${num(m.nightly_temp_c, 1)} °C` : '–'}
        </div>
      </CardBody>
    </Card>
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
    <div className="space-y-5">
      <div className="flex justify-between">
        <Skeleton className="h-7 w-40" />
        <Skeleton className="h-8 w-64" />
      </div>
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        {Array.from({ length: 8 }, (_, i) => <Skeleton key={i} className="h-[108px] rounded-xl" />)}
      </div>
      <Skeleton className="h-[420px] rounded-xl" />
    </div>
  );
}
