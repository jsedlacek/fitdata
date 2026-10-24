import { Link, Outlet } from '@tanstack/react-router';
import { useEffect, useReducer, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Activity, CheckCircle2, Laptop, LogOut, Moon, RefreshCw, Sun, Trash2, TriangleAlert, UserRound } from 'lucide-react';
import { deleteAccount, overviewQuery, startSync, syncQuery, type Me, type SyncRun } from '@/lib/api';
import { ago } from '@/lib/format';
import { cn } from '@/lib/utils';
import { Button } from './ui/button';
import { useThemeChoice, type ThemeChoice } from '@/lib/theme';
import { Segmented } from './ui/segmented';
import { ReconnectBanner } from './ReconnectBanner';
import { Popover, PopoverContent, PopoverTrigger } from './ui/popover';

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
          {data?.user.connected !== false && <SyncStatus run={data?.lastSync ?? null} />}
          <ThemeToggle />
          {data && <AccountMenu user={data.user} />}
        </div>
      </header>
      <main className="mx-auto max-w-6xl px-4 pb-16 pt-5">
        <ReconnectBanner run={data?.lastSync ?? null} user={data?.user ?? null} />
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

function AccountMenu({ user }: { user: Me }) {
  const [confirming, setConfirming] = useState(false);
  const remove = useMutation({ mutationFn: deleteAccount });
  return (
    <Popover onOpenChange={(open) => !open && setConfirming(false)}>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="icon-sm" className="text-ink-2 hover:text-ink" aria-label="Účet" title={user.email}>
          <UserRound />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-72 text-sm">
        <p className="truncate font-medium" title={user.email}>
          {user.email}
        </p>
        <div className="mt-3 flex flex-col gap-1">
          <Button asChild variant="ghost" size="sm" className="justify-start">
            <a href="/auth/logout">
              <LogOut /> Odhlásit
            </a>
          </Button>
          {confirming ? (
            <div className="mt-1 rounded-lg border border-l-4 border-l-critical p-2.5">
              <p>Smazat účet i všechna stažená data? Nejde to vrátit.</p>
              <div className="mt-2 flex gap-2">
                <Button size="sm" disabled={remove.isPending} onClick={() => remove.mutate()}>
                  {remove.isPending ? 'Mažu…' : 'Smazat'}
                </Button>
                <Button variant="outline" size="sm" onClick={() => setConfirming(false)}>
                  Zpět
                </Button>
              </div>
              {remove.isError && <p className="mt-2 text-critical">Smazání se nepodařilo, zkuste to znovu.</p>}
            </div>
          ) : (
            <Button variant="ghost" size="sm" className="justify-start text-critical" onClick={() => setConfirming(true)}>
              <Trash2 /> Smazat účet
            </Button>
          )}
        </div>
      </PopoverContent>
    </Popover>
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
