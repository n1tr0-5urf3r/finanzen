import { describe, expect, it } from 'vitest';

import { formatEuro, formatPercent, monthName, parseEuroInput } from './format';

describe('formatting is always de-DE', () => {
  it('renders cents as German currency', () => {
    // Intl puts U+00A0 or U+202F before the symbol depending on the ICU build,
    // so the assertions normalise it rather than depending on which one shipped.
    expect(formatEuro(2_700_000).replace(/[\u00a0\u202f]/g, ' ')).toBe('27.000,00 €');
    expect(formatEuro(900_000).replace(/[\u00a0\u202f]/g, ' ')).toBe('9.000,00 €');
    expect(formatEuro(4_900_000).replace(/[\u00a0\u202f]/g, ' ')).toBe('49.000,00 €');
    expect(formatEuro(0).replace(/[\u00a0\u202f]/g, ' ')).toBe('0,00 €');
  });

  it('shows an explicit sign when asked', () => {
    expect(formatEuro(900_000, { showSign: true })).toContain('+');
    // Intl uses U+2212 MINUS SIGN, not a hyphen.
    expect(formatEuro(-30_000, { showSign: true })).toMatch(/[-−]/);
    expect(formatEuro(-30_000).replace(/[\u00a0\u202f]/g, ' ')).toContain('300,00');
  });

  it('formats percentages the German way', () => {
    expect(formatPercent(0.25).replace(/[\u00a0\u202f]/g, ' ')).toBe('25,00 %');
    expect(formatPercent(0.5375).replace(/[\u00a0\u202f]/g, ' ')).toBe('53,75 %');
  });

  it('parses German decimal comma input', () => {
    expect(parseEuroInput('12,50')).toBe(1250);
    expect(parseEuroInput('1.234,56')).toBe(123456);
    expect(parseEuroInput('12,50 €')).toBe(1250);
    expect(parseEuroInput('0,99')).toBe(99);
    expect(parseEuroInput('')).toBeNull();
    expect(parseEuroInput('abc')).toBeNull();
  });

  it('keeps German month names', () => {
    expect(monthName(1)).toBe('Januar');
    expect(monthName(3)).toBe('März');
    expect(monthName(12)).toBe('Dezember');
  });
});
