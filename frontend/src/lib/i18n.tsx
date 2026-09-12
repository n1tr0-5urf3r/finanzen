import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react';

import { de, type MessageKey, type Messages } from './messages/de';
import { en } from './messages/en';

export type Locale = 'de' | 'en';

const CATALOGUES: Record<Locale, Messages> = { de, en };
const STORAGE_KEY = 'finanzen.locale';

type Vars = Record<string, string | number>;

interface I18nValue {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: (key: MessageKey, vars?: Vars) => string;
}

const I18nContext = createContext<I18nValue | null>(null);

function readStored(): Locale {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === 'de' || stored === 'en') return stored;
  } catch {
    /* private mode, blocked storage — fall through to the default */
  }
  return navigator.language?.startsWith('en') ? 'en' : 'de';
}

export function I18nProvider({
  children,
  initialLocale,
}: {
  children: ReactNode;
  initialLocale?: Locale;
}) {
  const [locale, setLocaleState] = useState<Locale>(() => initialLocale ?? readStored());

  const setLocale = useCallback((next: Locale) => {
    setLocaleState(next);
    document.documentElement.lang = next;
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      /* a remembered preference is a convenience, not a requirement */
    }
  }, []);

  const t = useCallback(
    (key: MessageKey, vars?: Vars) => {
      const template = CATALOGUES[locale][key] ?? CATALOGUES.de[key] ?? key;
      if (!vars) return template;
      return template.replace(/\{(\w+)\}/g, (match, name: string) =>
        name in vars ? String(vars[name]) : match,
      );
    },
    [locale],
  );

  const value = useMemo(() => ({ locale, setLocale, t }), [locale, setLocale, t]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  const value = useContext(I18nContext);
  if (!value) throw new Error('useI18n must be used inside I18nProvider');
  return value;
}

export function useT() {
  return useI18n().t;
}
