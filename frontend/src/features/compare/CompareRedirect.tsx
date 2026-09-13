import { Navigate, useLocation } from 'react-router-dom';

/**
 * `/vergleich` was a screen of its own for a few hours and may be bookmarked or
 * open in a tab. It is a sub-view of `/auswertung` now, so the old address keeps
 * working rather than becoming a 404 — the tidying is not worth a dead link.
 *
 * The query string travels with it, so `/vergleich?jahr=2024` still lands on 2024
 * rather than on this year.
 */
export function CompareRedirect() {
  const { search } = useLocation();
  const params = new URLSearchParams(search);
  params.set('ansicht', 'vergleich');
  return <Navigate to={`/auswertung?${params.toString()}`} replace />;
}
