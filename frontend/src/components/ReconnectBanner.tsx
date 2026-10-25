import { useEffect, useState } from 'react';
import { CheckCircle2, KeyRound, Watch } from 'lucide-react';
import type { Me, SyncRun } from '@/lib/api';
import { Button } from './ui/button';

/**
 * Asks a new user to connect Fitbit, and offers reconnecting only when the
 * last sync could not use the token.
 */
export function ReconnectBanner({ run, user }: { run: SyncRun | null; user: Me | null }) {
  // Set by the sign-in only for a user who has synced before; a new user's
  // first sync has its own card on the page (FirstSync).
  const [reconnected] = useState(() => new URLSearchParams(location.search).get('fitbit') === 'reconnected');
  useEffect(() => {
    if (reconnected) history.replaceState(null, '', location.pathname);
  }, [reconnected]);

  if (reconnected) {
    return (
      <div role="status" className="mb-5 flex items-start gap-3 rounded-xl border border-l-4 border-l-good bg-surface px-4 py-3 text-sm">
        <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-good" />
        <p>
          Access to your data is restored. A sync has started and will fill in the missing days.
        </p>
      </div>
    );
  }
  if (user && !user.connected) {
    return (
      <div role="status" className="mb-5 flex flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border border-l-4 border-l-s1 bg-surface px-4 py-3 text-sm">
        <Watch className="size-4 shrink-0 text-s1" />
        <p className="flex-1">Connect your Fitbit to see your data here. Access is read-only.</p>
        <Button asChild size="sm">
          <a href="/auth/fitbit">Connect Fitbit</a>
        </Button>
      </div>
    );
  }
  if (!run?.auth_error) return null;
  return (
    <div role="alert" className="mb-5 flex flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border border-l-4 border-l-warning bg-surface px-4 py-3 text-sm">
      <KeyRound className="size-4 shrink-0 text-warning" />
      <p className="flex-1">The sync lost access to your Google Health data, so no new data is coming in.</p>
      <Button asChild size="sm">
        <a href="/auth/fitbit">Restore access to data</a>
      </Button>
    </div>
  );
}
