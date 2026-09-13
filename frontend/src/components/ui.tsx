import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { AlertTriangle, Inbox, RefreshCw } from 'lucide-react';

import { errorMessage } from '../lib/api';
import { useT } from '../lib/i18n';
import { PrivacyToggle } from './PrivacyToggle';
import { ScopeNote } from './Money';

export function Button({
  variant = 'primary',
  busy,
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
  busy?: boolean;
}) {
  return (
    <button
      {...props}
      className={`button button--${variant} ${props.className ?? ''}`}
      disabled={busy || props.disabled}
    >
      {busy && <span className="spinner" aria-hidden="true" />}
      {children}
    </button>
  );
}

export function PageHeader({
  title,
  subtitle,
  actions,
}: {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
}) {
  return (
    <header className="page-header">
      <div>
        <h1 tabIndex={-1}>{title}</h1>
        {subtitle && <p>{subtitle}</p>}
      </div>
      {/* The privacy toggle rides in every header rather than living in Settings:
          it is wanted at the moment someone else can see the screen, and one place
          to reach it beats ten pages remembering to offer it. */}
      <div className="page-header__actions">
        {actions}
        <PrivacyToggle />
      </div>
    </header>
  );
}

/**
 * A headline figure with its label and its scope.
 *
 * `scope` is REQUIRED and has three values, because the question "are transfers
 * inside this number" has three honest answers and the dashboard's original
 * version only allowed two — which is why every KitchenOwl screen hand-rolled its
 * own tile rather than answer a question that does not apply to a ledger with no
 * transfers in it.
 *
 * - `with` / `without` — the personal ledger, where transfers exist.
 * - `none` — a ledger where the concept does not apply. Says so by saying nothing.
 */
export function Kpi({
  label,
  children,
  hint,
  scope,
  tone,
}: {
  label: string;
  children: ReactNode;
  hint?: string;
  scope: 'with' | 'without' | 'none';
  tone?: 'warn' | 'accent';
}) {
  return (
    <div className={`kpi ${tone ? `kpi--${tone}` : ''}`} title={hint}>
      <span className="kpi__label">{label}</span>
      <span className="kpi__value">{children}</span>
      {scope !== 'none' && <ScopeNote transfersIncluded={scope === 'with'} />}
    </div>
  );
}

export function LoadingState({ label }: { label?: string }) {
  const t = useT();
  return (
    <div className="state-panel" role="status">
      <span className="spinner" aria-hidden="true" />
      <p>{label ?? t('common.loading')}</p>
    </div>
  );
}

export function ErrorState({ error, retry }: { error: unknown; retry?: () => void }) {
  const t = useT();
  return (
    <div className="state-panel" role="alert">
      <AlertTriangle size={22} aria-hidden="true" />
      <h3>{t('error.title')}</h3>
      <p>{errorMessage(error)}</p>
      {retry && (
        <Button variant="secondary" onClick={retry}>
          <RefreshCw size={15} aria-hidden="true" /> {t('common.retry')}
        </Button>
      )}
    </div>
  );
}

export function EmptyState({ title, hint }: { title?: string; hint?: string }) {
  const t = useT();
  return (
    <div className="state-panel">
      <Inbox size={22} aria-hidden="true" />
      <h3>{title ?? t('empty.title')}</h3>
      {hint && <p>{hint}</p>}
    </div>
  );
}

export function StatusPill({
  tone = 'neutral',
  children,
}: {
  tone?: 'neutral' | 'good' | 'warn' | 'danger' | 'info';
  children: ReactNode;
}) {
  return <span className={`pill pill--${tone}`}>{children}</span>;
}

export function Banner({
  tone = 'info',
  children,
}: {
  tone?: 'info' | 'warn' | 'danger';
  children: ReactNode;
}) {
  return (
    <div className={`banner banner--${tone}`} role={tone === 'info' ? undefined : 'alert'}>
      <div>{children}</div>
    </div>
  );
}
