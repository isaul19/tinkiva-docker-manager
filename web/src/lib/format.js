// Formateadores compartidos. Todo en es-ES para que el panel hable un solo idioma.

const NUMBER = new Intl.NumberFormat('es-ES');
const DATE_TIME = new Intl.DateTimeFormat('es-ES', {
  dateStyle: 'medium',
  timeStyle: 'short',
});

export function formatNumber(value) {
  return Number.isFinite(value) ? NUMBER.format(value) : '—';
}

export function formatBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const decimals = value >= 100 || unit === 0 ? 0 : 1;
  return `${value.toFixed(decimals)} ${units[unit]}`;
}

export function formatCompact(value) {
  if (!Number.isFinite(value)) return '—';
  if (value < 1000) return String(value);
  if (value < 1_000_000) return `${(value / 1000).toFixed(1)}k`;
  if (value < 1_000_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  return `${(value / 1_000_000_000).toFixed(1)}B`;
}

export function formatDateTime(seconds) {
  if (!seconds) return '—';
  return DATE_TIME.format(new Date(seconds * 1000));
}

export function formatRelative(seconds) {
  if (!seconds) return '—';
  const delta = Math.floor(Date.now() / 1000) - seconds;
  if (delta < 60) return 'hace instantes';
  if (delta < 3600) return `hace ${Math.floor(delta / 60)} min`;
  if (delta < 86_400) return `hace ${Math.floor(delta / 3600)} h`;
  if (delta < 2_592_000) return `hace ${Math.floor(delta / 86_400)} d`;
  return formatDateTime(seconds);
}

export function formatIsoRelative(iso) {
  if (!iso) return '—';
  const parsed = Date.parse(iso);
  return Number.isNaN(parsed) ? '—' : formatRelative(Math.floor(parsed / 1000));
}

const AGE_UNITS = {
  second: ['segundo', 'segundos'],
  minute: ['minuto', 'minutos'],
  hour: ['hora', 'horas'],
  day: ['día', 'días'],
  week: ['semana', 'semanas'],
  month: ['mes', 'meses'],
  year: ['año', 'años'],
};

/** «3 weeks ago» de Docker a «hace 3 semanas». */
export function formatDockerAge(value) {
  const text = String(value || '').trim();
  if (!text) return '—';
  if (/^about a minute ago$/i.test(text)) return 'hace un minuto';
  if (/^about an hour ago$/i.test(text)) return 'hace una hora';
  if (/^less than a second ago$/i.test(text)) return 'hace instantes';

  const match = text.match(/^(\d+)\s+(second|minute|hour|day|week|month|year)s?\s+ago$/i);
  if (!match) return text;
  const [, count, unit] = match;
  const [singular, plural] = AGE_UNITS[unit.toLowerCase()];
  return `hace ${count} ${count === '1' ? singular : plural}`;
}

export function formatDuration(milliseconds) {
  if (!Number.isFinite(milliseconds)) return '—';
  if (milliseconds < 1000) return `${Math.round(milliseconds)} ms`;
  const seconds = milliseconds / 1000;
  if (seconds < 60) return `${seconds.toFixed(1)} s`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes} min ${Math.round(seconds % 60)} s`;
}

/** Convierte «Storagia API» en «storagia-api» para proponer un slug. */
export function toSlug(value) {
  return value
    // NFD separa «á» en «a» + tilde combinante; descartar lo no ASCII deja la base.
    .normalize('NFD')
    .replace(/[^\x20-\x7E]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 48)
    .replace(/-+$/g, '');
}

/** Porcentaje «12.5%» de Docker a número. */
export function parsePercent(value) {
  if (typeof value !== 'string') return null;
  const parsed = Number.parseFloat(value.replace('%', '').trim());
  return Number.isFinite(parsed) ? parsed : null;
}
