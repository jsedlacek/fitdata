import { useMemo, useState } from 'react';
import { areaY, defineChart, lineY, rect } from '@tanstack/charts';
import { crosshair } from '@tanstack/charts/crosshair';
import { controlledSignal } from '@tanstack/charts/interaction/signal';
import { zoomX, type ZoomXChange, type ZoomXWindow } from '@tanstack/charts/interaction/zoom';
import { Chart } from '@tanstack/charts/react';
import { scaleLinear } from '@tanstack/charts/scales/linear';
import { tooltip } from '@tanstack/charts/tooltip';
import { scaleTime } from 'd3-scale';
import { RotateCcw } from 'lucide-react';
import type { DayDetail } from '@/lib/api';
import { num } from '@/lib/format';
import { Legend } from '../DayTimeline';
import { Button } from '../ui/button';
import { Card, CardBody, CardHeader } from '../ui/card';

type HrRow = { t: Date; lo: number | null; avg: number | null; hi: number | null };
type StepRow = { from: Date; to: Date; steps: number };
type Band = { from: Date; to: Date; label: string; kind: 'sleep' | 'exercise' };

const MIN = 60_000;
const hhmm = new Intl.DateTimeFormat('cs-CZ', { hour: '2-digit', minute: '2-digit' });

/** Rows with a null row inserted into gaps > `gapMin`; TanStack lines and areas break on nulls. */
function heartRows(day: DayDetail, gapMin: number): HrRow[] {
  const out: HrRow[] = [];
  day.heartRate.forEach(([ts, lo, avg, hi], i) => {
    const t = new Date(ts);
    if (i && +t - +new Date(day.heartRate[i - 1][0]) > gapMin * MIN) {
      out.push({ t: new Date(+t - MIN), lo: null, avg: null, hi: null });
    }
    out.push({ t, lo, avg, hi });
  });
  return out;
}

/** TanStack Charts version of "Tep a pohyb": wheel, drag or pinch zoom shared by both charts. */
export function HeartTimelineTanstack({ day }: { day: DayDetail }) {
  const extent = useMemo<ZoomXWindow<Date>>(() => ({ start: new Date(day.range[0]), end: new Date(day.range[1]) }), [day]);
  const [window, setWindow] = useState<ZoomXWindow<Date>>(extent);
  const [shownDay, setShownDay] = useState(day.date);
  if (shownDay !== day.date) {
    setShownDay(day.date);
    setWindow(extent);
  }
  const zoomed = +window.start !== +extent.start || +window.end !== +extent.end;

  const rows = useMemo(() => heartRows(day, 5), [day]);
  const steps = useMemo<StepRow[]>(
    () => day.steps.map(([ts, v]) => ({ from: new Date(ts), to: new Date(+new Date(ts) + 15 * MIN), steps: v })),
    [day],
  );
  const bands = useMemo<Band[]>(
    () => [
      ...(day.sleep ? [{ from: new Date(day.sleep.start), to: new Date(day.sleep.end), label: 'Spánek', kind: 'sleep' as const }] : []),
      ...day.exercises.map((e) => ({ from: new Date(e.start), to: new Date(e.end), label: e.name || e.kind, kind: 'exercise' as const })),
    ],
    [day],
  );
  const yMax = Math.max(160, ...day.heartRate.map((r) => r[3])) + 10;

  const zoom = useMemo(
    () =>
      zoomX({
        window: controlledSignal<ZoomXWindow<Date>, ZoomXChange<Date>>(window, (next) => setWindow(next)),
        extent: [extent.start, extent.end],
        scaleExtent: [1, 48],
        ariaLabel: 'Přiblížení časové osy',
        format: (d) => hhmm.format(d),
      }),
    [window, extent],
  );

  const xScale = useMemo(() => scaleTime().domain([window.start, window.end]), [window]);
  const xAxis = { ticks: { format: (d: Date) => hhmm.format(d) } };

  const heart = useMemo(
    () =>
      defineChart({
        marks: [
          // Full-height backgrounds: sleep and exercise windows.
          rect(bands.filter((b) => b.kind === 'sleep'), { x1: 'from', x2: 'to', y1: () => 30, y2: () => yMax, fill: 'var(--shade)' }),
          rect(bands.filter((b) => b.kind === 'exercise'), {
            x1: 'from', x2: 'to', y1: () => 30, y2: () => yMax, fill: 'var(--s2)', fillOpacity: 0.14,
          }),
          areaY(rows, { x: 't', y1: 'lo', y2: 'hi', fill: 'var(--band)', fillOpacity: 0.6 }),
          lineY(rows, { x: 't', y: 'avg', stroke: 'var(--s1)', strokeWidth: 1.5 }),
          crosshair({ x: { label: true }, y: false }),
        ],
        scales: {
          x: { scale: xScale, axis: xAxis },
          y: { scale: scaleLinear().domain([30, yMax]), grid: true },
        },
        // Zoomed marks must not paint over the axes.
        clip: true,
        focus: 'group-x',
        tooltip: {
          use: tooltip,
          format: (point) => {
            const d = point.datum as Partial<HrRow>;
            return d.avg == null ? '' : `${hhmm.format(d.t!)} · tep ${num(d.avg)} bpm (${num(d.lo)}–${num(d.hi)})`;
          },
        },
        controls: [zoom],
      }),
    [rows, bands, yMax, xScale, zoom],
  );

  const stepsChart = useMemo(
    () =>
      defineChart({
        marks: [rect(steps, { x1: 'from', x2: 'to', y1: () => 0, y2: 'steps', fill: 'var(--s3)', inset: 0.5, radius: 2 })],
        scales: {
          x: { scale: xScale, axis: xAxis },
          y: { scale: scaleLinear, nice: true, grid: true, axis: { ticks: { count: 2 } } },
        },
        clip: true,
        tooltip: { use: tooltip, format: (p) => `${hhmm.format(p.datum.from)} · ${num(p.datum.steps)} kroků` },
        // Follows the heart rate chart's zoom window through the shared x scale.
      }),
    [steps, xScale],
  );

  return (
    <Card>
      <CardHeader
        title="Tep a pohyb"
        subtitle="TanStack Charts · v grafu tepu kolečkem (po kliknutí), tažením nebo gestem přiblížíš a posuneš"
        action={
          <div className="flex items-center gap-3">
            <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-ink-2">
              <Legend color="var(--s1)" label="Tep" />
              <Legend color="var(--band)" label="Min–max" />
              <Legend color="var(--s3)" label="Kroky" />
            </div>
            {zoomed && (
              <Button variant="outline" size="sm" onClick={() => setWindow(extent)}>
                <RotateCcw /> Celý den
              </Button>
            )}
          </div>
        }
      />
      <CardBody className="pt-2 text-muted">
        <Chart definition={heart} height={260} ariaLabel="Tep během dne" />
        <Chart definition={stepsChart} height={90} ariaLabel="Kroky po 15 minutách" />
      </CardBody>
    </Card>
  );
}
