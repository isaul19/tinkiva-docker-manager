import { Loader2 } from 'lucide-preact';

export function Button({
  variant = 'ghost',
  size = 'md',
  icon: Icon,
  loading = false,
  disabled = false,
  children,
  ...rest
}) {
  return (
    <button
      type="button"
      class={`btn btn-${variant} btn-${size}${loading ? ' is-loading' : ''}`}
      disabled={disabled || loading}
      {...rest}
    >
      {loading ? <Loader2 class="spin" size={15} /> : Icon ? <Icon size={15} /> : null}
      {children ? <span>{children}</span> : null}
    </button>
  );
}

export function Panel({ eyebrow, title, action, children, class: className = '', ...rest }) {
  return (
    <section class={`panel ${className}`} {...rest}>
      {(eyebrow || title || action) && (
        <header class="panel-head">
          <div>
            {eyebrow ? <p class="eyebrow">{eyebrow}</p> : null}
            {title ? <h3>{title}</h3> : null}
          </div>
          {action}
        </header>
      )}
      {children}
    </section>
  );
}

export function Badge({ tone = 'neutral', children }) {
  return <span class={`badge badge-${tone}`}>{children}</span>;
}

/** Traduce el estado de Docker a un tono de color. */
export function stateTone(state) {
  const value = String(state || '').toLowerCase();
  if (value.includes('running') || value === 'success') return 'ok';
  if (value.includes('exited') || value.includes('dead') || value === 'failed') return 'danger';
  if (value.includes('restarting') || value.includes('paused') || value.includes('created')) {
    return 'warning';
  }
  return 'neutral';
}

export function containerStateLabel(state) {
  const labels = {
    created: 'Creado',
    restarting: 'Reiniciando',
    running: 'En ejecucion',
    removing: 'Eliminando',
    paused: 'Pausado',
    exited: 'Finalizado',
    dead: 'Inactivo',
  };
  return labels[String(state || '').toLowerCase()] || state || 'Desconocido';
}

export function deploymentStatusLabel(status) {
  const labels = {
    success: 'Completado',
    failed: 'Fallido',
    running: 'En curso',
    pending: 'Pendiente',
  };
  return labels[String(status || '').toLowerCase()] || status || 'Desconocido';
}

export function Spinner({ label = 'Cargando…' }) {
  return (
    <div class="state-block">
      <Loader2 class="spin" size={18} />
      <span>{label}</span>
    </div>
  );
}

export function EmptyState({ icon: Icon, title, description, action }) {
  return (
    <div class="state-block state-empty">
      {Icon ? <Icon size={26} class="state-icon" /> : null}
      <p class="state-title">{title}</p>
      {description ? <p class="state-description">{description}</p> : null}
      {action}
    </div>
  );
}

export function ErrorState({ error, onRetry }) {
  const message = error?.message || 'Algo salió mal';
  return (
    <div class="state-block state-error">
      <p class="state-title">{message}</p>
      {onRetry ? (
        <Button variant="ghost" size="sm" onClick={onRetry}>
          Reintentar
        </Button>
      ) : null}
    </div>
  );
}

/** Envuelve el trío cargando / error / vacío que repiten todas las vistas. */
export function AsyncBlock({ query, empty, children }) {
  if (query.loading && !query.data) return <Spinner />;
  if (query.error) return <ErrorState error={query.error} onRetry={query.reload} />;
  const isEmpty = Array.isArray(query.data) ? query.data.length === 0 : !query.data;
  if (isEmpty && empty) return empty;
  return children(query.data);
}

export function Pagination({ page = 0, total = 0, pageSize = 10, onPageChange }) {
  const pages = Math.max(1, Math.ceil(total / pageSize));
  const current = Math.min(Math.max(0, page), pages - 1);
  const start = total ? current * pageSize + 1 : 0;
  const end = Math.min(total, (current + 1) * pageSize);
  if (total <= pageSize) return null;
  return (
    <div class="pagination" aria-label="Paginacion">
      <span class="muted small">{start} - {end} de {total}</span>
      <div class="pagination-actions">
        <Button size="sm" disabled={current === 0} onClick={() => onPageChange?.(current - 1)}>Anterior</Button>
        <span class="pagination-page">{current + 1} / {pages}</span>
        <Button size="sm" disabled={current >= pages - 1} onClick={() => onPageChange?.(current + 1)}>Siguiente</Button>
      </div>
    </div>
  );
}

export function Meter({ value, tone = 'accent' }) {
  const clamped = Math.max(0, Math.min(100, Number(value) || 0));
  return (
    <div class="meter" role="presentation">
      <span class={`meter-fill meter-${tone}`} style={`width:${clamped}%`} />
    </div>
  );
}

export function Stat({ label, value, hint, meter, tone }) {
  return (
    <article class="stat">
      <p class="stat-label">{label}</p>
      <p class="stat-value">{value}</p>
      {hint ? <p class="stat-hint">{hint}</p> : null}
      {typeof meter === 'number' ? <Meter value={meter} tone={tone} /> : null}
    </article>
  );
}
