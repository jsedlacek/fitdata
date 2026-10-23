import { useMemo } from 'react';
import { BedDouble, Moon, Sunrise, Timer } from 'lucide-react';
import type { Night, Stage } from '@/lib/api';
import { chrome } from '@/lib/chart';
import { clock, duration, num } from '@/lib/format';
import { useResolvedTheme } from '@/lib/theme';
import { EChart } from './EChart';
import { Card, CardBody, CardHeader } from './ui/card';

/** Stages in the fixed categorical color order; rows top to bottom as awake → deep. */
export const STAGES: { key: Stage; label: string; color: string }[] = [
  { key: 'DEEP', label: 'Hluboký', color: '--s1' },
  { key: 'LIGHT', label: 'Lehký', color: '--s2' },
  { key: 'REM', label: 'REM', color: '--s3' },
  { key: 'AWAKE', label: 'Bdění', color: '--s4' },
];
const ROW: Record<Stage, number> = { DEEP: 0, LIGHT: 1, REM: 2, AWAKE: 3 };

export function SleepCard({ night }: { night: Night | null }) {
  const theme = useResolvedTheme();
  const option = useMemo(() => {
    if (!night) return null;
    const t = chrome();
    const color = Object.fromEntries(STAGES.map((s) => [s.key, t.c[s.color.slice(2) as 's1']])) as Record<Stage, string>;
    const label = Object.fromEntries(STAGES.map((s) => [s.key, s.label])) as Record<Stage, string>;
    return {
      animation: false,
      textStyle: t.textStyle,
      grid: { left: 64, right: 12, top: 8, bottom: 24 },
      tooltip: {
        ...t.tooltip,
        trigger: 'item',
        formatter: (p: { value: [string, string, number, Stage] }) => {
          const [s, e, , st] = p.value;
          return `<b>${label[st]}</b><br>${clock(s)}–${clock(e)} · ${Math.round((+new Date(e) - +new Date(s)) / 60000)} min`;
        },
      },
      xAxis: { ...t.timeAxis, min: night.start, max: night.end },
      yAxis: {
        type: 'category', data: STAGES.map((s) => s.label), axisLabel: t.axisLabel,
        axisLine: { show: false }, axisTick: { show: false }, splitLine: { show: true, lineStyle: { color: t.c.grid } },
      },
      series: [{
        type: 'custom',
        data: night.stages.map((s) => [s.start, s.end, ROW[s.stage], s.stage]),
        encode: { x: [0, 1], y: 2 },
        renderItem: (_: unknown, api: { value: (i: number) => number | string; coord: (v: unknown[]) => number[]; size: (v: number[]) => number[] }) => {
          const a = api.coord([api.value(0), api.value(2)]);
          const b = api.coord([api.value(1), api.value(2)]);
          const h = api.size([0, 1])[1] * 0.62;
          return {
            type: 'rect',
            shape: { x: a[0], y: a[1] - h / 2, width: Math.max(b[0] - a[0] - 1, 1.5), height: h, r: 3 },
            style: { fill: color[api.value(3) as Stage] },
          };
        },
      }],
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [night, theme]);

  if (!night) {
    return (
      <Card>
        <CardHeader title="Spánek" />
        <CardBody>
          <p className="py-8 text-center text-sm text-muted">Pro tuhle noc nejsou data o spánku.</p>
        </CardBody>
      </Card>
    );
  }

  const asleep = night.minutes_asleep ?? 0;
  const inBed = (+new Date(night.end) - +new Date(night.start)) / 60000;
  const total = STAGES.reduce((a, s) => a + (night.stage_minutes[s.key] ?? 0), 0) || 1;

  return (
    <Card>
      <CardHeader title="Spánek" subtitle="Noc před vybraným dnem; víc záznamů z jedné noci je spojených." />
      <CardBody>
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <Stat icon={Moon} label="Usnutí" value={clock(night.start)} />
          <Stat icon={Sunrise} label="Probuzení" value={clock(night.end)} />
          <Stat icon={BedDouble} label="Spánek" value={duration(asleep)} />
          <Stat icon={Timer} label="Efektivita" value={`${num((asleep / inBed) * 100)} %`} hint={`z ${duration(inBed)} v posteli`} />
        </div>

        <div className="mt-4 flex h-2.5 overflow-hidden rounded-full" aria-hidden>
          {STAGES.map((s) => (
            <div key={s.key} style={{ width: `${((night.stage_minutes[s.key] ?? 0) / total) * 100}%`, background: `var(${s.color})` }} className="border-r-2 border-surface last:border-r-0" />
          ))}
        </div>
        <ul className="mt-2 grid grid-cols-2 gap-x-6 gap-y-1 text-xs">
          {STAGES.map((s) => {
            const m = night.stage_minutes[s.key] ?? 0;
            return (
              <li key={s.key} className="flex items-center gap-1.5 whitespace-nowrap text-ink-2">
                <i className="inline-block size-2.5 rounded-[3px]" style={{ background: `var(${s.color})` }} />
                {s.label}
                <span className="ml-auto tabular-nums text-ink">{duration(m)}</span>
                <span className="w-9 text-right tabular-nums text-muted">{num((m / total) * 100)} %</span>
              </li>
            );
          })}
        </ul>

        {option && <EChart option={option} className="mt-3 h-[180px]" label="Hypnogram: fáze spánku v čase" />}
      </CardBody>
    </Card>
  );
}

function Stat({ icon: Icon, label, value, hint }: { icon: typeof Moon; label: string; value: string; hint?: string }) {
  return (
    <div className="rounded-lg bg-surface-2 px-3 py-2">
      <div className="flex items-center gap-1.5 text-xs text-ink-2">
        <Icon className="size-3.5 text-muted" />
        {label}
      </div>
      <div className="mt-0.5 text-lg font-semibold tabular-nums">{value}</div>
      {hint && <div className="text-[11px] text-muted">{hint}</div>}
    </div>
  );
}
