import { useMemo, useState } from 'react';
import {
  VisArea,
  VisAxis,
  VisBrush,
  VisCrosshair,
  VisLine,
  VisPlotband,
  VisStackedBar,
  VisTooltip,
  VisXYContainer,
} from '@unovis/react';
import { AxisType, PlotbandLabelPosition } from '@unovis/ts';
import { RotateCcw } from 'lucide-react';
import type { DayDetail } from '@/lib/api';
import { num } from '@/lib/format';
import { Legend } from '../DayTimeline';
import { Button } from '../ui/button';
import { Card, CardBody, CardHeader } from '../ui/card';

type HrPoint = { t: number; lo?: number; avg?: number; hi?: number };
type StepBin = { t: number; steps: number };

const MIN = 60_000;
const ms = (s: string) => new Date(s).getTime();
/** Ticks on whole hours, every `step` hours, within [from, to]. */
export function hourTicks(from: number, to: number, step: number): number[] {
  const d = new Date(from);
  d.setMinutes(0, 0, 0);
  if (+d < from) d.setHours(d.getHours() + 1);
  while (d.getHours() % step) d.setHours(d.getHours() + 1);
  const out: number[] = [];
  for (; +d <= to; d.setHours(d.getHours() + step)) out.push(+d);
  return out;
}

/** Hours between ticks so that roughly 6–8 labels fit the visible span. */
const tickStep = ([a, b]: [number, number]) => {
  const h = (b - a) / 3_600_000;
  return h <= 6 ? 1 : h <= 12 ? 2 : h <= 24 ? 4 : 6;
};

const hhmm = (t: number | Date) => new Date(+t).toLocaleTimeString('cs-CZ', { hour: '2-digit', minute: '2-digit' });

/** Points with an empty one inserted into gaps > `gapMin`, so lines break there. */
function heartPoints(day: DayDetail, gapMin: number): HrPoint[] {
  const out: HrPoint[] = [];
  day.heartRate.forEach(([ts, lo, avg, hi], i) => {
    const t = ms(ts);
    if (i && t - ms(day.heartRate[i - 1][0]) > gapMin * MIN) out.push({ t: t - MIN });
    out.push({ t, lo, avg, hi });
  });
  return out;
}

/** Unovis version of "Tep a pohyb": brushing the steps strip zooms the heart rate chart. */
export function HeartTimelineUnovis({ day }: { day: DayDetail }) {
  const hr = useMemo(() => heartPoints(day, 5), [day]);
  // Area has no notion of missing values, so the band is drawn per continuous segment.
  const bandSegments = useMemo(() => {
    const segs: HrPoint[][] = [[]];
    for (const p of hr) {
      if (p.avg == null) segs.push([]);
      else segs[segs.length - 1].push(p);
    }
    return segs.filter((s) => s.length > 1);
  }, [hr]);
  const steps = useMemo<StepBin[]>(() => day.steps.map(([ts, v]) => ({ t: ms(ts) + 7.5 * MIN, steps: v })), [day]);
  const full: [number, number] = [ms(day.range[0]), ms(day.range[1])];
  const [zoom, setZoom] = useState<[number, number] | undefined>();
  const domain = zoom ?? full;

  const tooltip = (d: HrPoint) => {
    if (d.avg == null) return '';
    const s = steps.find((b) => Math.abs(b.t - d.t) <= 7.5 * MIN);
    return `<div style="font-weight:600;margin-bottom:2px">${hhmm(d.t)}</div>
      <div>Tep <b>${num(d.avg)}</b> bpm <span style="opacity:.6">(${num(d.lo)}–${num(d.hi)})</span></div>
      ${s ? `<div>Kroky (15 min) <b>${num(s.steps)}</b></div>` : ''}`;
  };

  return (
    <Card>
      <CardHeader
        title="Tep a pohyb"
        subtitle="Unovis · tažením v pásu kroků vybereš úsek, který se přiblíží"
        action={
          <div className="flex items-center gap-3">
            <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-ink-2">
              <Legend color="var(--s1)" label="Tep" />
              <Legend color="var(--band)" label="Min–max" />
              <Legend color="var(--s3)" label="Kroky" />
            </div>
            {zoom && (
              <Button variant="outline" size="sm" onClick={() => setZoom(undefined)}>
                <RotateCcw /> Celý den
              </Button>
            )}
          </div>
        }
      />
      <CardBody className="vis-theme pt-2">
        <VisXYContainer<HrPoint> data={hr} height={250} xDomain={domain} yDomain={[30, undefined]} clipPathExtend={0}>
          {day.sleep && (
            <VisPlotband axis={AxisType.X} from={ms(day.sleep.start)} to={ms(day.sleep.end)} labelText="Spánek" labelPosition={PlotbandLabelPosition.TopInside} />
          )}
          {day.exercises.map((e) => (
            <VisPlotband
              key={e.start}
              axis={AxisType.X}
              from={ms(e.start)}
              to={ms(e.end)}
              color="color-mix(in srgb, var(--s2) 14%, transparent)"
              labelText={e.name || e.kind}
              labelPosition={PlotbandLabelPosition.TopInside}
            />
          ))}
          {/* Unovis stacks the area on top of its baseline, so y is the span, not the upper bound. */}
          {bandSegments.map((seg) => (
            <VisArea<HrPoint> key={seg[0].t} data={seg} x={(d) => d.t} y={(d) => d.hi! - d.lo!} baseline={(d) => d.lo} color="var(--band)" opacity={0.6} />
          ))}
          <VisLine<HrPoint> x={(d) => d.t} y={(d) => d.avg} color="var(--s1)" lineWidth={1.5} />
          <VisAxis type="x" tickFormat={hhmm} tickValues={hourTicks(domain[0], domain[1], tickStep(domain))} gridLine={false} />
          <VisAxis type="y" numTicks={4} tickFormat={(v) => num(+v)} />
          <VisCrosshair<HrPoint> x={(d) => d.t} y={(d) => d.avg} template={tooltip} color="var(--s1)" />
          <VisTooltip />
        </VisXYContainer>

        <VisXYContainer<StepBin> data={steps} height={86} xDomain={full} margin={{ top: 6 }}>
          <VisStackedBar<StepBin> x={(d) => d.t} y={(d) => d.steps} color="var(--s3)" dataStep={15 * MIN} roundedCorners={2} barPadding={0.3} />
          <VisAxis type="x" tickFormat={hhmm} tickValues={hourTicks(full[0], full[1], 4)} gridLine={false} />
          <VisAxis type="y" numTicks={2} tickFormat={(v) => (+v >= 1000 ? `${+v / 1000}k` : String(v))} />
          <VisBrush
            selection={zoom}
            draggable
            onBrushEnd={(sel, _e, user) => {
              if (!user) return;
              const span = sel ? sel[1] - sel[0] : 0;
              // A (nearly) full selection means "no zoom"; tiny ones are accidental clicks.
              setZoom(sel && span > 20 * MIN && span < 0.98 * (full[1] - full[0]) ? sel : undefined);
            }}
          />
        </VisXYContainer>
      </CardBody>
    </Card>
  );
}
