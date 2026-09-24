import { Navigate, useLocation } from 'react-router-dom';

/**
 * An address that used to be a screen of its own and is a tab of the analysis
 * now. It may be bookmarked or open in a tab, so it keeps working rather than
 * becoming a 404 — the tidying is not worth a dead link.
 *
 * The query string travels with it, so `/monate?jahr=2024` still lands on 2024
 * rather than on this year.
 */
export function AnalysisRedirect({ view }: { view: string }) {
  const { search } = useLocation();
  const params = new URLSearchParams(search);
  params.set('ansicht', view);
  return <Navigate to={`/auswertung?${params.toString()}`} replace />;
}

/** `/vergleich`, which was a screen of its own for a few hours. */
export function CompareRedirect() {
  return <AnalysisRedirect view="vergleich" />;
}
