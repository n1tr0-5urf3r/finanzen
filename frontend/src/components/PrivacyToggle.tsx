import { Eye, EyeOff } from 'lucide-react';

import { useT } from '../lib/i18n';
import { usePrivacy } from '../lib/privacy';

/**
 * Sits in every page header, because the moment it is wanted — someone looking
 * over a shoulder, a screen share starting — is never the moment to go and find
 * it in Settings.
 */
export function PrivacyToggle() {
  const t = useT();
  const { hidden, setHidden } = usePrivacy();
  const label = t(hidden ? 'privacy.show' : 'privacy.hide');

  return (
    <button
      type="button"
      className={`icon-button ${hidden ? 'is-active' : ''}`}
      onClick={() => setHidden(!hidden)}
      aria-pressed={hidden}
      title={label}
      aria-label={label}
    >
      {hidden ? <EyeOff size={18} aria-hidden="true" /> : <Eye size={18} aria-hidden="true" />}
    </button>
  );
}
