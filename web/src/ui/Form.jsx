import { Check, ChevronDown, Copy } from 'lucide-preact';
import { useState } from 'preact/hooks';

export function Field({ label, hint, error, wide = false, children }) {
  return (
    <label class={`field${wide ? ' field-wide' : ''}${error ? ' field-error' : ''}`}>
      <span class="field-label">{label}</span>
      {children}
      {error ? <span class="field-message">{error}</span> : null}
      {!error && hint ? <span class="field-hint">{hint}</span> : null}
    </label>
  );
}

export function Input({ ...rest }) {
  return <input class="input" {...rest} />;
}

export function TextArea({ rows = 4, ...rest }) {
  return <textarea class="input textarea" rows={rows} {...rest} />;
}

export function Select({ options = [], ...rest }) {
  return (
    <span class="select-control">
      <select class="input select-input" {...rest}>
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
      <ChevronDown size={15} aria-hidden="true" />
    </span>
  );
}

/**
 * Casilla «Sin límite de RAM». Docker no obliga a fijar un límite y el panel
 * tampoco debería, pero llega desactivada a propósito: en un VPS pequeño un
 * contenedor sin techo puede dejar sin memoria al resto del servidor.
 */
export function MemoryUnlimited({ checked, onChange, engine }) {
  const warning =
    engine === 'Redis'
      ? 'Redis puede crecer hasta agotar la RAM del VPS; conviene mantener el límite.'
      : 'Docker no obliga a poner un límite, pero en un servidor pequeño conviene mantenerlo.';

  return (
    <label class={`exposure-option${checked ? ' is-public' : ''}`}>
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.currentTarget.checked)}
      />
      <span class="exposure-copy">
        <strong>Sin límite de RAM</strong>
        <span class="field-hint">
          {checked
            ? 'El contenedor podrá usar toda la memoria disponible del VPS. Puede afectar a otros servicios.'
            : warning}
        </span>
      </span>
    </label>
  );
}

export function FormGrid({ children, columns = 2 }) {
  return <div class={`form-grid form-grid-${columns}`}>{children}</div>;
}

/** Valor copiable: contraseñas, URIs de conexión y tokens de webhook. */
export function CopyValue({ value, masked = false, label }) {
  const [copied, setCopied] = useState(false);
  const [revealed, setRevealed] = useState(!masked);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div class="copy-value">
      {label ? <span class="copy-label">{label}</span> : null}
      <div class="copy-row">
        <code>{revealed ? value : '•'.repeat(Math.min(value.length, 32))}</code>
        {masked ? (
          <button type="button" class="icon-button" onClick={() => setRevealed((on) => !on)}>
            {revealed ? 'Ocultar' : 'Ver'}
          </button>
        ) : null}
        <button type="button" class="icon-button" onClick={copy} aria-label="Copiar">
          {copied ? <Check size={15} /> : <Copy size={15} />}
        </button>
      </div>
    </div>
  );
}
