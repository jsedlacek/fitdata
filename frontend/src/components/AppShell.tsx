import { Link, Outlet } from '@tanstack/react-router';
import { useEffect, useReducer, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowLeft, Bot, CheckCircle2, Laptop, LogOut, Moon, RefreshCw, Sun, Trash2, TriangleAlert, UserRound } from 'lucide-react';
import { deleteAccount, overviewQuery, startSync, syncQuery, type SyncRun } from '@/lib/api';
import { ago } from '@/lib/format';
import { cn } from '@/lib/utils';
import { Button } from './ui/button';
import { useThemeChoice, type ThemeChoice } from '@/lib/theme';
import { Segmented } from './ui/segmented';
import { ReconnectBanner } from './ReconnectBanner';
import { Popover, PopoverContent, PopoverTrigger } from './ui/popover';
import { McpConnect } from './McpConnect';

export function AppShell() {
  const { data } = useQuery(overviewQuery);
  return (
    <div className="min-h-dvh">
      <header className="sticky top-0 z-40 border-b bg-page/85 backdrop-blur supports-[backdrop-filter]:bg-page/70">
        <div className="mx-auto flex h-14 max-w-6xl items-center gap-3 px-4 sm:gap-6">
          <Link to="/" search={{}} className="flex items-center gap-2 font-semibold tracking-tight">
            <img src="/favicon.svg" alt="" className="size-7" />
            <span className="hidden sm:inline">Fitdata</span>
          </Link>
          <nav className="flex items-center gap-1 text-sm">
            <NavLink to="/">Day</NavLink>
            <NavLink to="/trends">Trends</NavLink>
          </nav>
          <div className="flex-1" />
          {data?.user.connected !== false && <SyncStatus run={data?.lastSync ?? null} />}
          <ThemeToggle />
          {data && <AccountMenu />}
        </div>
      </header>
      <main className="mx-auto max-w-6xl px-4 pb-16 pt-5">
        <ReconnectBanner run={data?.lastSync ?? null} user={data?.user ?? null} />
        <Outlet />
      </main>
      {data && (
        <footer className="mx-auto flex max-w-6xl flex-wrap items-center gap-x-5 gap-y-2 px-4 pb-8 text-[13px] text-muted">
          <Popover>
            <PopoverTrigger className="inline-flex items-center gap-1.5 rounded-md transition-colors hover:text-ink-2">
              <Bot className="size-3.5" /> Your data in Claude and ChatGPT (MCP)
            </PopoverTrigger>
            <PopoverContent align="start" side="top" className="w-80">
              <McpConnect />
            </PopoverContent>
          </Popover>
          <span className="flex gap-4 sm:ml-auto">
            <a href="/privacy" className="hover:text-ink-2">Privacy</a>
            <a href="/terms" className="hover:text-ink-2">Terms</a>
            <a href="https://github.com/jsedlacek/fitdata" className="hover:text-ink-2">GitHub</a>
          </span>
        </footer>
      )}
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

  // Re-render every minute so "5 min ago" keeps counting.
  const [, tick] = useReducer((n: number) => n + 1, 0);
  useEffect(() => {
    const id = setInterval(tick, 60_000);
    return () => clearInterval(id);
  }, []);

  const Icon = run?.ok ? CheckCircle2 : TriangleAlert;
  return (
    <div className="flex items-center gap-1.5 text-[13px] text-ink-2">
      {running ? (
        <span className="hidden md:inline">Syncing…</span>
      ) : (
        run && (
          <span
            className="hidden items-center gap-1.5 md:flex"
            title={`Last sync ${new Date(run.run_at).toLocaleString('en-GB')}`}
          >
            <Icon className={run.ok ? 'size-4 text-good' : 'size-4 text-critical'} />
            <span>{run.ok ? 'Synced' : 'Sync failed'}</span>
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
        aria-label="Sync now"
        title={start.isError ? 'Could not start the sync, try again' : running ? 'Sync in progress' : 'Sync now'}
      >
        <RefreshCw className={cn(running && 'animate-spin')} />
      </Button>
    </div>
  );
}

function AccountMenu() {
  const [confirming, setConfirming] = useState(false);
  const [mcp, setMcp] = useState(false);
  const remove = useMutation({ mutationFn: deleteAccount });
  const close = () => {
    setConfirming(false);
    setMcp(false);
  };
  return (
    <Popover onOpenChange={(open) => !open && close()}>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="icon-sm" className="text-ink-2 hover:text-ink" aria-label="Account">
          <UserRound />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className={cn('text-sm', mcp ? 'w-80' : 'w-72')}>
        {mcp ? (
          <>
            <Button variant="ghost" size="sm" className="-ml-2 mb-2 text-ink-2" onClick={() => setMcp(false)}>
              <ArrowLeft /> Back
            </Button>
            <McpConnect />
          </>
        ) : (
          <>
            <div className="flex flex-col gap-1">
              <Button variant="ghost" size="sm" className="justify-start" onClick={() => setMcp(true)}>
                <Bot /> Connect Claude / ChatGPT
              </Button>
              <Button asChild variant="ghost" size="sm" className="justify-start">
                <a href="/auth/logout">
                  <LogOut /> Sign out
                </a>
              </Button>
              {confirming ? (
                <div className="mt-1 rounded-lg border border-l-4 border-l-critical p-2.5">
                  <p>Delete your account and all downloaded data? This cannot be undone.</p>
                  <div className="mt-2 flex gap-2">
                    <Button size="sm" disabled={remove.isPending} onClick={() => remove.mutate()}>
                      {remove.isPending ? 'Deleting…' : 'Delete'}
                    </Button>
                    <Button variant="outline" size="sm" onClick={() => setConfirming(false)}>
                      Cancel
                    </Button>
                  </div>
                  {remove.isError && <p className="mt-2 text-critical">Could not delete the account, try again.</p>}
                </div>
              ) : (
                <Button variant="ghost" size="sm" className="justify-start text-critical" onClick={() => setConfirming(true)}>
                  <Trash2 /> Delete account
                </Button>
              )}
            </div>
          </>
        )}
      </PopoverContent>
    </Popover>
  );
}

function ThemeToggle() {
  const [choice, setChoice] = useThemeChoice();
  return (
    <Segmented<ThemeChoice>
      label="Theme"
      value={choice}
      onChange={setChoice}
      options={[
        { value: 'light', label: <Sun />, title: 'Light' },
        { value: 'system', label: <Laptop />, title: 'System' },
        { value: 'dark', label: <Moon />, title: 'Dark' },
      ]}
    />
  );
}
