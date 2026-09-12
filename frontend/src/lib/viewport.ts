/**
 * Keeps fixed-to-the-bottom chrome pinned to what the user can actually see.
 *
 * A `position: fixed; bottom: 0` element is laid out against the LAYOUT viewport.
 * On Android Chrome the URL bar hides by growing the layout viewport while the
 * visual viewport lags behind, and during (and sometimes after) that animation the
 * browser does not re-lay-out fixed children — so the bottom bar rides up off the
 * bottom edge and stays there until the next scroll settles it. The soft keyboard
 * does the same thing in reverse.
 *
 * There is no CSS unit for this: `dvh` describes the viewport's height, not the
 * gap between the two viewports. `visualViewport` reports both, so the gap is
 * computable, and one custom property lets the CSS shift the bar back down by
 * exactly that much. When the two viewports agree — every desktop browser, and a
 * phone at rest — the gap is 0 and nothing moves.
 */
export function installViewportInset(): () => void {
  const vv = window.visualViewport;
  if (!vv) return () => {};

  const root = document.documentElement;
  let frame = 0;

  const apply = () => {
    frame = 0;
    // How far the bottom of the layout viewport sits below the bottom of what is
    // on screen. Clamped at 0: a negative value would lift the bar for no reason.
    const gap = Math.max(0, Math.round(root.clientHeight - (vv.height + vv.offsetTop)));
    root.style.setProperty('--vv-bottom', `${gap}px`);
  };

  // Chrome fires these continuously through the URL-bar animation; coalescing to
  // one write per frame keeps the bar smooth instead of stuttering behind it.
  const schedule = () => {
    if (frame === 0) frame = window.requestAnimationFrame(apply);
  };

  apply();
  vv.addEventListener('resize', schedule);
  vv.addEventListener('scroll', schedule);
  window.addEventListener('orientationchange', schedule);

  return () => {
    if (frame !== 0) window.cancelAnimationFrame(frame);
    vv.removeEventListener('resize', schedule);
    vv.removeEventListener('scroll', schedule);
    window.removeEventListener('orientationchange', schedule);
    root.style.removeProperty('--vv-bottom');
  };
}
