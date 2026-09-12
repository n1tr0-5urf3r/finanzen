import { Navigate, Outlet, useLocation } from 'react-router-dom';

import { useAuth } from '../lib/auth';
import { useT } from '../lib/i18n';
import { LoadingState } from './ui';

export function ProtectedRoute() {
  const auth = useAuth();
  const location = useLocation();
  const t = useT();

  if (auth.loading) {
    return (
      <div className="centered-page">
        <LoadingState label={t('common.sessionChecking')} />
      </div>
    );
  }
  if (auth.setupRequired) return <Navigate to="/einrichten" replace />;
  if (!auth.user) {
    // Remembered so the user lands where they were headed, not on the dashboard.
    return <Navigate to="/anmelden" replace state={{ from: location.pathname }} />;
  }
  return <Outlet />;
}
