import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react';

const STORAGE_KEY = 'finanzen.privacy';

/**
 * What a hidden amount reads as. Not an empty string and not a layout collapse:
 * the row, the column width and the tabular alignment all have to survive, because
 * the point of this mode is that everything except the figure stays legible.
 */
export const MASKED_AMOUNT = '•••• €';

const PrivacyContext = createContext<{
  hidden: boolean;
  setHidden: (hidden: boolean) => void;
} | null>(null);

function readStored(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === 'on';
  } catch {
    /* blocked storage is not an error; the mode simply starts off */
  }
  return false;
}

/**
 * Privacy mode: every euro figure is masked while the structure of the data —
 * categories, months, counts, shares, the shape of every chart — stays exactly
 * as it was. It is for a screen someone else can see, not for a screen someone
 * else may use, so it is a display setting and nothing more: no data is withheld
 * from the client, and an export still exports real numbers.
 */
export function PrivacyProvider({
  children,
  initialHidden,
}: {
  children: ReactNode;
  initialHidden?: boolean;
}) {
  const [hidden, setHiddenState] = useState<boolean>(() => initialHidden ?? readStored());

  const setHidden = useCallback((next: boolean) => {
    setHiddenState(next);
    try {
      localStorage.setItem(STORAGE_KEY, next ? 'on' : 'off');
    } catch {
      /* a remembered preference is a convenience, not a requirement */
    }
  }, []);

  const value = useMemo(() => ({ hidden, setHidden }), [hidden, setHidden]);
  return <PrivacyContext.Provider value={value}>{children}</PrivacyContext.Provider>;
}

/**
 * Defaults to visible when no provider is above it. A missing provider must not
 * throw the way `useTheme` does: this hook sits inside `Money`, which renders in
 * places (tests, and any future island) where the app shell is not mounted, and a
 * crash there would be a worse failure than an unmasked figure in a test.
 */
export function usePrivacy() {
  return useContext(PrivacyContext) ?? { hidden: false, setHidden: () => {} };
}

/**
 * Masks an already-formatted amount. Everything that renders euros goes through
 * either this or `Money`, so there is one place the rule lives.
 */
export function useMaskAmount(): (text: string) => string {
  const { hidden } = usePrivacy();
  return useCallback((text: string) => (hidden ? MASKED_AMOUNT : text), [hidden]);
}

/**
 * For the amount INPUTS, which cannot simply be replaced without making the field
 * uneditable. They are blurred until focused, so the figure is unreadable over a
 * shoulder and still editable by the person at the keyboard.
 */
export function useMaskedFieldClass(): string {
  const { hidden } = usePrivacy();
  return hidden ? 'is-masked' : '';
}
