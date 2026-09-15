import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '../../lib/i18n';
import type { CategoryAnalysis, CategoryAnalysisRow, CategoryTypeSummary } from '../../lib/types';

const api = vi.fn();
vi.mock('../../lib/api', async () => {
  const actual = await vi.importActual<typeof import('../../lib/api')>('../../lib/api');
  return { ...actual, api: (...args: unknown[]) => api(...args) };
});

const { FlowPeriodPicker, FlowTab } = await import('./FlowTab');

function row(over: Partial<CategoryAnalysisRow>): CategoryAnalysisRow {
  return {
    categoryId: over.categoryName?.toLowerCase() ?? 'x',
    categoryName: 'Sonstiges',
    categoryType: 'Sonstiges',
    incomeCents: 0,
    expenseCents: 0,
    netCents: 0,
    netIsNegative: false,
    shareOfTotal: 0,
    averagePerMonthCents: 0,
    bookingCount: 1,
    monthlyNetCents: Array.from({ length: 12 }, () => 0),
    ...over,
  };
}

const TYPES: CategoryTypeSummary[] = [
  { typeCode: 'einkommen', label: 'Einkommen', netCents: 0, bookingCount: 0 },
  { typeCode: 'fixkosten', label: 'Fixkosten', netCents: 0, bookingCount: 0 },
  { typeCode: 'variabel', label: 'Variable Kosten', netCents: 0, bookingCount: 0 },
];

const ANALYSIS: CategoryAnalysis = {
  year: 2026,
  rows: [
    row({
      categoryName: 'Gehalt',
      categoryType: 'Einkommen',
      incomeCents: 300000,
      netCents: -300000,
      netIsNegative: true,
      monthlyNetCents: [-100000, -200000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    }),
    row({
      categoryName: 'Miete',
      categoryType: 'Fixkosten',
      expenseCents: 110000,
      netCents: 110000,
      monthlyNetCents: [55000, 55000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    }),
    row({
      categoryName: 'Strom',
      categoryType: 'Fixkosten',
      expenseCents: 20000,
      netCents: 20000,
      monthlyNetCents: [10000, 10000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    }),
    row({
      categoryName: 'Lebensmittel',
      categoryType: 'Variable Kosten',
      expenseCents: 40000,
      netCents: 40000,
      monthlyNetCents: [20000, 20000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    }),
    // A reimbursement: this one belongs on the INFLOW side in February.
    row({
      categoryName: 'Dienstreisen',
      categoryType: 'Variable Kosten',
      incomeCents: 30000,
      netCents: -30000,
      netIsNegative: true,
      monthlyNetCents: [0, -30000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    }),
  ],
  totalNetCents: 150000,
  monthsWithData: 2,
  uncategorizedCount: 0,
  excludedTransferCount: 4,
};

function renderTab() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale="de">
        <MemoryRouter initialEntries={['/auswertung?jahr=2026&ansicht=fluss']}>
          {/* The period picker lives in the page's filter bar, beside the year;
              both halves read the same URL parameter, so the test mounts both. */}
          <FlowPeriodPicker year={2026} />
          <FlowTab year={2026} />
        </MemoryRouter>
      </I18nProvider>
    </QueryClientProvider>,
  );
}

afterEach(cleanup);

beforeEach(() => {
  api.mockReset();
  api.mockImplementation((path: string) =>
    Promise.resolve((String(path).startsWith('/category-types') ? TYPES : ANALYSIS) as never),
  );
});

describe('the money flow', () => {
  it('draws one band per flow and makes both sides add up', async () => {
    const { container } = renderTab();
    await screen.findByText(/Geldfluss 2026/);

    // Left: the income. Right: the two cost types and what was left over.
    const table = container.querySelector('.chart__data table') as HTMLElement;
    const rows = [...table.querySelectorAll('tbody tr')].map((r) => r.textContent ?? '');
    expect(rows.some((r) => r.includes('Gehalt → Einnahmen'))).toBe(true);
    expect(rows.some((r) => r.includes('Einnahmen → Fixkosten'))).toBe(true);
    expect(rows.some((r) => r.includes('Einnahmen → Übrig'))).toBe(true);

    // 3.300 in, 1.700 out, 1.600 left — and the KPIs say exactly that.
    expect(within(container).getByText('3.300,00 €')).toBeTruthy();
    expect(within(container).getByText('1.700,00 €')).toBeTruthy();
    expect(within(container).getByText('+1.600,00 €')).toBeTruthy();
  });

  /**
   * The netting rule's sharpest edge: a month where a reimbursement lands makes a
   * cost category negative. Drawing that as a bar pointing the wrong way would be
   * unreadable; it is money that came in, so it is drawn as money coming in.
   */
  it('puts a reimbursed category on the inflow side for that month', async () => {
    const user = userEvent.setup();
    const { container } = renderTab();
    await screen.findByText(/Geldfluss 2026/);

    await user.selectOptions(screen.getByLabelText('Zeitraum'), '2');
    await screen.findByText(/Geldfluss Februar 2026/);

    const rows = [...container.querySelectorAll('.chart__data tbody tr')].map(
      (r) => r.textContent ?? '',
    );
    expect(rows.some((r) => r.includes('Dienstreisen → Einnahmen'))).toBe(true);
    // ...and it does not quietly reduce its type on the other side: Variable
    // Kosten is what Lebensmittel cost, 200,00 €, with the refund left where it
    // belongs. Netting happens inside a category, never across two of them.
    expect(rows.find((r) => r.includes('Einnahmen → Variable Kosten'))).toContain('200,00');
    expect(screen.getByText(/mehr eingebracht als gekostet/)).toBeTruthy();
  });

  it('breaks a type into its categories when its node is activated', async () => {
    const user = userEvent.setup();
    const { container } = renderTab();
    await screen.findByText(/Geldfluss 2026/);

    await user.click(screen.getByLabelText('Fixkosten: Kategorien anzeigen'));

    await waitFor(() => {
      const rows = [...container.querySelectorAll('.chart__data tbody tr')].map(
        (r) => r.textContent ?? '',
      );
      expect(rows.some((r) => r.includes('Einnahmen → Miete'))).toBe(true);
      expect(rows.some((r) => r.includes('Einnahmen → Strom'))).toBe(true);
      expect(rows.some((r) => r.includes('Einnahmen → Fixkosten'))).toBe(false);
      // the other type is untouched
      expect(rows.some((r) => r.includes('Einnahmen → Variable Kosten'))).toBe(true);
    });
  });

  it('offers only the months that actually have bookings', async () => {
    renderTab();
    await screen.findByText(/Geldfluss 2026/);

    const options = [...(screen.getByLabelText('Zeitraum') as HTMLSelectElement).options].map(
      (o) => o.textContent,
    );
    expect(options).toEqual(['Ganzes Jahr 2026', 'Januar', 'Februar']);
  });

  it('swaps the whole outflow column for its categories on request', async () => {
    const user = userEvent.setup();
    const { container } = renderTab();
    await screen.findByText(/Geldfluss 2026/);

    await user.click(screen.getByRole('button', { name: 'Nach Kategorie' }));

    await waitFor(() => {
      const rows = [...container.querySelectorAll('.chart__data tbody tr')].map(
        (r) => r.textContent ?? '',
      );
      // Every type is broken out at once, and the figures are the same ones.
      expect(rows.some((r) => r.includes('Einnahmen → Miete'))).toBe(true);
      expect(rows.some((r) => r.includes('Einnahmen → Lebensmittel'))).toBe(true);
      expect(rows.some((r) => r.includes('Einnahmen → Fixkosten'))).toBe(false);
      expect(rows.find((r) => r.includes('Einnahmen → Übrig'))).toContain('1.600,00');
    });
  });
});
