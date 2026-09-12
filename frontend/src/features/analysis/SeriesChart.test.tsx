import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import { SeriesChart } from './SeriesChart';

vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: vi.fn() };
});
const { api } = await import('../../lib/api');
const mocked = vi.mocked(api);

const CATEGORIES = [
  { id: 'c1', name: 'Auto & Parken', typeCode: 'variabel', typeLabel: 'Variable Kosten',
    sortOrder: 1, archived: false, bookingCount: 33, netCents: 200000 },
];

// As the API returns them: most used first.
const SUBJECTS = [
  { comment: 'essen', bookingCount: 94, netCents: 110000, categoryName: 'Essen auswärts' },
  { comment: 'tanken', bookingCount: 17, netCents: 94980, categoryName: 'Auto & Parken' },
];

const month = (m: number, name: string, net: number, count: number) => ({
  month: m, monthName: name, incomeCents: 0, expenseCents: net, netCents: net, bookingCount: count,
});

const SERIES = {
  year: 2026,
  mode: 'comment',
  subject: 'tanken',
  categoryId: null,
  months: [
    month(1, 'Januar', 6_500, 1), month(2, 'Februar', 0, 0), month(3, 'März', 7_200, 1),
    month(4, 'April', 0, 0), month(5, 'Mai', 0, 0), month(6, 'Juni', 0, 0),
    month(7, 'Juli', 0, 0), month(8, 'August', 0, 0), month(9, 'September', 8_100, 1),
    month(10, 'Oktober', 0, 0), month(11, 'November', 0, 0), month(12, 'Dezember', 0, 0),
  ],
  incomeCents: 0,
  expenseCents: 21_800,
  netCents: 21_800,
  averagePerActiveMonthCents: 7_266,
  bookingCount: 3,
  monthsWithData: 3,
};

function route(path: string) {
  if (path.includes('/series/subjects')) return SUBJECTS;
  if (path.includes('/series')) return SERIES;
  return [];
}

beforeEach(() => {
  mocked.mockReset();
  mocked.mockImplementation((path: string) => Promise.resolve(route(path) as never));
});
afterEach(cleanup);

function renderChart() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <SeriesChart year={2026} categories={CATEGORIES as never} />
      </I18nProvider>
    </QueryClientProvider>,
  );
}

describe('the per-subject series', () => {
  /**
   * "How much do I spend on tanken" is the question this exists for — the
   * category table answers a different one.
   */
  it('charts one comment across twelve months, empty ones included', async () => {
    const { container } = renderChart();
    await screen.findByText(/tanken · 2026/);

    // Twelve bars even though only three months have data: a missing month and a
    // zero month are not the same thing, and the gap is the information.
    const bars = container.querySelectorAll('.chart__bar');
    expect(bars).toHaveLength(12);
  });

  it('averages over the months that have the subject, not over the year', async () => {
    renderChart();
    await screen.findByText(/tanken · 2026/);
    // 218,00 over three active months, not over nine or twelve — a summer-only
    // expense should not be made to look small by the months it never occurs in.
    expect(screen.getByText(/72,66/)).toBeInTheDocument();
    expect(screen.getAllByText('3', { selector: '.num' }).length).toBeGreaterThan(0);
  });

  it('offers the most-used comments first and can switch to categories', async () => {
    const user = userEvent.setup();
    renderChart();
    await screen.findByText(/tanken · 2026/);

    const picker = screen.getByRole('combobox');
    const options = [...picker.querySelectorAll('option')].map((o) => o.textContent);
    expect(options[0]).toContain('essen');
    expect(options[0]).toContain('94');

    await user.click(screen.getByRole('button', { name: 'Nach Kategorie' }));
    const afterSwitch = [...screen.getByRole('combobox').querySelectorAll('option')]
      .map((o) => o.textContent);
    expect(afterSwitch).toContain('Auto & Parken');
  });

  it('survives a payload that is not a list', async () => {
    // An error body, or a mock wired to the wrong endpoint, must not blank the
    // page — this exact shape crashed two other screens before asList existed.
    // There is nothing to chart without the suggestion list, but the section must
    // still render and still let a category be chosen.
    mocked.mockImplementation((path: string) =>
      Promise.resolve((path.includes('/series/subjects') ? { code: 'boom' } : SERIES) as never),
    );
    const { container } = renderChart();

    expect(await screen.findByRole('combobox')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Nach Kategorie' })).toBeInTheDocument();
    expect(container.querySelector('.series')).not.toBeNull();
  });
});
