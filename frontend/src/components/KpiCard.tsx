import { ArrowDownRight, ArrowUpRight, Minus } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';

/** Headline number with a 14-day sparkline and the change against the recent average. */
export function KpiCard({
  icon: Icon,
  label,
  value,
  unit,
  delta,
  baselineLabel,
  spark,
  accent = 'var(--s1)',
}: {
  icon: LucideIcon;
  label: string;
  value: string;
  unit?: string;
  delta?: string | null;
  baselineLabel?: string;
  spark: (number | null)[];
  accent?: string;
}) {
  const sign = delta?.startsWith('+') ? 1 : delta?.startsWith('−') ? -1 : 0;
  const Arrow = sign > 0 ? ArrowUpRight : sign < 0 ? ArrowDownRight : Minus;
  return (
    <div className="flex flex-col gap-2 rounded-xl border bg-surface px-4 py-3.5 shadow-[0_1px_2px_rgba(0,0,0,0.04)]">
      <div className="flex items-center gap-2 text-[13px] text-ink-2">
        <Icon className="size-4" style={{ color: accent }} />
        {label}
      </div>
      <div className="flex items-end justify-between gap-3">
        <div className="text-[26px] font-semibold leading-none tracking-tight">
          {value}
          {unit && value !== '–' && <span className="ml-1 text-sm font-normal text-ink-2">{unit}</span>}
        </div>
        <Sparkline values={spark} color={accent} />
      </div>
      <div className={cn('flex min-h-4 items-center gap-1 text-xs tabular-nums text-muted')}>
        {delta && (
          <>
            <Arrow className="size-3.5" />
            <span className="text-ink-2">{delta}</span>
            <span>{baselineLabel}</span>
          </>
        )}
      </div>
    </div>
  );
}

/** Tiny line of the last values; the latest point is marked. */
function Sparkline({ values, color }: { values: (number | null)[]; color: string }) {
  const pts = values.map((v, i) => [i, v] as const).filter((p): p is readonly [number, number] => p[1] != null);
  if (pts.length < 2) return <div className="h-7 w-20" />;
  const w = 80, h = 28, pad = 3;
  const ys = pts.map((p) => p[1]);
  const lo = Math.min(...ys), hi = Math.max(...ys);
  const span = hi - lo || 1;
  const x = (i: number) => pad + (i / Math.max(values.length - 1, 1)) * (w - 2 * pad);
  const y = (v: number) => h - pad - ((v - lo) / span) * (h - 2 * pad);
  const d = pts.map(([i, v], k) => `${k ? 'L' : 'M'}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join('');
  const [li, lv] = pts[pts.length - 1];
  return (
    <svg width={w} height={h} className="hidden shrink-0 overflow-visible min-[440px]:block" aria-hidden>
      <path d={d} fill="none" stroke={color} strokeWidth={1.5} strokeLinejoin="round" strokeLinecap="round" opacity={0.85} />
      <circle cx={x(li)} cy={y(lv)} r={2.75} fill={color} stroke="var(--surface)" strokeWidth={1.5} />
    </svg>
  );
}
