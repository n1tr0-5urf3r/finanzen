import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { CategoryAnalysis, CategoryAnalysisRow } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { AnalysisPage } = await import('./AnalysisPage');
const { CompareRedirect } = await import('../compare/CompareRedirect');

function row(over: Partial<CategoryAnalysisRow>): CategoryAnalysisRow {
  return {
    categoryId: over.categoryName ?? 'x',
    categoryName: 'Sonstiges',
    categoryType: 'Sonstiges',
    incomeCents: 0,
    expenseCents: 0,
    netCents: 0,
    netIsNegative: false,
    shareOfTotal: 0,
    averagePerMonthCents: 0,
    bookingCount: 0,
    monthlyNetCents: Array.from({ length: 12 }, () => 0),
    ...over,
  };
}

const ANALYSIS: CategoryAnalysis = {
  year: 2026,
  rows: [
    // Both gross legs: rent is 10.200 out and 5.100 in because a flatmate pays half.
    row({
      categoryId: 'miete',
      categoryName: 'Miete',
      categoryType: 'Fixkosten',
      incomeCents: 480000,
      expenseCents: 960000,
      netCents: 480000,
      shareOfTotal: 0.19,
      averagePerMonthCents: 56667,
      bookingCount: 18,
    }),
    row({
      categoryId: 'lebensmittel',
      categoryName: 'Lebensmittel',
      categoryType: 'Variable Kosten',
      expenseCents: 82000,
      netCents: 82000,
      shareOfTotal: 0.03,
      averagePerMonthCents: 9110,
      bookingCount: 60,
    }),
    // A credit: the category brought in more than it cost. shareOfTotal is
    // deliberately 0 here, which must not read as a missing number.
    row({
      categoryId: 'gehalt',
      categoryName: 'Gehalt',
      categoryType: 'Einkommen',
      incomeCents: 2200000,
      netCents: -2200000,
      netIsNegative: true,
      shareOfTotal: 0,
      averagePerMonthCents: -304602,
      bookingCount: 9,
    }),
  ],
  totalNetCents: 900000,
  monthsWithData: 9,
  uncategorizedCount: 0,
  excludedTransferCount: 4,
};

function renderPage(locale: 'de' | 'en' = 'de') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale={locale}>
        <MemoryRouter initialEntries={['/auswertung?jahr=2026']}>
          <AnalysisPage />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

afterEach(cleanup);

/** Enough of a comparison for the tab to mount; its own suite tests the figures. */
const COMPARISON = {
  year: 2026,
  previousYear: 2025,
  current: { year: 2026, monthsWithData: 9, lastMonthWithData: 9 },
  previous: { year: 2025, monthsWithData: 12, lastMonthWithData: 12 },
  comparableMonths: [1, 2, 3, 4, 5, 6, 7, 8, 9],
  fullyComparable: false,
  rows: [],
  byType: [],
  previousYearHasData: true,
};

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) => {
    const p = String(path);
    if (p.startsWith('/analysis/compare')) return Promise.resolve(COMPARISON as never);
    if (p.startsWith('/analysis/trailing')) return Promise.resolve({ months: [] } as never);
    return Promise.resolve(ANALYSIS as never);
  });
});

function tableRows(container: HTMLElement) {
  return [...container.querySelectorAll('.screen-table tbody tr')] as HTMLElement[];
}

describe('the category analysis', () => {
  /**
   * The single most misreadable cell in the app. A negative net is money the
   * category brought IN, and rendering it as a bare minus invites reading it as a
   * negative cost.
   */
  /**
   * Money that came IN must not be printed with a minus. The stored convention is
   * expenses minus income, so Gehalt is −22.000,00 in the database and would read
   * as a loss in a column next to Miete — the one row that is unambiguously a
   * gain. The display flips; the arithmetic does not.
   */
  it('shows a category that earned money as a gain, not as a negative cost', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Gehalt');

    const gehalt = tableRows(container).find((r) => r.textContent?.includes('Gehalt'))!;
    expect(within(gehalt).getAllByText('Gutschrift').length).toBeGreaterThan(0);

    const net = within(gehalt).getByText(/^\+22\.000,00/);
    expect(net.className).toContain('money--credit');
    expect(net.className).toContain('money--income');
    expect(gehalt.textContent).not.toContain('-22.000,00');
  });

  /** ...and the other direction keeps its minus: that money left the account. */
  it('shows a cost as money out', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Miete');

    const miete = tableRows(container).find((r) => r.textContent?.includes('Miete'))!;
    const net = within(miete).getByText(/^-4\.800,00/);
    expect(net.className).toContain('money--expense');
  });

  /**
   * The chart and the table answer the same question at different resolutions, so
   * the table is the natural way to aim the chart — clicking a row beats hunting
   * the same name in a dropdown of thirty.
   */
  it('charts the category that was clicked in the table', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findAllByText('Lebensmittel');

    api.mockClear();
    await user.click(screen.getAllByTitle('Diese Kategorie im Diagramm zeigen')[1]);

    const calls = api.mock.calls.map(([p]) => String(p));
    expect(calls.some((p) => p.includes('/analysis/series?') && p.includes('categoryId=lebensmittel')))
      .toBe(true);
  });

  it('explains the zero share of a credit instead of printing 0,00 %', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Gehalt');

    const gehalt = tableRows(container).find((r) => r.textContent?.includes('Gehalt'))!;
    const share = within(gehalt).getByTitle(/Gutschriften haben keinen Anteil/);
    expect(share.textContent).toBe('—');
    expect(gehalt.textContent).not.toContain('0,00 %');
    // And the reason is on the page, not only in a tooltip.
    expect(screen.getByText(/Gutschriften haben keinen Anteil an den Kosten/)).toBeInTheDocument();
  });

  it('flags a category that carries income inside an expense category', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Miete');

    const miete = tableRows(container).find((r) => r.textContent?.includes('Miete'))!;
    expect(within(miete).getAllByText('enthält Erstattungen').length).toBeGreaterThan(0);
    // Both gross legs stay visible beside the net.
    expect(miete.textContent).toContain('9.600,00');
    expect(miete.textContent).toContain('4.800,00');
  });

  it('says how many transfers it left out', async () => {
    renderPage();
    expect(
      await screen.findByText('4 Umbuchungen sind in dieser Auswertung nicht enthalten.'),
    ).toBeInTheDocument();
  });

  it('sorts by a chosen column and reverses on a second press', async () => {
    const user = userEvent.setup();
    const { container } = renderPage();
    await screen.findAllByText('Miete');

    // Default is net, largest first.
    expect(tableRows(container)[0].textContent).toContain('Miete');

    await user.click(screen.getByRole('button', { name: /Buchungen/ }));
    expect(tableRows(container)[0].textContent).toContain('Lebensmittel');

    await user.click(screen.getByRole('button', { name: /Buchungen/ }));
    expect(tableRows(container)[0].textContent).toContain('Gehalt');
  });

  it('keeps category and type names German in the English interface', async () => {
    renderPage('en');
    await screen.findAllByText('Miete');
    for (const label of screen.getAllByText('Fixkosten')) {
      expect(label.getAttribute('lang')).toBe('de');
    }
  });

  /**
   * The comparison used to be a screen of its own, which is exactly the split the
   * household's identical pair never had. Both halves are reachable from here
   * now, and the year survives the switch — going from the breakdown of a year to
   * the comparison of that same year must not quietly send you back to today.
   */
  it('reaches the comparison from the same screen, keeping the year', async () => {
    const user = userEvent.setup();
    renderPage();
    await screen.findAllByText('Miete');

    await user.click(screen.getByRole('tab', { name: 'Jahresvergleich' }));

    await waitFor(() =>
      expect(api.mock.calls.map(([path]) => String(path))).toContain(
        '/analysis/compare?year=2026',
      ),
    );
    expect(screen.getByRole('tab', { name: 'Jahresvergleich' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
  });

  it('opens on the category breakdown when no tab is named', async () => {
    renderPage();
    await screen.findAllByText('Miete');
    expect(screen.getByRole('tab', { name: 'Nach Kategorie' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    expect(api.mock.calls.map(([p]) => String(p))).not.toContain('/analysis/compare?year=2026');
  });
});

describe('the address the comparison used to live at', () => {
  /**
   * A URL that worked an hour ago should not 404 because the screens were tidied,
   * and the year in it is the whole point of having bookmarked it.
   */
  it('still lands on the comparison, with its query string intact', () => {
    render(
      <MemoryRouter initialEntries={['/vergleich?jahr=2024']}>
        <Routes>
          <Route path="/vergleich" element={<CompareRedirect />} />
          <Route path="/auswertung" element={<Landed />} />
        </Routes>
      </MemoryRouter>,
    );
    expect(screen.getByTestId('landed').textContent).toBe('?jahr=2024&ansicht=vergleich');
  });
});

function Landed() {
  const { search } = useLocation();
  return <span data-testid="landed">{search}</span>;
}
