import { useMemo } from 'react';
import type { DayDetail } from '@/lib/api';
import { chrome, minutesBetween, wallClock } from '@/lib/chart';
import { num } from '@/lib/format';
import { useResolvedTheme } from '@/lib/theme';
import { EChart } from './EChart';
import { Card, CardBody, CardHeader } from './ui/card';

function series(points: [string, number][], maxGapMin: number) {
  const out: [string, number | null][] = [];
  points.forEach((p, i) => {
    if (i && minutesBetween(points[i - 1][0], p[0]) > maxGapMin) out.push([points[i - 1][0], null]);
    out.push(p);
  });
  return out;
}

function VitalChart({ title, subtitle, points, timeZone, unit, digits, gap, summary, label }: {
  title: string; subtitle: string; points: [string, number][]; timeZone: string; unit: string; digits: number; gap: number; summary: string; label: string;
}) {
  const theme = useResolvedTheme();
  const option = useMemo(() => {
    const t = chrome();
    const clock = wallClock(timeZone);
    return {
      animation: false,
      textStyle: t.textStyle,
      grid: { left: 36, right: 10, top: 10, bottom: 24 },
      tooltip: {
        ...t.tooltip, trigger: 'axis',
        formatter: (ps: { value: [string, number | null] }[]) =>
          ps[0]?.value[1] == null ? '' : `${clock(ps[0].value[0])}<br><b>${num(ps[0].value[1], digits)}</b> ${unit}`,
      },
      xAxis: t.timeAxis(timeZone),
      yAxis: { ...t.valueAxis, scale: true },
      series: [{
        type: 'line', data: series(points, gap), symbol: 'none', smooth: 0.2,
        lineStyle: { width: 1.5, color: t.c.s1 }, itemStyle: { color: t.c.s1 },
      }],
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [points, timeZone, theme]);
  return (
    <Card>
      <CardHeader title={title} subtitle={subtitle} action={<span className="text-sm font-semibold tabular-nums">{summary}</span>} />
      <CardBody className="pt-1">
        {points.length ? (
          <EChart option={option} className="h-[170px]" label={label} />
        ) : (
          <p className="py-12 text-center text-sm text-muted">Žádná měření.</p>
        )}
      </CardBody>
    </Card>
  );
}

export function NightVitals({ day }: { day: DayDetail }) {
  const m = day.metrics;
  return (
    <div className="grid gap-4 md:grid-cols-2">
      <VitalChart
        title="SpO₂ v noci" subtitle="Saturace krve kyslíkem, %" points={day.spo2} timeZone={day.timeZone} unit="%" digits={1} gap={10}
        summary={m?.spo2_avg != null ? `Ø ${num(m.spo2_avg, 1)} % · ${num(m.spo2_low, 1)}–${num(m.spo2_high, 1)}` : ''}
        label="Graf saturace kyslíkem během noci"
      />
      <VitalChart
        title="HRV v noci" subtitle="RMSSD v 5minutových oknech, ms" points={day.hrv} timeZone={day.timeZone} unit="ms" digits={1} gap={15}
        summary={m?.hrv_ms != null ? `Ø ${num(m.hrv_ms, 1)} ms` : ''}
        label="Graf variability srdečního tepu během noci"
      />
    </div>
  );
}
