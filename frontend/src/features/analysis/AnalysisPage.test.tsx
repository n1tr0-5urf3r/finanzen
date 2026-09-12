import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { CategoryAnalysis, CategoryAnalysisRow } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { AnalysisPage } = await import('./AnalysisPage');

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

beforeEach(() => {
  api.mockReset();
  api.mockResolvedValue(ANALYSIS);
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
  it('marks a negative net as a credit and says so', async () => {
    const { container } = renderPage();
    await screen.findAllByText('Gehalt');

    const gehalt = tableRows(container).find((r) => r.textContent?.includes('Gehalt'))!;
    expect(within(gehalt).getAllByText('Gutschrift').length).toBeGreaterThan(0);
    const net = within(gehalt).getByText(/-22\.000,00/);
    expect(net.className).toContain('money--credit');
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
});
