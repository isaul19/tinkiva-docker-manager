import { useEffect, useRef } from 'preact/hooks';
import { X } from 'lucide-preact';
import { useEscape } from '../lib/hooks.js';

/**
 * Diálogo modal accesible: bloquea el scroll de fondo, cierra con Escape y
 * lleva el foco al primer control al abrirse.
 */
export function Modal({ open, onClose, eyebrow, title, description, size = 'md', children, footer }) {
  const surface = useRef(null);
  useEscape(onClose, open);

  useEffect(() => {
    if (!open) return undefined;
    document.body.classList.add('modal-open');
    const focusable = surface.current?.querySelector(
      'input:not([type=hidden]), select, textarea, button',
    );
    focusable?.focus();
    return () => document.body.classList.remove('modal-open');
  }, [open]);

  if (!open) return null;

  return (
    <div
      class="modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div class={`modal modal-${size}`} role="dialog" aria-modal="true" ref={surface}>
        <header class="modal-head">
          <div>
            {eyebrow ? <p class="eyebrow">{eyebrow}</p> : null}
            <h3>{title}</h3>
            {description ? <p class="muted modal-description">{description}</p> : null}
          </div>
          <button type="button" class="icon-button" onClick={onClose} aria-label="Cerrar">
            <X size={18} />
          </button>
        </header>
        <div class="modal-body">{children}</div>
        {footer ? <footer class="modal-foot">{footer}</footer> : null}
      </div>
    </div>
  );
}
