import { useState, type FormEvent } from 'react';
import { Navigate, useLocation, useNavigate } from 'react-router-dom';

import { Button } from '../../components/ui';
import { errorMessage } from '../../lib/api';
import { useAuth } from '../../lib/auth';
import { useT } from '../../lib/i18n';

function Brand() {
  const t = useT();
  return (
    <div className="brand" style={{ justifyContent: 'center', padding: '0 0 1rem' }}>
      <img src="/fi.png" alt="" style={{ width: 44, height: 44 }} />
      <strong style={{ fontSize: '1.15rem' }}>{t('app.name')}</strong>
    </div>
  );
}

export function LoginPage() {
  const t = useT();
  const auth = useAuth();
  const navigate = useNavigate();
  const location = useLocation() as { state?: { from?: string } };
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);

  if (auth.setupRequired) return <Navigate to="/einrichten" replace />;
  if (auth.user) return <Navigate to={location.state?.from ?? '/dashboard'} replace />;

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await auth.login(username, password);
      navigate(location.state?.from ?? '/dashboard', { replace: true });
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="centered-page">
      <form className="panel panel--pad form-stack" style={{ width: 'min(92vw, 22rem)' }} onSubmit={submit}>
        <Brand />
        <h1 style={{ fontSize: '1.15rem' }}>{t('auth.loginTitle')}</h1>
        <div className="field">
          <label htmlFor="username">{t('auth.username')}</label>
          <input
            id="username" className="input" autoComplete="username" required
            value={username} onChange={(e) => setUsername(e.target.value)}
          />
        </div>
        <div className="field">
          <label htmlFor="password">{t('auth.password')}</label>
          <input
            id="password" className="input" type="password" autoComplete="current-password" required
            value={password} onChange={(e) => setPassword(e.target.value)}
          />
        </div>
        {error != null && (
          <p role="alert" style={{ color: 'var(--danger)', fontSize: '.85rem' }}>
            {errorMessage(error)}
          </p>
        )}
        <Button type="submit" busy={busy}>{t('auth.login')}</Button>
      </form>
    </div>
  );
}

export function SetupPage() {
  const t = useT();
  const auth = useAuth();
  const navigate = useNavigate();
  const [username, setUsername] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);

  if (!auth.setupRequired) return <Navigate to="/anmelden" replace />;

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await auth.setup(username, displayName || username, password);
      navigate('/dashboard', { replace: true });
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="centered-page">
      <form className="panel panel--pad form-stack" style={{ width: 'min(92vw, 24rem)' }} onSubmit={submit}>
        <Brand />
        <h1 style={{ fontSize: '1.15rem' }}>{t('auth.setupTitle')}</h1>
        <p style={{ fontSize: '.85rem', color: 'var(--muted)' }}>{t('auth.setupIntro')}</p>
        <div className="field">
          <label htmlFor="s-username">{t('auth.username')}</label>
          <input id="s-username" className="input" autoComplete="username" required
                 value={username} onChange={(e) => setUsername(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="s-display">{t('auth.displayName')}</label>
          <input id="s-display" className="input" autoComplete="name"
                 value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="s-password">{t('auth.password')}</label>
          <input id="s-password" className="input" type="password" autoComplete="new-password"
                 required minLength={10}
                 value={password} onChange={(e) => setPassword(e.target.value)} />
          <small>{t('auth.passwordHint')}</small>
        </div>
        {error != null && (
          <p role="alert" style={{ color: 'var(--danger)', fontSize: '.85rem' }}>
            {errorMessage(error)}
          </p>
        )}
        <Button type="submit" busy={busy}>{t('common.save')}</Button>
      </form>
    </div>
  );
}
