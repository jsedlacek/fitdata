// Live charts for the docs: the app's own charts on demo data, each with a legend
// that says what every line, band, dot and bar means.
import type { ReactNode } from 'react';
import { DayExample } from '@/routes/day';
import { TrendExample } from '@/routes/trends';
import { Example, Legend, Mark } from './ui';

type Row = Parameters<typeof TrendExample>[0]['rows'][number];

const CURSOR: [ReactNode, string, ReactNode] = [
  <Mark kind="cursor" />,
  'Cursor',
  'Move the pointer over a chart: a line marks that moment in every chart, a dot sits on each value, and the numbers on the left show it.',
];

/** The usual legend of a Trends line row: each day, its 7-day average, the cursor. */
const lineLegend = (color: string): [ReactNode, string, ReactNode][] => [
  [<Mark kind="line" color={color} />, 'Each day', 'The line connects the value of every day; it breaks where a day has no data.'],
  [<Mark kind="band" color={color} />, '7-day average', 'The soft band behind it: each day averaged with the six before. One odd day barely moves it, a lasting change does.'],
  CURSOR,
];

/** Trends rows on demo data with a legend: the line rows get the usual one unless `legend` replaces it. */
export function TrendFigure({ rows, days = 30, color, legend }: { rows: Row[]; days?: number; color?: string; legend?: [ReactNode, string, ReactNode][] }) {
  return (
    <>
      <Example>
        <TrendExample rows={rows} days={days} />
      </Example>
      <Legend items={legend ?? lineLegend(color ?? 'var(--s1)')} />
    </>
  );
}

const STAGES: [ReactNode, string, ReactNode][] = [
  [<Mark kind="block" color="var(--stage-deep)" />, 'Deep', 'Deep sleep, mostly early in the night.'],
  [<Mark kind="block" color="var(--stage-light)" />, 'Light', 'Light sleep, most of the night.'],
  [<Mark kind="block" color="var(--stage-rem)" />, 'REM', 'Dream sleep, longer towards the morning.'],
  [<Mark kind="block" color="var(--stage-awake)" />, 'Awake', 'Short awakenings; a row of lighter amber marks restless moments.'],
];

const LEGENDS: Record<'night' | 'day', [ReactNode, string, ReactNode][]> = {
  night: [
    ...STAGES,
    [<Mark kind="line" color="var(--s5)" />, 'Heart rate', 'The average heart rate in each minute of the night.'],
    [<Mark kind="line" color="var(--s1)" />, 'SpO₂', 'Blood oxygen, measured about every minute while you sleep.'],
    [<Mark kind="dots" color="var(--s7)" />, 'HRV', 'One dot per HRV measurement, every 5 minutes of sleep.'],
    CURSOR,
  ],
  day: [
    [<Mark kind="line" color="var(--s1)" />, 'Heart rate', 'The average heart rate in each minute.'],
    [<Mark kind="range" />, 'Lowest to highest', 'The light band around the line: the lowest and highest reading in that minute.'],
    [<Mark kind="shade" color="var(--ex-band)" />, 'Exercise', 'A recorded activity: its icon above the chart, a faint band over its time. The list beside the charts has its numbers.'],
    [<Mark kind="bars" color="var(--s3)" />, 'Steps', 'One bar per 15 minutes: the steps in that quarter of an hour.'],
    CURSOR,
  ],
};

/** A demo day's night or day charts with their legend. */
export function DayFigure({ part }: { part: 'night' | 'day' }) {
  return (
    <>
      <Example>
        <DayExample part={part} />
      </Example>
      <Legend items={LEGENDS[part]} />
    </>
  );
}

export { STAGES as STAGE_LEGEND };
