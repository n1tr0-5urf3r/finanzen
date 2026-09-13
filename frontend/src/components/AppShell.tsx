import { useEffect, useState } from 'react';
import { NavLink, Outlet, useLocation } from 'react-router-dom';
import {
  ArrowDownUp, BarChart3, CalendarRange, FileSpreadsheet,
  LayoutDashboard, MoreHorizontal, PiggyBank, Plus, Receipt, Repeat, Search,
  Settings, Tags, WalletMinimal,
} from 'lucide-react';

import { useT } from '../lib/i18n';
import type { MessageKey } from '../lib/messages/de';
import { AppFooter } from './AppFooter';

const NAV: { to: string; labelKey: MessageKey; icon: typeof LayoutDashboard }[] = [
  { to: '/dashboard', labelKey: 'nav.dashboard', icon: LayoutDashboard },
  { to: '/buchungen', labelKey: 'nav.bookings', icon: ArrowDownUp },
  { to: '/suche', labelKey: 'nav.search', icon: Search },
  { to: '/monate', labelKey: 'nav.months', icon: CalendarRange },
  { to: '/auswertung', labelKey: 'nav.analysis', icon: BarChart3 },
  { to: '/steuer', labelKey: 'nav.tax', icon: Receipt },
  { to: '/vorlagen', labelKey: 'nav.recurring', icon: Repeat },
  { to: '/ruecklagen', labelKey: 'nav.funds', icon: PiggyBank },
  { to: '/kitchenowl', labelKey: 'nav.kitchenowl', icon: WalletMinimal },
  { to: '/kategorien', labelKey: 'nav.categories', icon: Tags },
  { to: '/import', labelKey: 'nav.import', icon: FileSpreadsheet },
  { to: '/einstellungen', labelKey: 'nav.settings', icon: Settings },
];

/** The four that fit a bottom bar. The other four live behind "Mehr". */
const MOBILE = ['/dashboard', '/buchungen', '/auswertung', '/einstellungen'];

export function AppShell() {
  const t = useT();
  const [moreOpen, setMoreOpen] = useState(false);
  const location = useLocation();

  // Any route change closes the sheet, including a back gesture.
  useEffect(() => setMoreOpen(false), [location.pathname]);

  const primary = NAV.filter((n) => MOBILE.includes(n.to));
  const secondary = NAV.filter((n) => !MOBILE.includes(n.to));

  return (
    <div className="app-shell">
      <a className="skip-link" href="#main">
        {t('common.skipToContent')}
      </a>

      <nav className="sidebar" aria-label={t('app.name')}>
        <div className="brand">
          <img src="/fi.png" alt="" />
          <div>
            <strong>{t('app.name')}</strong>
            <span>{t('app.tagline')}</span>
          </div>
        </div>
        {NAV.map(({ to, labelKey, icon: Icon }) => (
          <NavLink key={to} to={to} className="nav-link">
            <Icon size={17} aria-hidden="true" />
            {t(labelKey)}
          </NavLink>
        ))}
        <div className="sidebar__spacer" />
        <NavLink to="/schnell" className="nav-link">
          <Plus size={17} aria-hidden="true" />
          {t('nav.quickAdd')}
        </NavLink>
      </nav>

      <main className="main" id="main">
        <div className="page">
          <Outlet />
          <AppFooter />
        </div>
      </main>

      {/* Four entries fit a bottom bar; there are eight screens. Without the
          "Mehr" sheet, Monate, Steuer, Kategorien and Import simply do not
          exist on a phone — there is no sidebar to fall back to.

          The sheet is INSIDE the dock rather than beside it: the dock is what
          sticks to the bottom of the scroll, so anchoring the sheet to it is what
          makes the two move as one thing instead of two that agree. */}
      {moreOpen && (
        <div className="sheet-scrim" onClick={() => setMoreOpen(false)} aria-hidden="true" />
      )}

      <div className="mobile-dock">
        {moreOpen && (
          <nav className="mobile-more" aria-label={t('nav.more')}>
            {secondary.map(({ to, labelKey, icon: Icon }) => (
              <NavLink key={to} to={to} onClick={() => setMoreOpen(false)}>
                <Icon size={18} aria-hidden="true" />
                {t(labelKey)}
              </NavLink>
            ))}
          </nav>
        )}

        <nav className="mobile-bar" aria-label={t('app.name')}>
          {primary.slice(0, 2).map(({ to, labelKey, icon: Icon }) => (
            <NavLink key={to} to={to}>
              <Icon size={19} aria-hidden="true" />
              {t(labelKey)}
            </NavLink>
          ))}
          <NavLink to="/schnell" className="mobile-bar__add" aria-label={t('nav.quickAdd')}>
            <Plus size={24} aria-hidden="true" />
          </NavLink>
          {primary.slice(2, 3).map(({ to, labelKey, icon: Icon }) => (
            <NavLink key={to} to={to}>
              <Icon size={19} aria-hidden="true" />
              {t(labelKey)}
            </NavLink>
          ))}
          <button
            type="button"
            className={`mobile-bar__more ${moreOpen ? 'is-open' : ''}`}
            aria-expanded={moreOpen}
            onClick={() => setMoreOpen((v) => !v)}
          >
            <MoreHorizontal size={19} aria-hidden="true" />
            {t('nav.more')}
          </button>
        </nav>
      </div>
    </div>
  );
}
