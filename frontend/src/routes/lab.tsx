import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { FlaskConical } from 'lucide-react';
import { dayQuery, overviewQuery } from '@/lib/api';
import { DateNav } from '@/components/DateNav';
import { DayTimeline } from '@/components/DayTimeline';
import { SleepCard } from '@/components/SleepCard';
import { HeartTimelineUnovis } from '@/components/unovis/HeartTimelineUnovis';
import { HypnogramUnovis } from '@/components/unovis/HypnogramUnovis';
import { HeartTimelineTanstack } from '@/components/tanstack/HeartTimelineTanstack';
import { HypnogramTanstack } from '@/components/tanstack/HypnogramTanstack';
import { Segmented } from '@/components/ui/segmented';
import { Skeleton } from '@/components/ui/skeleton';
import { ErrorCard, withData } from './day';

/** Side-by-side comparison of chart libraries on real data (not linked from the nav). */
export default function LabPage({ date: wanted }: { date?: string }) {
  const navigate = useNavigate();
  const overview = useQuery(overviewQuery);
  const days = withData(overview.data?.days ?? []);
  const date = wanted && days.some((d) => d.date === wanted) ? wanted : days.at(-1)?.date;
  const day = useQuery({ ...dayQuery(date ?? ''), enabled: !!date });
  const [lib, setLib] = useState<Lib>('all');

  if (overview.isError || day.isError) return <ErrorCard onRetry={() => (overview.refetch(), day.refetch())} />;

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="flex items-center gap-2 text-xl font-semibold tracking-tight">
            <FlaskConical className="size-5 text-s7" /> Lab: grafové knihovny
          </h1>
          <p className="text-sm text-muted">Stejná data ve třech knihovnách: současné ECharts a prototypy v Unovis a TanStack Charts.</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Segmented<Lib>
            label="Knihovna"
            value={lib}
            onChange={setLib}
            options={[
              { value: 'all', label: 'Všechny' },
              { value: 'echarts', label: 'ECharts' },
              { value: 'unovis', label: 'Unovis' },
              { value: 'tanstack', label: 'TanStack' },
            ]}
          />
          {date && <DateNav date={date} available={days.map((d) => d.date)} onChange={(d) => navigate({ to: '/lab', search: { date: d } })} />}
        </div>
      </div>

      {!day.data ? (
        <div className="space-y-5">
          {Array.from({ length: 2 }, (_, i) => <Skeleton key={i} className="h-[420px] rounded-xl" />)}
        </div>
      ) : (
        <>
          <Section title="Tep a pohyb">
            {show(lib, 'echarts') && <Column name="ECharts"><DayTimeline day={day.data} /></Column>}
            {show(lib, 'unovis') && <Column name="Unovis"><HeartTimelineUnovis day={day.data} /></Column>}
            {show(lib, 'tanstack') && <Column name="TanStack Charts"><HeartTimelineTanstack day={day.data} /></Column>}
          </Section>
          <Section title="Spánek">
            {show(lib, 'echarts') && <Column name="ECharts"><SleepCard night={day.data.sleep} /></Column>}
            {show(lib, 'unovis') && <Column name="Unovis"><HypnogramUnovis night={day.data.sleep} /></Column>}
            {show(lib, 'tanstack') && <Column name="TanStack Charts"><HypnogramTanstack night={day.data.sleep} /></Column>}
          </Section>
        </>
      )}
    </div>
  );
}

type Lib = 'all' | 'echarts' | 'unovis' | 'tanstack';
const show = (selected: Lib, lib: Lib) => selected === 'all' || selected === lib;

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-3">
      <h2 className="border-b pb-1 text-sm font-semibold text-ink-2">{title}</h2>
      <div className="space-y-5">{children}</div>
    </section>
  );
}

function Column({ name, children }: { name: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0 space-y-2">
      <div className="text-xs font-medium uppercase tracking-wide text-muted">{name}</div>
      {children}
    </div>
  );
}
