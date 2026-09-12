import { NavLink, Outlet } from 'react-router-dom';
import {
  ArrowDownUp, BarChart3, CalendarRange, FileSpreadsheet, LayoutDashboard,
  Plus, Receipt, Settings, Tags,
} from 'lucide-react';

import { useT } from '../lib/i18n';
import type { MessageKey } from '../lib/messages/de';

const NAV: { to: string; labelKey: MessageKey; icon: typeof LayoutDashboard }[] = [
  { to: '/dashboard', labelKey: 'nav.dashboard', icon: LayoutDashboard },
  { to: '/buchungen', labelKey: 'nav.bookings', icon: ArrowDownUp },
  { to: '/monate', labelKey: 'nav.months', icon: CalendarRange },
  { to: '/auswertung', labelKey: 'nav.analysis', icon: BarChart3 },
  { to: '/steuer', labelKey: 'nav.tax', icon: Receipt },
  { to: '/kategorien', labelKey: 'nav.categories', icon: Tags },
  { to: '/import', labelKey: 'nav.import', icon: FileSpreadsheet },
  { to: '/einstellungen', labelKey: 'nav.settings', icon: Settings },
];

/** The four that matter on a phone, plus the reason the app exists. */
const MOBILE = ['/dashboard', '/buchungen', '/auswertung', '/einstellungen'];

export function AppShell() {
  const t = useT();

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
        </div>
      </main>

      <nav className="mobile-bar" aria-label={t('app.name')}>
        {NAV.filter((n) => MOBILE.includes(n.to))
          .slice(0, 2)
          .map(({ to, labelKey, icon: Icon }) => (
            <NavLink key={to} to={to}>
              <Icon size={19} aria-hidden="true" />
              {t(labelKey)}
            </NavLink>
          ))}
        <NavLink to="/schnell" className="mobile-bar__add" aria-label={t('nav.quickAdd')}>
          <Plus size={24} aria-hidden="true" />
        </NavLink>
        {NAV.filter((n) => MOBILE.includes(n.to))
          .slice(2)
          .map(({ to, labelKey, icon: Icon }) => (
            <NavLink key={to} to={to}>
              <Icon size={19} aria-hidden="true" />
              {t(labelKey)}
            </NavLink>
          ))}
      </nav>
    </div>
  );
}
