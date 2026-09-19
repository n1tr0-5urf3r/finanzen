import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useSearchParams } from 'react-router-dom';
import { Download, ExternalLink, FileJson, RefreshCw } from 'lucide-react';

import { DataLabel } from '../../components/DataLabel';
import { FlowMoney, Money } from '../../components/Money';
import {
  Banner,
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  StatusPill,
} from '../../components/ui';
import { api, downloadFile, jsonBody, asList } from '../../lib/api';
import { useAuth } from '../../lib/auth';
import { formatDateTime, formatEuro, parseEuroInput } from '../../lib/format';
import { useI18n, useT, type Locale } from '../../lib/i18n';
import { invalidateAfterYearChange, qk } from '../../lib/queryKeys';
import { useTheme, type ThemePreference } from '../../lib/theme';
import type { MessageKey } from '../../lib/messages/de';
import type { KoStatus, KoSyncResult, User, Year } from '../../lib/types';
import { useMaskAmount, useMaskedFieldClass } from '../../lib/privacy';

type Tab = 'jahre' | 'vorlagen' | 'kitchenowl' | 'export' | 'darstellung' | 'konto' | 'benutzer';

const TABS: [Tab, MessageKey][] = [
  ['jahre', 'settings.tabYears'],
  ['vorlagen', 'settings.tabRecurring'],
  ['kitchenowl', 'settings.tabKitchenOwl'],
  ['export', 'settings.tabExport'],
  ['darstellung', 'settings.tabAppearance'],
  ['konto', 'settings.tabAccount'],
  ['benutzer', 'settings.tabUsers'],
];

export function SettingsPage() {
  const t = useT();
  const [params, setParams] = useSearchParams();
  const raw = params.get('ansicht');
  const tab: Tab = (TABS.find(([value]) => value === raw)?.[0] ?? 'jahre') as Tab;

  return (
    <>
      <PageHeader title={t('settings.title')} subtitle={t('settings.intro')} />

      <div className="segmented tabs" role="tablist">
        {TABS.map(([value, labelKey]) => (
          <button
            key={value}
            type="button"
            role="tab"
            aria-pressed={tab === value}
            aria-selected={tab === value}
            onClick={() =>
              setParams(
                (prev) => {
                  const next = new URLSearchParams(prev);
                  if (value === 'jahre') next.delete('ansicht');
                  else next.set('ansicht', value);
                  return next;
                },
                { replace: true },
              )
            }
          >
            {t(labelKey)}
          </button>
        ))}
      </div>

      {tab === 'jahre' && <YearsSection />}
      {tab === 'vorlagen' && <RecurringSection />}
      {tab === 'kitchenowl' && <KitchenOwlSection />}
      {tab === 'export' && <ExportSection />}
      {tab === 'darstellung' && <AppearanceSection />}
      {tab === 'konto' && <AccountSection />}
      {tab === 'benutzer' && <UsersSection />}
    </>
  );
}

/**
 * Years and the opening balance.
 *
 * The carry-over is a **configured** value, not a derived one: the legacy sheet's
 * own month markers can add up to more than its rows do, and the project's
 * standing decision is to import the rows unchanged rather than invent correction
 * bookings. So a gap here is an explanation, not an error state.
 */
function YearsSection() {
  const t = useT();
  const maskAmount = useMaskAmount();
  const maskedField = useMaskedFieldClass();
  const client = useQueryClient();
  const [editing, setEditing] = useState<number | null>(null);
  const [amount, setAmount] = useState('');
  const [locked, setLocked] = useState(false);
  const [newYear, setNewYear] = useState(String(new Date().getFullYear()));
  const [notice, setNotice] = useState<string | null>(null);

  const years = useQuery({ queryKey: qk.years(), queryFn: () => api<Year[]>('/years') });

  const save = useMutation({
    mutationFn: (body: { year: number; openingBalanceCents: number; locked: boolean }) =>
      api<Year>(`/years/${body.year}`, { method: 'PUT', ...jsonBody(body) }),
    onSuccess: (year) => {
      invalidateAfterYearChange(client, year.year);
      setEditing(null);
      setNotice(t('settings.yearSaved', { year: year.year }));
    },
  });

  const create = useMutation({
    mutationFn: (body: { year: number; openingBalanceCents: number }) =>
      api<Year>('/years', { method: 'POST', ...jsonBody(body) }),
    onSuccess: (year) => {
      invalidateAfterYearChange(client, year.year);
      setNotice(t('settings.yearSaved', { year: year.year }));
    },
  });

  if (years.isLoading) return <LoadingState />;
  if (years.isError) return <ErrorState error={years.error} retry={() => years.refetch()} />;
  const rows = asList<Year>(years.data);

  return (
    <section className="settings-section">
      <h2>{t('settings.years')}</h2>
      <p>{t('settings.yearsIntro')}</p>

      {notice && <Banner tone="info">{notice}</Banner>}
      {save.isError && <ErrorState error={save.error} />}
      {create.isError && <ErrorState error={create.error} />}

      {rows
        .filter((y) => y.carryoverGapCents != null)
        .map((y) => (
          <Banner key={y.year} tone="info">
            {y.year}: {t('settings.carryoverGapHint', {
              amount: maskAmount(formatEuro(y.carryoverGapCents as number, { showSign: true })),
            })}
          </Banner>
        ))}

      {rows.length === 0 && <EmptyState />}

      {rows.length > 0 && (
        <div className="panel table-wrap" style={{ marginBottom: '1rem' }}>
          <table className="data-table">
            <thead>
              <tr>
                <th>{t('common.year')}</th>
                <th className="num">{t('settings.openingBalance')}</th>
                <th>{t('settings.openingSource')}</th>
                <th className="num">{t('bookings.income')}</th>
                <th className="num">{t('bookings.expense')}</th>
                <th className="num">{t('months.balance')}</th>
                <th className="num">{t('dashboard.closingBalance')}</th>
                <th className="num">{t('settings.carryoverGapLabel')}</th>
                <th>{t('settings.locked')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rows.map((y) => (
                <tr key={y.year}>
                  <th scope="row">{y.year}</th>
                  <td className="num">
                    <Money cents={y.openingBalanceCents} />
                  </td>
                  <td>
                    <StatusPill tone={y.openingSource === 'configured' ? 'info' : 'neutral'}>
                      {t(
                        y.openingSource === 'configured'
                          ? 'settings.openingConfigured'
                          : 'settings.openingDerived',
                      )}
                    </StatusPill>
                  </td>
                  <td className="num">
                    <Money cents={y.incomeCents} tone="income" />
                  </td>
                  <td className="num">
                    <Money cents={y.expenseCents} tone="expense" />
                  </td>
                  <td className="num">
                    <FlowMoney flowCents={y.balanceCents} />
                  </td>
                  <td className="num">
                    <Money cents={y.closingBalanceCents} />
                  </td>
                  <td className="num">
                    <Money cents={y.carryoverGapCents} basis="signed" />
                  </td>
                  <td>{y.locked ? <StatusPill tone="warn">{t('settings.locked')}</StatusPill> : null}</td>
                  <td>
                    <Button
                      variant="ghost"
                      onClick={() => {
                        setEditing(y.year);
                        setAmount((y.openingBalanceCents / 100).toFixed(2).replace('.', ','));
                        setLocked(y.locked);
                      }}
                    >
                      {t('common.edit')}
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {editing !== null && (
        <form
          className="panel panel--pad settings-form"
          style={{ marginBottom: '1rem' }}
          onSubmit={(e) => {
            e.preventDefault();
            const cents = parseEuroInput(amount);
            if (cents === null) return;
            save.mutate({ year: editing, openingBalanceCents: cents, locked });
          }}
        >
          <h3>{editing}</h3>
          <div className="field">
            <label htmlFor="year-opening">{t('settings.openingBalance')}</label>
            <input
              id="year-opening"
              className={`input ${maskedField}`}
              inputMode="decimal"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
            />
          </div>
          <label className="chip" style={{ cursor: 'pointer', minHeight: 'var(--tap)' }}>
            <input type="checkbox" checked={locked} onChange={(e) => setLocked(e.target.checked)} />
            {t('settings.locked')}
          </label>
          <small>{t('settings.lockedHint')}</small>
          <div style={{ display: 'flex', gap: '.5rem' }}>
            <Button type="submit" busy={save.isPending}>
              {t('common.save')}
            </Button>
            <Button type="button" variant="ghost" onClick={() => setEditing(null)}>
              {t('common.cancel')}
            </Button>
          </div>
        </form>
      )}

      <form
        className="panel panel--pad settings-form"
        onSubmit={(e) => {
          e.preventDefault();
          const year = Number(newYear);
          if (Number.isFinite(year)) create.mutate({ year, openingBalanceCents: 0 });
        }}
      >
        <h3>{t('settings.addYear')}</h3>
        <div className="field">
          <label htmlFor="new-year">{t('common.year')}</label>
          <input
            id="new-year"
            className="input"
            type="number"
            value={newYear}
            onChange={(e) => setNewYear(e.target.value)}
          />
        </div>
        <Button type="submit" busy={create.isPending}>
          {t('settings.addYear')}
        </Button>
      </form>
    </section>
  );
}

/** The templates screen already exists; duplicating it here would fork the truth. */
function RecurringSection() {
  const t = useT();
  return (
    <section className="settings-section">
      <h2>{t('recurring.title')}</h2>
      <p>{t('settings.recurringIntro')}</p>
      <Link className="button button--secondary" to="/vorlagen">
        <ExternalLink size={15} aria-hidden="true" /> {t('settings.openRecurring')}
      </Link>
    </section>
  );
}

function KitchenOwlSection() {
  const t = useT();
  const client = useQueryClient();
  const [notice, setNotice] = useState<string | null>(null);

  const status = useQuery({
    queryKey: qk.kitchenowl.status(),
    queryFn: () => api<KoStatus>('/kitchenowl/status'),
  });
  const sync = useMutation({
    mutationFn: () => api<KoSyncResult>('/kitchenowl/sync', { method: 'POST' }),
    onSuccess: (result) => {
      client.invalidateQueries({ queryKey: qk.kitchenowl.root });
      setNotice(
        result.started
          ? t('ko.synced', {
              created: result.expenses?.createdCount ?? 0,
              updated: result.expenses?.updatedCount ?? 0,
              archived: result.expenses?.archivedCount ?? 0,
            })
          : t('ko.syncBusy'),
      );
    },
  });

  if (status.isLoading) return <LoadingState />;
  if (status.isError) return <ErrorState error={status.error} retry={() => status.refetch()} />;
  const s = status.data!;

  return (
    <section className="settings-section">
      <h2>{t('ko.title')}</h2>
      <p>{t('settings.kitchenOwlIntro')}</p>

      {notice && <Banner tone="info">{notice}</Banner>}
      {sync.isError && <ErrorState error={sync.error} />}

      {!s.configured ? (
        <EmptyState title={t('ko.notConfigured')} hint={t('ko.notConfiguredHint')} />
      ) : (
        <>
          <div className="grid grid--kpi" style={{ marginBottom: '1rem' }}>
            <div className="kpi">
              <span className="kpi__label">{t('ko.household')}</span>
              <span className="kpi__value">
                {s.householdName ? <DataLabel>{s.householdName}</DataLabel> : '—'}
              </span>
            </div>
            <div className={`kpi ${s.reachable === false ? 'kpi--warn' : ''}`}>
              <span className="kpi__label">{t('settings.kitchenOwlState')}</span>
              <span className="kpi__value" style={{ fontSize: '1rem' }}>
                {t(
                  s.reachable === null
                    ? 'settings.kitchenOwlUnknown'
                    : s.reachable
                      ? 'settings.kitchenOwlReachable'
                      : 'settings.kitchenOwlUnreachable',
                )}
              </span>
            </div>
            <div className="kpi">
              <span className="kpi__label">{t('ko.countMirrored')}</span>
              <span className="kpi__value">{s.mirroredCount}</span>
            </div>
            <div className="kpi">
              <span className="kpi__label">{t('ko.countLinked')}</span>
              <span className="kpi__value">{s.linkedCount}</span>
            </div>
          </div>
          <p className="footnote" style={{ marginBottom: '.75rem' }}>
            {t('ko.lastSynced', {
              when: s.lastExpenseRun?.finishedAt
                ? formatDateTime(s.lastExpenseRun.finishedAt)
                : t('ko.never'),
            })}
            {' · '}
            {s.pollSeconds === 0
              ? t('ko.nextSyncManual')
              : t('ko.nextSync', { when: s.nextRunAt ? formatDateTime(s.nextRunAt) : '—' })}
          </p>
          <div style={{ display: 'flex', gap: '.5rem', flexWrap: 'wrap' }}>
            <Button busy={sync.isPending || s.running} onClick={() => sync.mutate()}>
              <RefreshCw size={15} aria-hidden="true" /> {t('ko.syncNow')}
            </Button>
            <Link className="button button--secondary" to="/kitchenowl">
              <ExternalLink size={15} aria-hidden="true" /> {t('settings.kitchenOwlOpen')}
            </Link>
          </div>
        </>
      )}
    </section>
  );
}

function ExportSection() {
  const t = useT();
  const [year, setYear] = useState('');
  const [notice, setNotice] = useState<string | null>(null);

  const years = useQuery({ queryKey: qk.years(), queryFn: () => api<Year[]>('/years') });
  const download = useMutation({
    mutationFn: (path: string) => downloadFile(path),
    onSuccess: (filename) => setNotice(t('export.downloaded', { filename })),
  });

  const suffix = year ? `?year=${year}` : '';

  return (
    <section className="settings-section">
      <h2>{t('export.title')}</h2>
      <p>{t('settings.exportIntro')}</p>

      {notice && <Banner tone="info">{notice}</Banner>}
      {download.isError && <ErrorState error={download.error} />}

      <div className="settings-form">
        <div className="field">
          <label htmlFor="export-year">{t('settings.exportYear')}</label>
          <select
            id="export-year"
            className="select"
            value={year}
            onChange={(e) => setYear(e.target.value)}
          >
            <option value="">{t('settings.exportAllYears')}</option>
            {asList<Year>(years.data).map((y) => (
              <option key={y.year} value={y.year}>
                {y.year}
              </option>
            ))}
          </select>
        </div>
        <div style={{ display: 'flex', gap: '.5rem', flexWrap: 'wrap' }}>
          <Button
            variant="secondary"
            busy={download.isPending}
            title={t('export.jsonHint')}
            onClick={() => download.mutate(`/exports/bookings.json${suffix}`)}
          >
            <FileJson size={15} aria-hidden="true" /> {t('export.json')}
          </Button>
          <Button
            variant="secondary"
            busy={download.isPending}
            title={t('export.csvHint')}
            onClick={() => download.mutate(`/exports/bookings.csv${suffix}`)}
          >
            <Download size={15} aria-hidden="true" /> {t('export.csv')}
          </Button>
        </div>
      </div>
    </section>
  );
}

function AppearanceSection() {
  const t = useT();
  const { locale, setLocale } = useI18n();
  const { theme, setTheme } = useTheme();

  return (
    <section className="settings-section">
      <h2>{t('settings.theme')}</h2>
      <p>{t('settings.dataLanguageNote')}</p>

      <div className="settings-form">
        <div className="field">
          <span style={{ fontSize: '.8rem', fontWeight: 600 }}>{t('settings.language')}</span>
          <div className="segmented">
            {(['de', 'en'] as Locale[]).map((value) => (
              <button
                key={value}
                type="button"
                aria-pressed={locale === value}
                onClick={() => setLocale(value)}
              >
                {value === 'de' ? 'Deutsch' : 'English'}
              </button>
            ))}
          </div>
        </div>
        <div className="field">
          <span style={{ fontSize: '.8rem', fontWeight: 600 }}>{t('settings.theme')}</span>
          <div className="segmented">
            {(
              [
                ['system', 'settings.themeSystem'],
                ['light', 'settings.themeLight'],
                ['dark', 'settings.themeDark'],
              ] as [ThemePreference, MessageKey][]
            ).map(([value, labelKey]) => (
              <button
                key={value}
                type="button"
                aria-pressed={theme === value}
                onClick={() => setTheme(value)}
              >
                {t(labelKey)}
              </button>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

function AccountSection() {
  const t = useT();
  const { user } = useAuth();
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [repeat, setRepeat] = useState('');
  const [notice, setNotice] = useState<string | null>(null);
  const [mismatch, setMismatch] = useState(false);

  const change = useMutation({
    mutationFn: (body: { currentPassword: string; newPassword: string }) =>
      api<void>('/auth/password', { method: 'PUT', ...jsonBody(body) }),
    onSuccess: () => {
      setCurrent('');
      setNext('');
      setRepeat('');
      setNotice(t('settings.passwordChanged'));
    },
  });

  return (
    <section className="settings-section">
      <h2>{user?.displayName ?? t('settings.tabAccount')}</h2>
      <p>{user?.username}</p>

      {notice && <Banner tone="info">{notice}</Banner>}
      {mismatch && <Banner tone="warn">{t('settings.passwordMismatch')}</Banner>}
      {change.isError && <ErrorState error={change.error} />}

      <form
        className="settings-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (next !== repeat) {
            setMismatch(true);
            return;
          }
          setMismatch(false);
          change.mutate({ currentPassword: current, newPassword: next });
        }}
      >
        <div className="field">
          <label htmlFor="pw-current">{t('settings.currentPassword')}</label>
          <input
            id="pw-current"
            className="input"
            type="password"
            autoComplete="current-password"
            value={current}
            onChange={(e) => setCurrent(e.target.value)}
            required
          />
        </div>
        <div className="field">
          <label htmlFor="pw-new">{t('settings.newPassword')}</label>
          <input
            id="pw-new"
            className="input"
            type="password"
            autoComplete="new-password"
            value={next}
            onChange={(e) => setNext(e.target.value)}
            required
          />
          <small>{t('auth.passwordHint')}</small>
        </div>
        <div className="field">
          <label htmlFor="pw-repeat">{t('settings.repeatPassword')}</label>
          <input
            id="pw-repeat"
            className="input"
            type="password"
            autoComplete="new-password"
            value={repeat}
            onChange={(e) => setRepeat(e.target.value)}
            required
          />
        </div>
        <Button type="submit" busy={change.isPending}>
          {t('settings.changePassword')}
        </Button>
      </form>
    </section>
  );
}

function UsersSection() {
  const t = useT();
  const client = useQueryClient();
  const { user } = useAuth();
  const [username, setUsername] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [password, setPassword] = useState('');
  const [isAdmin, setIsAdmin] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const users = useQuery({
    queryKey: qk.admin.users(),
    queryFn: () => api<User[]>('/admin/users'),
    enabled: user?.isAdmin === true,
    retry: false,
  });

  const create = useMutation({
    mutationFn: (body: {
      username: string;
      displayName: string;
      password: string;
      isAdmin: boolean;
    }) => api<User>('/admin/users', { method: 'POST', ...jsonBody(body) }),
    onSuccess: (created) => {
      client.invalidateQueries({ queryKey: qk.admin.users() });
      setUsername('');
      setDisplayName('');
      setPassword('');
      setIsAdmin(false);
      setNotice(t('settings.userCreated', { username: created.username }));
    },
  });

  if (user && !user.isAdmin) {
    return (
      <section className="settings-section">
        <h2>{t('settings.users')}</h2>
        <EmptyState hint={t('settings.usersAdminOnly')} />
      </section>
    );
  }

  return (
    <section className="settings-section">
      <h2>{t('settings.users')}</h2>
      <p>{t('settings.usersIntro')}</p>

      {notice && <Banner tone="info">{notice}</Banner>}
      {users.isError && <ErrorState error={users.error} />}
      {create.isError && <ErrorState error={create.error} />}
      {users.isLoading && <LoadingState />}

      {users.data && (
        <div className="panel table-wrap" style={{ marginBottom: '1rem' }}>
          <table className="data-table">
            <thead>
              <tr>
                <th>{t('auth.username')}</th>
                <th>{t('auth.displayName')}</th>
                <th>{t('settings.userIsAdmin')}</th>
              </tr>
            </thead>
            <tbody>
              {users.data.map((u) => (
                <tr key={u.id}>
                  <td>{u.username}</td>
                  <td>{u.displayName}</td>
                  <td>{u.isAdmin ? <StatusPill tone="info">{t('settings.userIsAdmin')}</StatusPill> : null}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <form
        className="panel panel--pad settings-form"
        onSubmit={(e) => {
          e.preventDefault();
          create.mutate({ username, displayName, password, isAdmin });
        }}
      >
        <h3>{t('settings.newUser')}</h3>
        <div className="field">
          <label htmlFor="user-name">{t('auth.username')}</label>
          <input
            id="user-name"
            className="input"
            autoComplete="off"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            required
          />
        </div>
        <div className="field">
          <label htmlFor="user-display">{t('auth.displayName')}</label>
          <input
            id="user-display"
            className="input"
            value={displayName}
            onChange={(e) => setDisplayName(e.target.value)}
            required
          />
        </div>
        <div className="field">
          <label htmlFor="user-password">{t('auth.password')}</label>
          <input
            id="user-password"
            className="input"
            type="password"
            autoComplete="new-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
          />
          <small>{t('auth.passwordHint')}</small>
        </div>
        <label className="chip" style={{ cursor: 'pointer', minHeight: 'var(--tap)' }}>
          <input type="checkbox" checked={isAdmin} onChange={(e) => setIsAdmin(e.target.checked)} />
          {t('settings.userIsAdmin')}
        </label>
        <Button type="submit" busy={create.isPending}>
          {t('settings.newUser')}
        </Button>
      </form>
    </section>
  );
}
