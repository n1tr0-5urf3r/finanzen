import type { QueryClient } from '@tanstack/react-query';

/**
 * One place where query keys are built.
 *
 * The aggregates in this app are heavily interdependent: a single booking change
 * moves the dashboard, the monthly overview, the category analysis and the tax
 * report for that year. Ad-hoc string arrays would drift, so keys are namespaced
 * and the invalidation rules live next to them.
 */
export const qk = {
  auth: {
    setupStatus: () => ['auth', 'setup-status'] as const,
    me: () => ['auth', 'me'] as const,
  },
  taxonomy: {
    categories: () => ['taxonomy', 'categories'] as const,
    types: () => ['taxonomy', 'types'] as const,
    rules: () => ['taxonomy', 'rules'] as const,
  },
  bookings: {
    root: ['bookings'] as const,
    list: (filters: Record<string, unknown>) => ['bookings', 'list', filters] as const,
    one: (id: string) => ['bookings', 'one', id] as const,
    comments: () => ['bookings', 'comments'] as const,
  },
  derived: {
    root: ['derived'] as const,
    year: (year: number) => ['derived', year] as const,
    dashboard: (year: number) => ['derived', year, 'dashboard'] as const,
    months: (year: number) => ['derived', year, 'months'] as const,
    categories: (year: number) => ['derived', year, 'categories'] as const,
    tax: (year: number) => ['derived', year, 'tax'] as const,
  },
  years: () => ['years'] as const,
  recurring: {
    root: ['recurring'] as const,
    /** Scoped by period: due/booked flags are answers about one month. */
    list: (year: number, month: number) => ['recurring', 'list', year, month] as const,
  },
  drafts: (year: number, month: number) => ['bookings', 'drafts', year, month] as const,
  imports: {
    root: ['imports'] as const,
    list: () => ['imports', 'list'] as const,
    one: (id: string) => ['imports', 'one', id] as const,
    review: (id: string) => ['imports', 'review', id] as const,
  },
} as const;

/**
 * A booking changed. Every aggregate for the affected years is stale, and so is
 * the comment list that feeds Quick Add's suggestions — but other years are not,
 * which is the whole reason this is scoped rather than a blanket reset.
 */
export function invalidateAfterBookingChange(client: QueryClient, years: number[]) {
  client.invalidateQueries({ queryKey: qk.bookings.root });
  client.invalidateQueries({ queryKey: qk.years() });
  for (const year of new Set(years)) {
    client.invalidateQueries({ queryKey: qk.derived.year(year) });
  }
}

/**
 * A rule or category changed. This recategorises history, so every year's
 * aggregates move — including years the user is not looking at.
 */
export function invalidateAfterTaxonomyChange(client: QueryClient) {
  client.invalidateQueries({ queryKey: qk.taxonomy.rules() });
  client.invalidateQueries({ queryKey: qk.taxonomy.categories() });
  client.invalidateQueries({ queryKey: qk.bookings.root });
  client.invalidateQueries({ queryKey: qk.derived.root });
}

/**
 * Templates were materialised. Bookings changed, every aggregate for that year
 * changed, and the checklist's own due/booked flags changed — the last of which
 * is the one a blanket booking invalidation would miss.
 */
export function invalidateAfterMaterialize(client: QueryClient, year: number) {
  client.invalidateQueries({ queryKey: qk.recurring.root });
  invalidateAfterBookingChange(client, [year]);
}

/** Only the year overview and that year's aggregates; bookings are untouched. */
export function invalidateAfterYearChange(client: QueryClient, year: number) {
  client.invalidateQueries({ queryKey: qk.years() });
  client.invalidateQueries({ queryKey: qk.derived.year(year) });
}
