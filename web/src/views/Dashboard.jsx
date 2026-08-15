import { Boxes, Container, Rocket } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { navigate, useAsync, usePolling } from '../lib/hooks.js';
import { formatBytes, formatDuration, formatRelative } from '../lib/format.js';
import { AsyncBlock, Badge, Button, EmptyState, Panel, Stat, stateTone } from '../ui/Primitives.jsx';

export function Dashboard() {
  const { info, refreshToken, openAddResource } = useApp();

  const metrics = useAsync(() => api.get('/api/system'), [refreshToken]);
  const containers = useAsync(() => api.get('/api/containers'), [refreshToken]);
  const history = useAsync(() => api.get('/api/history', { limit: 6 }), [refreshToken]);

  usePolling(() => {
    metrics.reload();
    containers.reload();
  }, 15_000);

  const system = metrics.data;
  const memoryPercent = system?.memory_total
    ? (system.memory_used / system.memory_total) * 100
    : 0;
  const panelPercent = system?.memory_total
    ? (system.process_rss / system.memory_total) * 100
    : 0;

  const running = (containers.data || []).filter((container) =>
    String(container.state).toLowerCase().includes('running'),
  ).length;

  return (
    <>
      <div class="stat-grid">
        <Stat
          label="CPU del host"
          value={system ? `${system.cpu_percent.toFixed(1)}%` : '—'}
          hint={system ? `${system.cpu_threads} hilos · carga ${system.load_1.toFixed(2)}` : ''}
          meter={system?.cpu_percent ?? 0}
          tone={system?.cpu_percent > 85 ? 'danger' : 'accent'}
        />
        <Stat
          label="Memoria del host"
          value={system ? formatBytes(system.memory_used) : '—'}
            hint={
              system
                ? `${formatBytes(system.memory_available)} disponibles · ${formatBytes(system.memory_total)} total`
                : ''
            }
          meter={memoryPercent}
          tone={memoryPercent > 90 ? 'danger' : 'accent'}
        />
        <Stat
          label="Disco raíz"
          value={system ? formatBytes(system.disk_used) : '—'}
          hint={system ? `${formatBytes(system.disk_available)} libres` : ''}
          meter={system?.disk_percent ?? 0}
          tone={system?.disk_percent > 90 ? 'danger' : 'accent'}
        />
        <Stat
          label="Este panel"
          value={system ? formatBytes(system.process_rss) : '—'}
          hint={
            info
              ? `${info.workers} workers · ${panelPercent.toFixed(2)}% del host`
              : 'Consumo residente'
          }
          meter={panelPercent}
        />
      </div>

      <div class="two-column">
        <Panel
          eyebrow="DOCKER"
          title={`Contenedores${containers.data ? ` · ${running}/${containers.data.length}` : ''}`}
          action={
            <Button size="sm" onClick={() => navigate('containers')}>
              Ver todos
            </Button>
          }
        >
          <AsyncBlock
            query={containers}
            empty={
              <EmptyState
                icon={Container}
                title="Sin contenedores"
                description="Cuando despliegues un recurso aparecerá aquí."
                action={
                  <Button variant="primary" size="sm" onClick={openAddResource}>
                    Añadir recurso
                  </Button>
                }
              />
            }
          >
            {(list) => (
              <ul class="compact-list">
                {list.slice(0, 6).map((container) => (
                  <li key={container.id}>
                    <div class="compact-main">
                      <strong>{container.name}</strong>
                      <span class="muted">{container.image}</span>
                    </div>
                    <div class="compact-side">
                      <Badge tone={stateTone(container.state)}>{container.state}</Badge>
                      {container.memory ? <span class="muted">{container.memory}</span> : null}
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </AsyncBlock>
        </Panel>

        <Panel
          eyebrow="ACTIVIDAD"
          title="Últimos despliegues"
          action={
            <Button size="sm" onClick={() => navigate('deployments')}>
              Ver historial
            </Button>
          }
        >
          <AsyncBlock
            query={history}
            empty={
              <EmptyState
                icon={Rocket}
                title="Todavía no hay despliegues"
                description="El primero se registrará al crear o desplegar un recurso."
              />
            }
          >
            {(list) => (
              <ul class="compact-list">
                {list.map((deployment) => (
                  <li key={deployment.id}>
                    <div class="compact-main">
                      <strong>{deployment.project}</strong>
                      <span class="muted">
                        {formatRelative(deployment.created_at)} · {deployment.trigger} ·{' '}
                        {formatDuration(deployment.duration_ms)}
                      </span>
                    </div>
                    <Badge tone={stateTone(deployment.status)}>{deployment.status}</Badge>
                  </li>
                ))}
              </ul>
            )}
          </AsyncBlock>
        </Panel>
      </div>

      {info && info.projects === 0 ? (
        <Panel class="cta-panel">
          <EmptyState
            icon={Boxes}
            title="Aún no hay recursos registrados"
            description="Crea una base de datos, despliega una imagen de Docker Hub o conecta un repositorio de GitHub."
            action={
              <Button variant="primary" onClick={openAddResource}>
                Añadir el primer recurso
              </Button>
            }
          />
        </Panel>
      ) : null}
    </>
  );
}
