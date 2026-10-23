import { useEffect, useState } from 'react';
import { CheckCircle2, KeyRound } from 'lucide-react';
import type { SyncRun } from '@/lib/api';
import { Button } from './ui/button';

/** Offers reconnecting only when the last sync could not use the token. */
export function ReconnectBanner({ run }: { run: SyncRun | null }) {
  const [justConnected] = useState(() => new URLSearchParams(location.search).has('fitbit'));
  useEffect(() => {
    if (justConnected) history.replaceState(null, '', location.pathname);
  }, [justConnected]);

  if (justConnected) {
    return (
      <div role="status" className="mb-5 flex items-start gap-3 rounded-xl border border-l-4 border-l-good bg-surface px-4 py-3 text-sm">
        <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-good" />
        <p>Přístup k datům je obnovený. Chybějící dny se doplní při příští synchronizaci (každou hodinu).</p>
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
