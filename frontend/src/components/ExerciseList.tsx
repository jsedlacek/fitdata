import { Bike, Dumbbell, Footprints, Waves, Zap } from 'lucide-react';
import type { Exercise } from '@/lib/api';
import { clock, duration, num, plural } from '@/lib/format';
import { Card, CardBody, CardHeader } from './ui/card';

const KIND: Record<string, { label: string; icon: typeof Bike }> = {
  BIKING: { label: 'Cycling', icon: Bike },
  WALKING: { label: 'Walking', icon: Footprints },
  RUNNING: { label: 'Running', icon: Footprints },
  SWIMMING_POOL: { label: 'Swimming', icon: Waves },
  SWIMMING_OPEN_WATER: { label: 'Open water swimming', icon: Waves },
};

const kind = (k: string) => KIND[k] ?? { label: k.toLowerCase().replace(/_/g, ' '), icon: Dumbbell };

/** The recorded name, else the activity type in words. */
export const exerciseLabel = (e: Exercise) => e.name || kind(e.kind).label;

export function ExerciseList({ exercises }: { exercises: Exercise[] }) {
  if (!exercises.length) return null;
  return (
    <Card>
      <CardHeader title="Exercise" subtitle={`${exercises.length} ${plural(exercises.length, 'session', 'sessions')}`} />
      <CardBody className="grid gap-3">
        {exercises.map((e) => {
          const k = kind(e.kind);
          const mins = (+new Date(e.end) - +new Date(e.start)) / 60000;
          return (
            <div key={e.start} className="flex gap-3 rounded-lg bg-surface-2 p-3">
              <span className="grid size-10 shrink-0 place-items-center rounded-lg bg-s2/15 text-s2">
                <k.icon className="size-5" />
              </span>
              <div className="min-w-0 flex-1">
                <div className="flex items-baseline justify-between gap-2">
                  <span className="truncate font-medium">{exerciseLabel(e)}</span>
                  <span className="shrink-0 text-xs tabular-nums text-muted">{clock(e.start)}–{clock(e.end)}</span>
                </div>
                <dl className="mt-2 flex flex-wrap gap-x-5 gap-y-1 text-xs">
                  <Metric label="Duration" value={duration(e.active_minutes ?? mins)} />
                  <Metric label="Avg HR" value={e.avg_hr != null ? `${num(e.avg_hr)} bpm` : '–'} />
                  <Metric label="kcal" value={num(e.kcal)} />
                  <Metric label={<span className="inline-flex items-center gap-0.5"><Zap className="size-3" />AZM</span>} title="Active Zone Minutes" value={num(e.azm)} />
                </dl>
              </div>
            </div>
          );
        })}
      </CardBody>
    </Card>
  );
}

function Metric({ label, value, title }: { label: React.ReactNode; value: string; title?: string }) {
  return (
    <div title={title}>
      <dt className="whitespace-nowrap text-muted">{label}</dt>
      <dd className="whitespace-nowrap text-sm font-semibold tabular-nums text-ink">{value}</dd>
    </div>
  );
}
