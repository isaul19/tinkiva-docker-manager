import { CheckCircle2, XCircle } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { useAsync, usePolling } from '../lib/hooks.js';
import { formatBytes, formatNumber } from '../lib/format.js';
import { BrandIcon } from '../ui/BrandIcon.jsx';
import { AsyncBlock, Badge, Meter, Panel } from '../ui/Primitives.jsx';

function Rows({ entries }) {
  return (
    <dl class="definition-list">
      {entries.map(([label, value]) => (
        <div key={label}>
          <dt>{label}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

function formatUptime(seconds) {
  if (!seconds) return '—';
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days) return `${days} d ${hours} h`;
  if (hours) return `${hours} h ${minutes} min`;
  return `${minutes} min`;
}

export function System() {
  const { info, capabilities, refreshToken } = useApp();
  const metrics = useAsync(() => api.get('/api/system'), [refreshToken]);
  usePolling(metrics.reload, 15_000);

  const tools = [
    ['docker', info?.docker?.available, info?.docker?.server_version || info?.docker?.error],
    ['compose', Boolean(info?.docker?.compose_version), info?.docker?.compose_version || info?.docker?.compose_error],
    ['curl', capabilities.curl, 'Docker Hub y GitHub'],
    ['openssl', capabilities.openssl, 'Firma de JWT de GitHub App'],
    ['git', capabilities.git, 'Clonado de repositorios'],
  ];

  return (
    <>
      <p class="page-intro muted">
        Estado del host, del propio panel y de las herramientas externas que utiliza.
      </p>

      <div class="two-column">
        <Panel eyebrow="HOST" title="Recursos">
          <AsyncBlock query={metrics}>
            {(system) => (
              <>
                <Rows
                  entries={[
                    ['Hostname', system.hostname],
                    ['Uptime', formatUptime(system.uptime_seconds)],
                    ['Hilos de CPU', formatNumber(system.cpu_threads)],
                    [
                      'Carga',
                      `${system.load_1.toFixed(2)} · ${system.load_5.toFixed(2)} · ${system.load_15.toFixed(2)}`,
                    ],
                    [
                      'Memoria',
                      `${formatBytes(system.memory_used)} de ${formatBytes(system.memory_total)}`,
                    ],
                    [
                      'Swap',
                      system.swap_total
                        ? `${formatBytes(system.swap_used)} de ${formatBytes(system.swap_total)}`
                        : 'Sin swap',
                    ],
                    [
                      'Disco raíz',
                      `${formatBytes(system.disk_used)} de ${formatBytes(system.disk_total)}`,
                    ],
                  ]}
                />
                <Meter value={system.disk_percent} tone={system.disk_percent > 90 ? 'danger' : 'accent'} />
              </>
            )}
          </AsyncBlock>
        </Panel>

        <Panel eyebrow="PANEL" title="Tinkiva Docker Manager">
          <Rows
            entries={[
              ['Versión', info ? `v${info.version}` : '—'],
              ['Workers HTTP', info ? formatNumber(info.workers) : '—'],
              ['Memoria residente', metrics.data ? formatBytes(metrics.data.process_rss) : '—'],
              ['Recursos registrados', info ? formatNumber(info.projects) : '—'],
              ['Despliegues guardados', info ? formatNumber(info.deployments) : '—'],
              ['Raíz permitida', <code>{info?.allowed_root || '—'}</code>],
              ['Directorio de datos', <code>{info?.data_dir || '—'}</code>],
            ]}
          />
          <p class="built-with">
            <span>Construido con</span>
            <BrandIcon slug="rust" size={15} title="Rust" />
            <strong>Rust</strong>
            <span aria-hidden="true">+</span>
            <BrandIcon slug="preact" size={15} title="Preact" />
            <strong>Preact</strong>
          </p>
          <p class="muted small">
            Sin dependencias de terceros en el binario: servidor HTTP, almacén y métricas están
            escritos sobre la biblioteca estándar de Rust.
          </p>
        </Panel>
      </div>

      <Panel eyebrow="DEPENDENCIAS" title="Herramientas externas">
        <ul class="tool-list">
          {tools.map(([name, available, detail]) => (
            <li key={name}>
              {available ? (
                <CheckCircle2 size={17} class="ok-icon" />
              ) : (
                <XCircle size={17} class="error-icon" />
              )}
              <div class="compact-main">
                <strong class="mono">{name}</strong>
                <span class="muted small">{detail || (available ? 'Disponible' : 'No instalado')}</span>
              </div>
              <Badge tone={available ? 'ok' : 'danger'}>
                {available ? 'Disponible' : 'Falta'}
              </Badge>
            </li>
          ))}
        </ul>
      </Panel>
    </>
  );
}
