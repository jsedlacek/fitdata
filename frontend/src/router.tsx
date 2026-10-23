import { lazy, Suspense } from 'react';
import { createRootRoute, createRoute, createRouter } from '@tanstack/react-router';
import { AppShell } from '@/components/AppShell';
import { DayPage } from '@/routes/day';
import { TrendsPage, type Range } from '@/routes/trends';

const rootRoute = createRootRoute({ component: AppShell });

const dayRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  validateSearch: (s: Record<string, unknown>): { date?: string } =>
    typeof s.date === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(s.date) ? { date: s.date } : {},
  component: function Day() {
    const { date } = dayRoute.useSearch();
    return <DayPage date={date} />;
  },
});

// Chart library comparison; loaded on demand so Unovis stays out of the main bundle.
const LabPage = lazy(() => import('@/routes/lab'));

const labRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/lab',
  validateSearch: (s: Record<string, unknown>): { date?: string } =>
    typeof s.date === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(s.date) ? { date: s.date } : {},
  component: function Lab() {
    const { date } = labRoute.useSearch();
    return (
      <Suspense>
        <LabPage date={date} />
      </Suspense>
    );
  },
});

const RANGES: Range[] = ['7', '30', '90', 'all'];

const trendsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/trends',
  validateSearch: (s: Record<string, unknown>): { range?: Range } => {
    const r = String(s.range ?? '');
    return RANGES.includes(r as Range) ? { range: r as Range } : {};
  },
  component: function Trends() {
    const { range } = trendsRoute.useSearch();
    return <TrendsPage range={range ?? '30'} />;
  },
});

export const router = createRouter({
  routeTree: rootRoute.addChildren([dayRoute, trendsRoute, labRoute]),
  defaultPreload: 'intent',
});

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}
