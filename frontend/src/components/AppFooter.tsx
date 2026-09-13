import { useT } from '../lib/i18n';
import { APP_VERSION, GITHUB_URL } from '../version';

/**
 * The GitHub mark, drawn here rather than imported.
 *
 * lucide 1.0 removed every brand icon — not just this one — because a brand mark
 * is someone else's trademark on someone else's licence terms, which is not a
 * thing a general icon set can keep promising. There is no replacement inside the
 * package, and a generic `ExternalLink` would say "a link" where the point is
 * saying "GitHub", so the mark is inlined. It is the one icon in the app that
 * names a product, and the only one that will not move again.
 */
function GitHubMark() {
  return (
    <svg viewBox="0 0 24 24" width="15" height="15" fill="currentColor" aria-hidden="true">
      <path d="M12 .7a11.5 11.5 0 0 0-3.6 22.4c.6.1.8-.2.8-.5v-2.2c-3.3.7-4-1.4-4-1.4-.5-1.4-1.3-1.8-1.3-1.8-1.1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1.1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.8-1.6-2.7-.3-5.5-1.3-5.5-5.7 0-1.3.5-2.3 1.2-3.1-.1-.3-.5-1.6.1-3.1 0 0 1-.3 3.2 1.2a11 11 0 0 1 5.8 0c2.2-1.5 3.2-1.2 3.2-1.2.6 1.5.2 2.8.1 3.1.8.8 1.2 1.8 1.2 3.1 0 4.4-2.8 5.4-5.5 5.7.4.4.8 1.1.8 2.2v3.3c0 .3.2.6.8.5A11.5 11.5 0 0 0 12 .7Z" />
    </svg>
  );
}

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
        <GitHubMark />
        {t('footer.github')}
      </a>
    </footer>
  );
}
