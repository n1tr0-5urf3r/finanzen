/**
 * Formatting is ALWAYS de-DE, in both UI languages.
 *
 * The money is euros and the dates are German months; switching the interface to
 * English must not turn `1.234,56 €` into `1,234.56 €`, because the number would
 * then disagree with the user's bank, their spreadsheet and every receipt. Locale
 * here is a property of the data, not of the interface.
 */
const LOCALE = 'de-DE';

const euro = new Intl.NumberFormat(LOCALE, {
  style: 'currency',
  currency: 'EUR',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});

const euroSigned = new Intl.NumberFormat(LOCALE, {
  style: 'currency',
  currency: 'EUR',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
  signDisplay: 'exceptZero',
});

const compact = new Intl.NumberFormat(LOCALE, {
  notation: 'compact',
  maximumFractionDigits: 1,
});

const percent = new Intl.NumberFormat(LOCALE, {
  style: 'percent',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});

/** Cents to `1.234,56 €`. The only currency formatter in the application. */
export function formatEuro(cents: number, options: { showSign?: boolean } = {}): string {
  const value = cents / 100;
  return options.showSign ? euroSigned.format(value) : euro.format(value);
}

/** For chart axes, where the full figure does not fit. */
export function formatEuroCompact(cents: number): string {
  return `${compact.format(cents / 100)} €`;
}

export function formatPercent(fraction: number): string {
  return percent.format(fraction);
}

export function formatNumber(value: number, maximumFractionDigits = 0): string {
  return new Intl.NumberFormat(LOCALE, { maximumFractionDigits }).format(value);
}

export const MONTHS_DE = [
  'Januar', 'Februar', 'März', 'April', 'Mai', 'Juni',
  'Juli', 'August', 'September', 'Oktober', 'November', 'Dezember',
] as const;

export const MONTHS_DE_SHORT = [
  'Jan', 'Feb', 'Mär', 'Apr', 'Mai', 'Jun',
  'Jul', 'Aug', 'Sep', 'Okt', 'Nov', 'Dez',
] as const;

export function monthName(month: number): string {
  return MONTHS_DE[month - 1] ?? '';
}

export function monthShort(month: number): string {
  return MONTHS_DE_SHORT[month - 1] ?? '';
}

export function formatDate(iso: string | null | undefined): string {
  if (!iso) return '–';
  return new Intl.DateTimeFormat(LOCALE, { dateStyle: 'medium' }).format(new Date(iso));
}

export function formatDateTime(iso: string | null | undefined): string {
  if (!iso) return '–';
  return new Intl.DateTimeFormat(LOCALE, { dateStyle: 'medium', timeStyle: 'short' })
    .format(new Date(iso));
}

/**
 * Parses German amount input. Accepts `12,50`, `1.234,56` and a bare `1250`
 * meaning twelve fifty is NOT assumed — a bare integer is euros, as typed.
 */
export function parseEuroInput(raw: string): number | null {
  // U+00A0 and U+202F are what Intl puts before the currency symbol.
  const cleaned = raw.replace(/[\s\u00a0\u202f€]/g, '');
  if (!cleaned) return null;
  const normalized =
    cleaned.includes(',') ? cleaned.replace(/\./g, '').replace(',', '.') : cleaned;
  const value = Number(normalized);
  if (!Number.isFinite(value)) return null;
  return Math.round(value * 100);
}
