import { useMemo } from 'react';
import { barX, defineChart } from '@tanstack/charts';
import { Chart } from '@tanstack/charts/react';
import { scaleBand } from '@tanstack/charts/scales/band';
import { tooltip } from '@tanstack/charts/tooltip';
import { scaleLinear } from '@tanstack/charts/scales/linear';
import { hourTicks } from '../unovis/HeartTimelineUnovis';
import type { Night, Stage } from '@/lib/api';
import { clock, duration } from '@/lib/format';
import { STAGES } from '../SleepCard';
import { Legend } from '../DayTimeline';
import { Card, CardBody, CardHeader } from '../ui/card';

type Segment = { stage: Stage; label: string; from: number; to: number; minutes: number };

const hhmm = new Intl.DateTimeFormat('cs-CZ', { hour: '2-digit', minute: '2-digit' });
/** Rows top to bottom. */
const ROWS: Stage[] = ['AWAKE', 'REM', 'LIGHT', 'DEEP'];

/** TanStack Charts hypnogram: one barX mark per stage on a categorical y band. */
export function HypnogramTanstack({ night }: { night: Night | null }) {
  const definition = useMemo(() => {
    if (!night) return null;
    const label = Object.fromEntries(STAGES.map((s) => [s.key, s.label])) as Record<Stage, string>;
    const segments: Segment[] = night.stages.map((s) => ({
      stage: s.stage,
      label: label[s.stage],
      // barX positions are numeric, so time is passed as epoch milliseconds.
      from: +new Date(s.start),
      to: +new Date(s.end),
      minutes: Math.round((+new Date(s.end) - +new Date(s.start)) / 60_000),
    }));
    return defineChart({
      // Fills are fixed per mark, so each stage is its own mark in the fixed color order.
      marks: STAGES.map((s) =>
        barX(segments.filter((g) => g.stage === s.key), { y: 'label', x1: 'from', x2: 'to', fill: `var(${s.color})`, radius: 3 }),
      ),
      scales: {
        x: {
          scale: scaleLinear().domain([+new Date(night.start), +new Date(night.end)]),
          axis: { ticks: { values: hourTicks(+new Date(night.start), +new Date(night.end), 1), format: (t: number) => hhmm.format(t) } },
        },
        y: { scale: scaleBand().domain(ROWS.map((r) => label[r])).padding(0.28), grid: true },
      },
      tooltip: {
        use: tooltip,
        format: (p) => `${p.datum.label} · ${hhmm.format(p.datum.from)}–${hhmm.format(p.datum.to)} · ${p.datum.minutes} min`,
      },
    });
  }, [night]);

  if (!night || !definition) return null;
  return (
    <Card>
      <CardHeader
        title="Hypnogram"
        subtitle={`TanStack Charts · ${clock(night.start)}–${clock(night.end)}, spánek ${duration(night.minutes_asleep)}`}
        action={
          <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-ink-2">
            {STAGES.map((s) => <Legend key={s.key} color={`var(${s.color})`} label={s.label} />)}
          </div>
        }
      />
      <CardBody className="pt-2 text-muted">
        <Chart definition={definition} height={184} ariaLabel="Hypnogram: fáze spánku v čase" />
      </CardBody>
    </Card>
  );
}
