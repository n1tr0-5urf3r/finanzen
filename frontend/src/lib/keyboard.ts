import { useEffect } from 'react';

/** The CSS variable the Quick Add sheets are positioned against. */
const VAR = '--keyboard-inset';

/**
 * Publishes the height of the on-screen keyboard as `--keyboard-inset`, for as
 * long as the calling component is mounted.
 *
 * This used to be `interactive-widget=resizes-content` in the viewport meta,
 * which asked the browser to shrink the LAYOUT viewport around the keyboard so
 * that `bottom: 0` meant "above the keyboard" for free. It bought one sheet its
 * keyboard behaviour and cost the whole app its bottom navigation: the key opts
 * Gecko out of the viewport path that holds the initial containing block still
 * while the dynamic toolbar animates, and the navigation bar is measured against
 * exactly that box (see index.html).
 *
 * So the compensation moves here, where its blast radius is one element on one
 * screen. Quick Add is a full-screen overlay with nothing scrolling behind it,
 * so the toolbar is not animating while this is listening — the only thing that
 * moves the visual viewport here is the keyboard, which is what we want to read.
 *
 * `visualViewport` is absent in jsdom and on old browsers; the sheet then simply
 * sits at the bottom, which is where it sat before any of this existed.
 */
export function useKeyboardInset() {
  useEffect(() => {
    const vv = window.visualViewport;
    if (!vv) return;

    const apply = () => {
      // What the keyboard covers: the part of the window the visual viewport no
      // longer reaches, discounting anything the browser has scrolled it by.
      const covered = window.innerHeight - vv.height - vv.offsetTop;
      // Under a few pixels it is rounding, not a keyboard, and acting on it would
      // make the sheet twitch.
      const inset = covered > 8 ? Math.round(covered) : 0;
      document.documentElement.style.setProperty(VAR, `${inset}px`);
    };

    apply();
    vv.addEventListener('resize', apply);
    vv.addEventListener('scroll', apply);
    return () => {
      vv.removeEventListener('resize', apply);
      vv.removeEventListener('scroll', apply);
      document.documentElement.style.removeProperty(VAR);
    };
  }, []);
}
