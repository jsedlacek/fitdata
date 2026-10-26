import { Link, Outlet } from '@tanstack/react-router';
import { useEffect, useReducer, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { BookOpen, Bot, CheckCircle2, RefreshCw, Settings, TriangleAlert } from 'lucide-react';
import { overviewQuery, saveSettings, startSync, syncQuery, type Me, type Overview, type SyncRun } from '@/lib/api';
import { detectSettings, setPrefs } from '@/lib/prefs';
import { ago } from '@/lib/format';
import { cn } from '@/lib/utils';
import { Button } from './ui/button';
import { ReconnectBanner } from './ReconnectBanner';
import { Popover, PopoverContent, PopoverTrigger } from './ui/popover';
import { McpConnect } from './McpConnect';

export function AppShell() {
  const { data } = useQuery(overviewQuery);
  // Before the pages render: the formatting helpers read the settings.
  if (data) setPrefs(data.user.settings);
  useDetectedSettings(data?.user);
  return (
    <div className="min-h-dvh">
      <header className="sticky top-0 z-40 border-b bg-page/85 backdrop-blur supports-[backdrop-filter]:bg-page/70">
        <div className="mx-auto flex h-14 max-w-6xl items-center gap-2 px-4 sm:gap-6">
          <Link to="/" search={{}} className="flex shrink-0 items-center gap-2 font-semibold tracking-tight">
            <img src="/favicon.svg" alt="" className="size-7" />
            <span className="hidden sm:inline">Fitdata</span>
          </Link>
          <nav className="flex items-center gap-1 text-sm">
            <NavLink to="/">Day</NavLink>
            <NavLink to="/trends">Trends</NavLink>
          </nav>
          <div className="flex-1" />
          {data && <AskAi />}
          {data?.user.connected !== false && <SyncStatus run={data?.lastSync ?? null} />}
          <Button asChild variant="ghost" size="icon-sm" className="text-ink-2 hover:text-ink">
            <a href="/docs" target="_blank" rel="noopener" aria-label="Docs: what the metrics and charts mean" title="Docs: what the metrics and charts mean">
              <BookOpen />
            </a>
          </Button>
          {data && (
            <Button asChild variant="ghost" size="icon-sm" className="text-ink-2 hover:text-ink">
              <Link to="/settings" aria-label="Settings" title="Settings" activeProps={{ className: 'bg-shade !text-ink' }}>
                <Settings />
              </Link>
            </Button>
          )}
        </div>
      </header>
      <main className="mx-auto max-w-6xl px-4 pb-16 pt-5">
        <ReconnectBanner run={data?.lastSync ?? null} user={data?.user ?? null} />
        <Outlet />
      </main>
      {data && (
        <footer className="mx-auto flex max-w-6xl flex-wrap items-center gap-x-5 gap-y-2 px-4 pb-8 text-[13px] text-muted">
          <span className="flex gap-4 sm:ml-auto">
            <a href="/docs" className="hover:text-ink-2">Docs</a>
            <a href="/privacy" className="hover:text-ink-2">Privacy</a>
            <a href="/terms" className="hover:text-ink-2">Terms</a>
            <a href="https://github.com/jsedlacek/fitdata" className="hover:text-ink-2">GitHub</a>
          </span>
        </footer>
      )}
    </div>
  );
}

/** A new user's first visit saves settings from the browser: time zone, units, week start, clock. */
function useDetectedSettings(user: Me | undefined) {
  const queryClient = useQueryClient();
  const sent = useRef(false);
  useEffect(() => {
    if (!user || user.settingsSaved || sent.current) return;
    sent.current = true;
    saveSettings(detectSettings())
      .then((me) => queryClient.setQueryData<Overview>(overviewQuery.queryKey, (o) => o && { ...o, user: me }))
      .catch(() => {
        // Next visit tries again; the defaults show meanwhile.
      });
  }, [user, queryClient]);
}

const MCP_SEEN = 'mcp-seen';

/**
 * Asking Claude or ChatGPT is what sets Fitdata apart, so the header offers
 * it; a dot marks it until the instructions have been opened once.
 */
function AskAi() {
  const [seen, setSeen] = useState(() => {
    try {
      return localStorage.getItem(MCP_SEEN) === '1';
    } catch {
      return true;
    }
  });
  const open = (isOpen: boolean) => {
    if (!isOpen || seen) return;
    setSeen(true);
    try {
      localStorage.setItem(MCP_SEEN, '1');
    } catch {
      // Private mode: the dot comes back next time, nothing worse.
    }
  };
  return (
    <Popover onOpenChange={open}>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm" className="px-2 text-ink-2 hover:text-ink md:px-2.5" aria-label="Ask Claude or ChatGPT about your data">
          <span className="relative">
            <Bot />
            {!seen && <span className="absolute -right-1 -top-1 size-2 rounded-full bg-s1 ring-2 ring-page" />}
          </span>
          <span className="hidden md:inline">Ask Claude or ChatGPT</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-80">
        <McpConnect />
      </PopoverContent>
    </Popover>
  );
}

function NavLink({ to, children }: { to: '/' | '/trends'; children: React.ReactNode }) {
  return (
    <Link
      to={to}
      className="rounded-md px-2 py-1.5 text-ink-2 transition-colors sm:px-2.5 hover:bg-shade hover:text-ink"
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

  // One control: the state of the last sync, and a click starts a new one.
  const failed = start.isError || (run && !run.ok);
  const [Icon, color] = running
    ? [RefreshCw, '']
    : failed
      ? [TriangleAlert, 'text-critical']
      : run
        ? [CheckCircle2, 'text-good']
        : [RefreshCw, ''];
  const label = running ? 'Syncing…' : start.isError ? 'Could not start the sync' : run ? (run.ok ? 'Synced' : 'Sync failed') : 'Sync now';
  const title = running
    ? 'Sync in progress'
    : `${run ? `Last sync ${new Date(run.run_at).toLocaleString('en-GB')}. ` : ''}Click to sync now`;
  return (
    <Button
      variant="ghost"
      size="sm"
      className="group px-2 text-[13px] font-normal text-ink-2 hover:text-ink disabled:opacity-100 md:px-2.5"
      disabled={running}
      onClick={() => start.mutate()}
      aria-label={running ? 'Syncing' : 'Sync now'}
      title={title}
    >
      {/* At rest the icon tells how the last sync went; on hover it offers a new one. */}
      <Icon className={cn(color, running && 'animate-spin', !running && 'group-hover:hidden')} />
      {!running && <RefreshCw className="hidden group-hover:block" />}
      <span className="hidden md:inline">{label}</span>
      {run && !running && !start.isError && <span className="hidden text-muted md:inline">{ago(run.run_at)}</span>}
    </Button>
  );
}


