import { useMemo } from 'react';
import type { DayDetail } from '@/lib/api';
import { chrome, minutesBetween, row } from '@/lib/chart';
import { clock, num } from '@/lib/format';
import { useResolvedTheme } from '@/lib/theme';
import { EChart } from './EChart';
import { exerciseLabel } from './ExerciseList';
import { Card, CardBody, CardHeader } from './ui/card';

/** Break lines where the tracker recorded nothing (e.g. while charging). */
function withGaps<T extends [string, ...unknown[]]>(rows: T[], maxGapMin: number): (T | [string, null, null, null])[] {
  const out: (T | [string, null, null, null])[] = [];
  for (let i = 0; i < rows.length; i++) {
    if (i && minutesBetween(rows[i - 1][0], rows[i][0]) > maxGapMin) out.push([rows[i - 1][0], null, null, null]);
    out.push(rows[i]);
  }
  return out;
}

/** Heart rate (top) and steps (bottom) on one shared, zoomable time axis. */
export function DayTimeline({ day }: { day: DayDetail }) {
  const theme = useResolvedTheme();
  const option = useMemo(() => {
    const t = chrome();
    const { c } = t;
    const rows = withGaps(day.heartRate, 5);
    const sleepArea = day.sleep ? [[{ xAxis: day.sleep.start, name: 'Spánek' }, { xAxis: day.sleep.end }]] : [];
    const exerciseAreas = day.exercises.map((e) => [{ xAxis: e.start, name: exerciseLabel(e) }, { xAxis: e.end }]);
    const x = { ...t.timeAxis(day.timeZone), min: day.range[0], max: day.range[1] };
    return {
      animation: false,
      textStyle: t.textStyle,
      axisPointer: { link: [{ xAxisIndex: 'all' }], lineStyle: { color: c.axis } },
      tooltip: {
        ...t.tooltip,
        trigger: 'axis',
        axisPointer: { type: 'line' },
        formatter: (ps: { seriesId: string; value: [string, number | null] }[]) => {
          const ts = ps[0]?.value[0];
          if (!ts) return '';
          const r = day.heartRate.find((h) => h[0] === ts);
          const s = day.steps.find((st) => Date.parse(st[0]) <= Date.parse(ts) && Date.parse(ts) < Date.parse(st[0]) + 15 * 60_000);
          let html = `<div style="font-weight:600;margin-bottom:2px">${clock(ts)}</div>`;
          if (r) html += row(c.s1, 'Tep', `${num(r[2])} bpm`) + row(c.band, 'Rozsah', `${num(r[1])}–${num(r[3])}`);
          if (s) html += row(c.s3, 'Kroky (15 min)', num(s[1]));
          return html;
        },
      },
      grid: [
        { left: 40, right: 12, top: 12, height: '62%' },
        { left: 40, right: 12, top: '76%', bottom: 26 },
      ],
      xAxis: [
        { ...x, gridIndex: 0, axisLabel: { show: false }, axisLine: { show: false } },
        { ...x, gridIndex: 1 },
      ],
      yAxis: [
        { ...t.valueAxis, gridIndex: 0, scale: true, min: (v: { min: number }) => Math.max(30, Math.floor(v.min / 10) * 10 - 10), axisLabel: { ...t.axisLabel, showMinLabel: false } },
        { ...t.valueAxis, gridIndex: 1, splitNumber: 2, axisLabel: { ...t.axisLabel, formatter: (v: number) => (v >= 1000 ? `${v / 1000}k` : String(v)) } },
      ],
      dataZoom: [{ type: 'inside', xAxisIndex: [0, 1], filterMode: 'none', minValueSpan: 30 * 60_000 }],
      series: [
        { id: 'lo', type: 'line', data: rows.map((r) => [r[0], r[1]]), stack: 'band', symbol: 'none', lineStyle: { opacity: 0 }, silent: true, tooltip: { show: false } },
        {
          id: 'span', type: 'line', data: rows.map((r) => [r[0], r[1] == null ? null : (r[3] as number) - (r[1] as number)]),
          stack: 'band', symbol: 'none', lineStyle: { opacity: 0 }, areaStyle: { color: c.band, opacity: 0.55 }, silent: true,
        },
        {
          id: 'avg', type: 'line', data: rows.map((r) => [r[0], r[2]]), symbol: 'none', lineStyle: { width: 1.5, color: c.s1 }, itemStyle: { color: c.s1 },
          markArea: {
            silent: true,
            label: { color: c.muted, fontSize: 11, position: 'insideTop', distance: 4 },
            data: [
              ...sleepArea.map((a) => [{ ...a[0], itemStyle: { color: c.shade } }, a[1]]),
              ...exerciseAreas.map((a) => [{ ...a[0], itemStyle: { color: c.s2, opacity: 0.12 } }, a[1]]),
            ],
          },
          markLine: Date.parse(day.range[0]) < Date.parse(day.midnight) ? {
            silent: true, symbol: 'none', label: { show: false },
            lineStyle: { color: c.axis, type: 'dashed', width: 1 }, data: [{ xAxis: day.midnight }],
          } : undefined,
        },
        {
          id: 'steps', type: 'bar', xAxisIndex: 1, yAxisIndex: 1,
          data: day.steps.map(([ts, v]) => [new Date(Date.parse(ts) + 7.5 * 60_000).toISOString(), v]),
          barWidth: '55%', itemStyle: { color: c.s3, borderRadius: [2, 2, 0, 0] },
        },
      ],
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [day, theme]);

  const totalSteps = day.steps.reduce((a, s) => a + s[1], 0);
  return (
    <Card>
      <CardHeader
        title="Tep a pohyb"
        subtitle="Průměr za minutu s pásmem min–max, kroky po 15 minutách. Kolečkem nebo gestem lze přiblížit."
        action={
          <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-ink-2">
            <Legend color="var(--s1)" label="Tep" />
            <Legend color="var(--band)" label="Min–max" />
            <Legend color="var(--s3)" label={`Kroky (${num(totalSteps)})`} />
            {day.sleep && <Legend color="var(--shade)" label="Spánek" border />}
            {day.exercises.length > 0 && <Legend color="color-mix(in srgb, var(--s2) 25%, transparent)" label="Cvičení" />}
          </div>
        }
      />
      <CardBody className="pt-1">
        <EChart option={option} className="h-[340px]" label="Graf tepu a kroků během dne" />
      </CardBody>
    </Card>
  );
}

export function Legend({ color, label, border }: { color: string; label: string; border?: boolean }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <i className="inline-block size-2.5 rounded-[3px]" style={{ background: color, border: border ? '1px solid var(--axis)' : undefined }} />
      {label}
    </span>
  );
}
