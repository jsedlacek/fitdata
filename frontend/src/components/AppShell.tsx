import { Link, Outlet } from '@tanstack/react-router';
import { useEffect, useReducer, useRef } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Activity, CheckCircle2, Laptop, Moon, RefreshCw, Sun, TriangleAlert } from 'lucide-react';
import { overviewQuery, startSync, syncQuery, type SyncRun } from '@/lib/api';
import { ago } from '@/lib/format';
import { cn } from '@/lib/utils';
import { Button } from './ui/button';
import { useThemeChoice, type ThemeChoice } from '@/lib/theme';
import { Segmented } from './ui/segmented';
import { ReconnectBanner } from './ReconnectBanner';

export function AppShell() {
  const { data } = useQuery(overviewQuery);
  return (
    <div className="min-h-dvh">
      <header className="sticky top-0 z-40 border-b bg-page/85 backdrop-blur supports-[backdrop-filter]:bg-page/70">
        <div className="mx-auto flex h-14 max-w-6xl items-center gap-3 px-4 sm:gap-6">
          <Link to="/" search={{}} className="flex items-center gap-2 font-semibold tracking-tight">
            <span className="grid size-7 place-items-center rounded-lg bg-s1 text-white">
              <Activity className="size-4" strokeWidth={2.5} />
            </span>
            <span className="hidden sm:inline">fitdata</span>
          </Link>
          <nav className="flex items-center gap-1 text-sm">
            <NavLink to="/">Den</NavLink>
            <NavLink to="/trends">Trendy</NavLink>
          </nav>
          <div className="flex-1" />
          <SyncStatus run={data?.lastSync ?? null} />
          <ThemeToggle />
        </div>
      </header>
      <main className="mx-auto max-w-6xl px-4 pb-16 pt-5">
        <ReconnectBanner run={data?.lastSync ?? null} />
        <Outlet />
      </main>
    </div>
  );
}

function NavLink({ to, children }: { to: '/' | '/trends'; children: React.ReactNode }) {
  return (
    <Link
      to={to}
      className="rounded-md px-2.5 py-1.5 text-ink-2 transition-colors hover:bg-shade hover:text-ink"
      activeProps={{ className: 'bg-shade !text-ink font-medium' }}
      activeOptions={{ exact: true, includeSearch: false }}
    >
      {children}
    </Link>
  );
}

function SyncStatus({ run }: { run: SyncRun | null }) {
  const queryClient = useQueryClient();
  const sync = useQuery(syncQuery);
  const start = useMutation({
    mutationFn: startSync,
    onSuccess: (state) => queryClient.setQueryData(syncQuery.queryKey, state),
  });
  const running = start.isPending || !!sync.data?.running;

  // When a run (ours or the scheduled one) finishes, reload the data.
  const wasRunning = useRef(false);
  useEffect(() => {
    if (wasRunning.current && !running) {
      void queryClient.invalidateQueries({ predicate: (q) => q.queryKey[0] !== 'sync' });
    }
    wasRunning.current = running;
  }, [running, queryClient]);

  // Re-render every minute so "před 5 min" keeps counting.
  const [, tick] = useReducer((n: number) => n + 1, 0);
  useEffect(() => {
    const id = setInterval(tick, 60_000);
    return () => clearInterval(id);
  }, []);

  const Icon = run?.ok ? CheckCircle2 : TriangleAlert;
  return (
    <div className="flex items-center gap-1.5 text-[13px] text-ink-2">
      {running ? (
        <span className="hidden md:inline">Synchronizuji…</span>
      ) : (
        run && (
          <span
            className="hidden items-center gap-1.5 md:flex"
            title={`Poslední synchronizace ${new Date(run.run_at).toLocaleString('cs-CZ')}`}
          >
            <Icon className={run.ok ? 'size-4 text-good' : 'size-4 text-critical'} />
            <span>{run.ok ? 'Synchronizováno' : 'Synchronizace selhala'}</span>
            <span className="text-muted">{ago(run.run_at)}</span>
          </span>
        )
      )}
      <Button
        variant="ghost"
        size="icon-sm"
        className={cn('text-ink-2 hover:text-ink disabled:opacity-100', start.isError && 'text-critical')}
        disabled={running}
        onClick={() => start.mutate()}
        aria-label="Synchronizovat teď"
        title={start.isError ? 'Synchronizaci se nepodařilo spustit, zkusit znovu' : running ? 'Synchronizace běží' : 'Synchronizovat teď'}
      >
        <RefreshCw className={cn(running && 'animate-spin')} />
      </Button>
    </div>
  );
}

function ThemeToggle() {
  const [choice, setChoice] = useThemeChoice();
  return (
    <Segmented<ThemeChoice>
      label="Vzhled"
      value={choice}
      onChange={setChoice}
      options={[
        { value: 'light', label: <Sun />, title: 'Světlý' },
        { value: 'system', label: <Laptop />, title: 'Podle systému' },
        { value: 'dark', label: <Moon />, title: 'Tmavý' },
      ]}
    />
  );
}
