import { useMemo } from 'react';
import { VisAxis, VisTimeline, VisTooltip, VisXYContainer } from '@unovis/react';
import { Timeline } from '@unovis/ts';
import type { Night, Stage } from '@/lib/api';
import { clock, duration } from '@/lib/format';
import { STAGES } from '../SleepCard';
import { Legend } from '../DayTimeline';
import { Card, CardBody, CardHeader } from '../ui/card';
import { hourTicks } from './HeartTimelineUnovis';

type Segment = { start: number; length: number; stage: Stage; label: string; from: string; to: string };

const ms = (s: string) => new Date(s).getTime();
const hhmm = (t: number | Date) => new Date(+t).toLocaleTimeString('cs-CZ', { hour: '2-digit', minute: '2-digit' });
/** Top to bottom: awake, REM, light, deep (rows follow the order data appears in). */
const ROW_ORDER: Stage[] = ['AWAKE', 'REM', 'LIGHT', 'DEEP'];

/** Unovis version of the hypnogram, built on the Timeline (Gantt) component. */
export function HypnogramUnovis({ night }: { night: Night | null }) {
  const data = useMemo<Segment[]>(() => {
    if (!night) return [];
    const label = Object.fromEntries(STAGES.map((s) => [s.key, s.label])) as Record<Stage, string>;
    return [...night.stages]
      .sort((a, b) => ROW_ORDER.indexOf(a.stage) - ROW_ORDER.indexOf(b.stage) || a.start.localeCompare(b.start))
      .map((s) => ({
        start: ms(s.start),
        length: ms(s.end) - ms(s.start),
        stage: s.stage,
        label: label[s.stage],
        from: s.start,
        to: s.end,
      }));
  }, [night]);

  if (!night) return null;
  const color = Object.fromEntries(STAGES.map((s) => [s.key, `var(${s.color})`])) as Record<Stage, string>;

  return (
    <Card>
      <CardHeader
        title="Hypnogram"
        subtitle={`Unovis · ${clock(night.start)}–${clock(night.end)}, spánek ${duration(night.minutes_asleep)}`}
        action={
          <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-ink-2">
            {STAGES.map((s) => <Legend key={s.key} color={`var(${s.color})`} label={s.label} />)}
          </div>
        }
      />
      <CardBody className="vis-theme pt-2">
        <VisXYContainer<Segment> data={data} height={184} xDomain={[ms(night.start), ms(night.end)]}>
          <VisTimeline<Segment>
            x={(d) => d.start}
            length={(d) => d.length}
            lineRow={(d) => d.label}
            color={(d) => color[d.stage]}
            rowHeight={34}
            lineWidth={20}
            showRowLabels
            alternatingRowColors={false}
            rowLabelWidth={64}
          />
          <VisAxis type="x" tickFormat={hhmm} tickValues={hourTicks(ms(night.start), ms(night.end), 1)} gridLine={false} />
          <VisTooltip
            triggers={{
              [Timeline.selectors.line]: (d: Segment) =>
                `<b>${d.label}</b><br>${clock(d.from)}–${clock(d.to)} · ${Math.round(d.length / 60_000)} min`,
            }}
          />
        </VisXYContainer>
      </CardBody>
    </Card>
  );
}
