import { useState } from 'preact/hooks';
import { KeyRound } from 'lucide-preact';
import { api } from '../lib/api.js';
import { BrandIcon } from '../ui/BrandIcon.jsx';
import { Button } from '../ui/Primitives.jsx';

export function Login() {
  const [token, setToken] = useState('');
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  const submit = async (event) => {
    event.preventDefault();
    if (token.length < 32) {
      setError('El token tiene al menos 32 caracteres.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.signIn(token);
    } catch (cause) {
      setError(cause.status === 401 ? 'Token incorrecto.' : cause.message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <main class="login-view">
      <div class="login-card">
        <div class="brand-mark" aria-hidden="true">
          T
        </div>
        <p class="eyebrow">TINKIVA</p>
        <h1>Docker Manager</h1>
        <p class="muted">
          Despliegues, contenedores y métricas de un servidor pequeño, sin la carga de una
          plataforma completa.
        </p>

        <form onSubmit={submit} autocomplete="off">
          <label class="field field-wide">
            <span class="field-label">Token administrador</span>
            <div class="input-with-icon">
              <KeyRound size={16} />
              <input
                class="input"
                type="password"
                name="token"
                minLength={32}
                required
                autofocus
                placeholder="TDM_ADMIN_TOKEN"
                value={token}
                onInput={(event) => setToken(event.currentTarget.value)}
              />
            </div>
            {error ? <span class="field-message">{error}</span> : null}
          </label>
          <Button variant="primary" size="lg" type="submit" loading={busy}>
            Entrar
          </Button>
        </form>

        <p class="login-note">El token se guarda solo en esta pestaña.</p>

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
