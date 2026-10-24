import { useEffect, useState } from 'react';
import { CheckCircle2, KeyRound, Watch } from 'lucide-react';
import type { Me, SyncRun } from '@/lib/api';
import { Button } from './ui/button';

/**
 * Asks a new user to connect Fitbit, and offers reconnecting only when the
 * last sync could not use the token.
 */
export function ReconnectBanner({ run, user }: { run: SyncRun | null; user: Me | null }) {
  const [justConnected] = useState(() => new URLSearchParams(location.search).has('fitbit'));
  useEffect(() => {
    if (justConnected) history.replaceState(null, '', location.pathname);
  }, [justConnected]);

  if (justConnected) {
    return (
      <div role="status" className="mb-5 flex items-start gap-3 rounded-xl border border-l-4 border-l-good bg-surface px-4 py-3 text-sm">
        <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-good" />
        <p>
          {run
            ? 'Přístup k datům je obnovený. Chybějící dny se doplní při příští synchronizaci (každou hodinu).'
            : 'Fitbit je připojený. Načítám historii dat, první dny se objeví za pár minut.'}
        </p>
      </div>
    );
  }
  if (user && !user.connected) {
    return (
      <div role="status" className="mb-5 flex flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border border-l-4 border-l-s1 bg-surface px-4 py-3 text-sm">
        <Watch className="size-4 shrink-0 text-s1" />
        <p className="flex-1">Připoj svůj Fitbit, ať se tu objeví tvoje data. Přístup je jen ke čtení.</p>
        <Button asChild size="sm">
          <a href="/auth/fitbit">Připojit Fitbit</a>
        </Button>
      </div>
    );
  }
  if (!run?.auth_error) return null;
  return (
    <div role="alert" className="mb-5 flex flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border border-l-4 border-l-warning bg-surface px-4 py-3 text-sm">
      <KeyRound className="size-4 shrink-0 text-warning" />
      <p className="flex-1">Synchronizace ztratila přístup k datům z Google Health, nová data nepřibývají.</p>
      <Button asChild size="sm">
        <a href="/auth/fitbit">Obnovit přístup k datům</a>
      </Button>
    </div>
  );
}
