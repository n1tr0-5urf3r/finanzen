import { useCallback, useMemo, useReducer } from 'react';

import type { BookingKind, CommentSummary, Rule } from '../../lib/types';

/**
 * Amount entry is cents-first: keystrokes append to an integer cent buffer, so
 * `1` is 0,01 and `1250` is 12,50.
 *
 * There is no decimal key, which removes the `,` versus `.` ambiguity entirely and
 * makes it impossible to type `12.5` meaning twelve fifty. This is the ATM and
 * split-the-bill convention, and it is the fastest known one-handed entry for
 * money.
 */
export interface QuickState {
  cents: number;
  kind: BookingKind;
  comment: string;
  year: number;
  month: number;
}

export type Action =
  | { type: 'digit'; value: number }
  | { type: 'doubleZero' }
  | { type: 'backspace' }
  | { type: 'clearAmount' }
  | { type: 'setComment'; comment: string }
  | { type: 'cycleKind' }
  | { type: 'setKind'; kind: BookingKind }
  | { type: 'setPeriod'; year: number; month: number }
  | { type: 'reset' };

const MAX_CENTS = 99_999_999; // 999.999,99 — well beyond any personal booking

function reducer(state: QuickState, action: Action): QuickState {
  switch (action.type) {
    case 'digit': {
      const next = state.cents * 10 + action.value;
      return next > MAX_CENTS ? state : { ...state, cents: next };
    }
    case 'doubleZero': {
      const next = state.cents * 100;
      return next > MAX_CENTS ? state : { ...state, cents: next };
    }
    case 'backspace':
      return { ...state, cents: Math.floor(state.cents / 10) };
    case 'clearAmount':
      return { ...state, cents: 0 };
    case 'setComment':
      return { ...state, comment: action.comment };
    case 'cycleKind': {
      const order: BookingKind[] = ['expense', 'income', 'transfer'];
      const index = order.indexOf(state.kind);
      return { ...state, kind: order[(index + 1) % order.length] };
    }
    case 'setKind':
      return { ...state, kind: action.kind };
    case 'setPeriod':
      return { ...state, year: action.year, month: action.month };
    case 'reset':
      // Kind and period persist: entries usually come in runs.
      return { ...state, cents: 0, comment: '' };
  }
}

export function useQuickAdd(initial?: Partial<QuickState>) {
  const now = new Date();
  const [state, dispatch] = useReducer(reducer, {
    cents: 0,
    kind: 'expense',
    comment: '',
    year: now.getFullYear(),
    month: now.getMonth() + 1,
    ...initial,
  });
  return { state, dispatch };
}

export type Prediction =
  | { status: 'idle' }
  | { status: 'rule'; categoryId: string; categoryName: string }
  | { status: 'guess'; categoryId: string; categoryName: string }
  | { status: 'unmatched' };

/**
 * Category prediction runs in the browser against the cached rule table.
 *
 * ~192 rules are a few kilobytes, so this is instant and works with no connection
 * — which matters, because the whole point of the screen is that it never makes
 * you wait. The server still decides: the response to POST /bookings carries the
 * authoritative category, and the confirm bar only ever shows what is *about* to
 * happen.
 */
export function usePrediction(comment: string, rules: Rule[] | undefined): Prediction {
  return useMemo(() => {
    const needle = comment.trim().toLocaleLowerCase('de');
    if (!needle || !rules) return { status: 'idle' };

    const exact = rules.find((r) => r.normalizedComment === needle);
    if (exact?.categoryId && exact.categoryName) {
      return { status: 'rule', categoryId: exact.categoryId, categoryName: exact.categoryName };
    }

    // A single prefix match is a usable guess; two or more is a coin flip, so it
    // is treated as unmatched rather than picking one.
    const prefixed = rules.filter((r) => r.normalizedComment.startsWith(needle) && r.categoryId);
    if (prefixed.length === 1 && prefixed[0].categoryId && prefixed[0].categoryName) {
      return {
        status: 'guess',
        categoryId: prefixed[0].categoryId,
        categoryName: prefixed[0].categoryName,
      };
    }
    return { status: 'unmatched' };
  }, [comment, rules]);
}

/**
 * Suggestion tiles, ranked by frequency so `tanken` and `essen` stay permanently
 * visible while a once-a-year comment does not.
 *
 * A new account has no history, and a grid that is empty on first use makes the
 * screen look broken — so the rule table stands in until real bookings exist.
 * Its keys are the comments this user has told the app about, which is the best
 * available guess at what they are about to type.
 */
export function useSuggestions(
  comments: CommentSummary[] | undefined,
  rules: Rule[] | undefined,
  limit = 5,
): CommentSummary[] {
  return useMemo(() => {
    const used = [...(comments ?? [])].sort((a, b) => b.count - a.count).slice(0, limit);
    if (used.length >= limit && used.length > 0) return used;

    const seen = new Set(used.map((c) => c.comment.toLocaleLowerCase('de')));
    const fallback = (rules ?? [])
      .filter((r) => r.categoryId && !seen.has(r.normalizedComment))
      // A rule that has already matched something is a better guess than one
      // that never has.
      .sort((a, b) => b.matchCount - a.matchCount)
      .slice(0, limit - used.length)
      .map<CommentSummary>((r) => ({
        comment: r.comment,
        count: r.matchCount,
        categoryName: r.categoryName,
        isUncategorized: false,
      }));
    return [...used, ...fallback];
  }, [comments, rules, limit]);
}

/** Routes hardware-keyboard input into the same reducer the pad uses. */
/**
 * A hardware keyboard drives the on-screen keypad — but ONLY when nothing else is
 * being typed into.
 *
 * This listens on `window`, so without the guard below it swallows every digit and
 * every Backspace in the app, `preventDefault()` included. Open the category
 * search, type to filter, and the digits silently edit the AMOUNT while Backspace
 * deletes it a digit at a time instead of deleting what you typed — and once the
 * amount reaches zero the save button greys out with no explanation, which is what
 * "I can't save it even though I set a category" actually was.
 */
export function useKeyboardBridge(dispatch: (action: Action) => void) {
  return useCallback(
    (event: KeyboardEvent) => {
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      // Anything with its own text cursor owns the keystroke. `closest` as well
      // as `isContentEditable`, because the latter is true for a child of an
      // editable host in a browser and absent altogether in jsdom — a guard that
      // cannot be tested is a guard that quietly stops guarding.
      const target = event.target as HTMLElement | null;
      if (
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement ||
        target?.isContentEditable ||
        target?.closest?.('[contenteditable=""], [contenteditable="true"]')
      ) {
        return;
      }
      if (event.key >= '0' && event.key <= '9') {
        dispatch({ type: 'digit', value: Number(event.key) });
        event.preventDefault();
      } else if (event.key === 'Backspace') {
        dispatch({ type: 'backspace' });
        event.preventDefault();
      }
    },
    [dispatch],
  );
}
