import { useSyncExternalStore } from 'react';

/** The one breakpoint the charts care about — the same 559px the stylesheet uses
    to switch chart text to phone size. Keep the two in step. */
const QUERY = '(max-width: 559px)';

function subscribe(onChange: () => void): () => void {
  const mql = window.matchMedia?.(QUERY);
  if (!mql) return () => {};
  mql.addEventListener('change', onChange);
  return () => mql.removeEventListener('change', onChange);
}

/**
 * Is this a phone-width screen?
 *
 * A chart drawn into a fixed 720-unit viewBox is scaled by the width it is given,
 * so the SAME geometry renders at half size on a phone: labels shrink, a 300-unit
 * diagram becomes 150 rendered pixels. The stylesheet compensates for text; only
 * the component can compensate for layout, and only if it knows. Hence a hook
 * rather than more CSS.
 *
 * `useSyncExternalStore` rather than an effect + state: it reads the right value
 * on the first render, so nothing is drawn at the wrong size and corrected a frame
 * later, and it degrades to `false` under SSR or a jsdom without matchMedia.
 */
export function useNarrow(): boolean {
  return useSyncExternalStore(
    subscribe,
    () => window.matchMedia?.(QUERY).matches ?? false,
    () => false,
  );
}
