import { RefreshCw } from 'lucide-react';
import type { SyncProgress } from '@/lib/api';
import { Card, CardBody } from './ui/card';

/** What the first sync is doing, and roughly how far along it is (0–100). */
function describe(p: SyncProgress | null | undefined): { label: string; percent: number } {
  switch (p?.step) {
    case 'recent':
      return { label: 'Checking your data and downloading the last few days', percent: 8 };
    case 'history': {
      const share = p.total ? p.done / p.total : 0;
      return {
        label: `${p.done} of ${p.total} data types downloaded`,
        percent: 12 + Math.round(78 * share),
      };
    }
    case 'tables':
      return { label: 'Building your charts', percent: 94 };
    default:
      return { label: 'Starting the sync', percent: 3 };
  }
}

/**
 * Shown instead of the empty dashboard while a new user's whole history is
 * downloading; the page reloads its data by itself when the sync ends.
 */
export function FirstSync({ progress }: { progress?: SyncProgress | null }) {
  const { label, percent } = describe(progress);
  return (
    <Card>
      <CardBody className="mx-auto flex max-w-md flex-col items-center py-14 text-center">
        <span className="grid size-12 place-items-center rounded-full bg-s1/12 text-s1">
          <RefreshCw className="size-6 animate-spin [animation-duration:2s]" />
        </span>
        <h2 className="mt-4 text-lg font-semibold tracking-tight">Downloading your Fitbit history</h2>
        <p className="mt-1 text-sm text-ink-2" aria-live="polite">
          {label}
        </p>
        <div
          className="mt-5 h-1.5 w-full overflow-hidden rounded-full bg-grid"
          role="progressbar"
          aria-label="Sync progress"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
        >
          <div className="h-full rounded-full bg-s1 transition-[width] duration-700 ease-out" style={{ width: `${percent}%` }} />
        </div>
        <p className="mt-4 text-[13px] text-muted">
          The first sync takes a few minutes. This page updates by itself, and you can close it and come back later.
        </p>
      </CardBody>
    </Card>
  );
}
