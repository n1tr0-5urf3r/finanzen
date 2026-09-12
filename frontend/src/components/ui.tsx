import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { AlertTriangle, Inbox, RefreshCw } from 'lucide-react';

import { errorMessage } from '../lib/api';
import { useT } from '../lib/i18n';

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
      {actions && <div className="page-header__actions">{actions}</div>}
    </header>
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
