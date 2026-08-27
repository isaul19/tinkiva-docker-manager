import { useEffect, useState } from 'preact/hooks';
import { KeyRound, UserRound } from 'lucide-preact';
import { api } from '../lib/api.js';
import { BrandIcon } from '../ui/BrandIcon.jsx';
import { Button } from '../ui/Primitives.jsx';

export function Login({ mustChangePassword = false }) {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const [retryAfter, setRetryAfter] = useState(0);

  useEffect(() => {
    if (!retryAfter) return undefined;
    const timer = setInterval(() => setRetryAfter((seconds) => Math.max(0, seconds - 1)), 1000);
    return () => clearInterval(timer);
  }, [retryAfter > 0]);

  const submitLogin = async (event) => {
    event.preventDefault();
    if (retryAfter > 0) return;
    setBusy(true);
    setError(null);
    try {
      await api.signIn(username.trim(), password);
    } catch (cause) {
      setError(cause.message);
      setRetryAfter(Number(cause.payload?.retry_after_seconds) || 0);
    } finally {
      setBusy(false);
    }
  };

  const submitPassword = async (event) => {
    event.preventDefault();
    if (password.length < 12) {
      setError('La contraseña debe tener al menos 12 caracteres.');
      return;
    }
    if (password !== confirmation) {
      setError('Las contraseñas no coinciden.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.changePassword(password);
    } catch (cause) {
      setError(cause.message);
    } finally {
      setBusy(false);
    }
  };

  const blockedMessage = retryAfter > 0
    ? retryAfter >= 60 * 60
      ? `Acceso bloqueado. Intenta de nuevo en ${Math.ceil(retryAfter / 3600)} h.`
      : `Podrás volver a intentarlo en ${Math.ceil(retryAfter / 60)} min.`
    : null;

  return (
    <main class="login-view">
      <div class="login-card">
        <div class="brand-mark" aria-hidden="true">T</div>
        <p class="eyebrow">TINKIVA</p>
        <h1>{mustChangePassword ? 'Protege tu cuenta' : 'Docker Manager'}</h1>
        <p class="muted">
          {mustChangePassword
            ? 'Antes de abrir el panel, reemplaza la contraseña inicial por una personal.'
            : 'Ingresa con el usuario y la contraseña configurados para este servidor.'}
        </p>

        {mustChangePassword ? (
          <form onSubmit={submitPassword} autocomplete="off">
            <PasswordField
              label="Nueva contraseña"
              name="new-password"
              value={password}
              onInput={setPassword}
              autofocus
            />
            <PasswordField
              label="Confirmar contraseña"
              name="confirm-password"
              value={confirmation}
              onInput={setConfirmation}
            />
            {error ? <p class="login-error" role="alert">{error}</p> : null}
            <Button variant="primary" size="lg" type="submit" loading={busy}>
              Guardar y entrar
            </Button>
          </form>
        ) : (
          <form onSubmit={submitLogin} autocomplete="off">
            <label class="field field-wide">
              <span class="field-label">Usuario</span>
              <div class="input-with-icon">
                <UserRound size={16} />
                <input
                  class="input"
                  name="username"
                  maxlength={64}
                  required
                  autofocus
                  autocomplete="username"
                  value={username}
                  onInput={(event) => setUsername(event.currentTarget.value)}
                />
              </div>
            </label>
            <PasswordField
              label="Contraseña"
              name="password"
              value={password}
              onInput={setPassword}
              autocomplete="current-password"
            />
            {error ? <p class="login-error" role="alert">{error}</p> : null}
            {blockedMessage ? <p class="login-lock" role="status">{blockedMessage}</p> : null}
            <Button variant="primary" size="lg" type="submit" loading={busy} disabled={retryAfter > 0}>
              {retryAfter > 0 ? 'Acceso temporalmente bloqueado' : 'Entrar'}
            </Button>
          </form>
        )}

        <p class="login-note">La sesión se guarda solo en esta pestaña.</p>
        <footer class="built-with">
          <span>Construido con</span>
          <BrandIcon slug="rust" size={16} title="Rust" />
          <strong>Rust</strong>
          <span aria-hidden="true">+</span>
          <BrandIcon slug="preact" size={16} title="Preact" />
          <strong>Preact</strong>
        </footer>
      </div>
    </main>
  );
}

function PasswordField({
  label,
  name,
  value,
  onInput,
  autofocus = false,
  autocomplete = 'new-password',
}) {
  return (
    <label class="field field-wide">
      <span class="field-label">{label}</span>
      <div class="input-with-icon">
        <KeyRound size={16} />
        <input
          class="input"
          type="password"
          name={name}
          minlength={12}
          maxlength={256}
          required
          autofocus={autofocus}
          autocomplete={autocomplete}
          value={value}
          onInput={(event) => onInput(event.currentTarget.value)}
        />
      </div>
    </label>
  );
}
