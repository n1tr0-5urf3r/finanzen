import { describe, expect, it } from 'vitest';

import { bands, linearScale, niceTicks } from './scales';

describe('chart scales', () => {
  /**
   * The misrepresentation this function exists to prevent: an axis running from
   * 30.000 € to 36.000 € turns a six percent difference into a cliff.
   */
  it('always puts zero inside the domain', () => {
    expect(niceTicks(3000000, 3600000)).toContain(0);
    expect(niceTicks(-30000, 990000)).toContain(0);
    expect(niceTicks(100, 200)[0]).toBe(0);
  });

  it('spans a diverging domain in both directions', () => {
    const ticks = niceTicks(-2200000, 900000);
    expect(ticks[0]).toBeLessThanOrEqual(-2200000);
    expect(ticks[ticks.length - 1]).toBeGreaterThanOrEqual(900000);
  });

  it('degenerates safely when everything is zero', () => {
    expect(niceTicks(0, 0)).toEqual([0]);
    const scale = linearScale([0, 0], 0, 100);
    expect(Number.isFinite(scale.zero)).toBe(true);
  });

  it('maps values onto pixels with the baseline where zero is', () => {
    const scale = linearScale([0, 1000], 0, 100);
    expect(scale.y(scale.domain[0])).toBe(100);
    expect(scale.y(scale.domain[1])).toBe(0);
    expect(scale.zero).toBe(scale.y(0));
  });

  it('spaces twelve months evenly', () => {
    const band = bands(12, 0, 720);
    expect(band.width).toBe(60);
    expect(band.centre(0)).toBe(30);
    expect(band.centre(11)).toBe(690);
  });
});
