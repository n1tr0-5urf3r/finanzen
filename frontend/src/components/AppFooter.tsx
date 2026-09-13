import { Github } from 'lucide-react';

import { useT } from '../lib/i18n';
import { APP_VERSION, GITHUB_URL } from '../version';

/**
 * Sits under the content on every screen inside the shell — not in the bottom bar,
 * which is for navigation, and not on a page of its own, which nobody would open.
 */
export function AppFooter() {
  const t = useT();
  return (
    <footer className="app-footer">
      <p>
        {t('app.name')} v{APP_VERSION} · © 2026 Fabian Ihle
      </p>
      <p>{t('footer.builtWith')}</p>
      <a href={GITHUB_URL} target="_blank" rel="noreferrer" className="app-footer__link">
        <Github size={15} aria-hidden="true" />
        {t('footer.github')}
      </a>
    </footer>
  );
}
