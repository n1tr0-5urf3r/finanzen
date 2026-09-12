/**
 * The arithmetic behind the hand-rolled charts.
 *
 * There is no chart library here on purpose: Recharts costs roughly 100 kB gzip —
 * more than the rest of this bundle — to draw twelve points per series, and every
 * chart in this app needs semantics no default provides (a baseline that diverges
 * at zero, a line that stops rather than falls to zero, credit colouring).
 */

/**
 * Tick values for a value axis, with **zero always inside the domain**.
 *
 * That is the whole point of the function. An axis running from 30.000 € to
 * 36.000 € makes a six percent difference look like a collapse, which is the
 * single commonest way a truthful number becomes a misleading picture.
 */
export function niceTicks(min: number, max: number, count = 4): number[] {
  const lo = Math.min(0, min);
  const hi = Math.max(0, max);
  if (lo === hi) return [0];

  const raw = (hi - lo) / Math.max(1, count);
  const magnitude = 10 ** Math.floor(Math.log10(raw));
  const step =
    [1, 2, 2.5, 5, 10].map((m) => m * magnitude).find((s) => s >= raw) ?? 10 * magnitude;

  const start = Math.floor(lo / step) * step;
  const end = Math.ceil(hi / step) * step;
  const ticks: number[] = [];
  // Guard against a pathological step; a chart is never worth an infinite loop.
  for (let v = start, i = 0; v <= end + step / 1000 && i < 64; v += step, i += 1) {
    ticks.push(Math.round(v));
  }
  return ticks;
}

export interface Scale {
  /** Value to pixel, inside [top, bottom]. */
  y: (value: number) => number;
  /** Where zero sits — the baseline every diverging chart hangs from. */
  zero: number;
  ticks: number[];
  domain: [number, number];
}

export function linearScale(
  values: number[],
  top: number,
  bottom: number,
  tickCount = 4,
): Scale {
  const ticks = niceTicks(Math.min(0, ...values), Math.max(0, ...values), tickCount);
  const lo = ticks[0];
  const hi = ticks[ticks.length - 1];
  const span = hi - lo || 1;
  const y = (value: number) => bottom - ((value - lo) / span) * (bottom - top);
  return { y, zero: y(0), ticks, domain: [lo, hi] };
}

/** Evenly spaced band centres, the x-axis for twelve months. */
export function bands(count: number, left: number, right: number) {
  const width = (right - left) / Math.max(1, count);
  return {
    width,
    start: (i: number) => left + i * width,
    centre: (i: number) => left + i * width + width / 2,
  };
}
