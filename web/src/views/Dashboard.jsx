import { AlertTriangle, Container } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { navigate, useAsync, usePolling } from '../lib/hooks.js';
import { formatBytes } from '../lib/format.js';
import { AsyncBlock, Badge, Button, EmptyState, Panel, Stat, containerStateLabel, stateTone } from '../ui/Primitives.jsx';

export function Dashboard() {
  const { info, refreshToken } = useApp();
  const metrics = useAsync(() => api.get('/api/system'), [refreshToken]);
  const containers = useAsync(() => api.get('/api/containers'), [refreshToken]);
  usePolling(() => { metrics.reload(); containers.reload(); }, 15_000);

  const system = metrics.data;
  const memoryPercent = system?.memory_total ? (system.memory_used / system.memory_total) * 100 : 0;
  const list = containers.data || [];
  const running = list.filter((container) => String(container.state).toLowerCase() === 'running').length;
  const stopped = list.length - running;

  return (
    <>
      <div class="stat-grid">
        <Stat label="CPU del host" value={system ? `${system.cpu_percent.toFixed(1)}%` : '—'} hint={system ? `${system.cpu_threads} hilos · carga ${system.load_1.toFixed(2)}` : ''} meter={system?.cpu_percent ?? 0} tone={system?.cpu_percent > 85 ? 'danger' : 'accent'} />
        <Stat label="Memoria del host" value={system ? formatBytes(system.memory_used) : '—'} hint={system ? `${formatBytes(system.memory_available)} disponibles` : ''} meter={memoryPercent} tone={memoryPercent > 90 ? 'danger' : 'accent'} />
        <Stat label="Disco raíz" value={system ? formatBytes(system.disk_used) : '—'} hint={system ? `${formatBytes(system.disk_available)} libres` : ''} meter={system?.disk_percent ?? 0} tone={system?.disk_percent > 90 ? 'danger' : 'accent'} />
        <Stat label="Contenedores activos" value={containers.data ? `${running}/${list.length}` : '—'} hint={stopped ? `${stopped} requieren atención` : 'Todos operativos'} meter={list.length ? (running / list.length) * 100 : 0} tone={stopped ? 'danger' : 'accent'} />
      </div>

      <Panel eyebrow="DOCKER" title="Estado de aplicaciones" action={<Button size="sm" onClick={() => navigate('containers')}>Ver contenedores y logs</Button>}>
        <AsyncBlock query={containers} empty={<EmptyState icon={Container} title="Sin contenedores" description="Los despliegues externos aparecerán automáticamente cuando creen contenedores en este host." />}>
          {(items) => (
            <ul class="compact-list">
              {items.slice(0, 10).map((container) => (
                <li key={container.id}>
                  <div class="compact-main"><strong>{container.name}</strong><span class="muted">{container.image}</span></div>
                  <div class="compact-side">
                    <Badge tone={stateTone(container.state)}>{containerStateLabel(container.state)}</Badge>
                    {container.memory ? <span class="muted">{container.memory}</span> : null}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </AsyncBlock>
      </Panel>

      {stopped > 0 ? <Panel eyebrow="ATENCIÓN" title="Contenedores detenidos"><p class="muted"><AlertTriangle size={16} /> Revisa los contenedores marcados como detenidos y sus logs. Esta edición no los reinicia ni modifica.</p></Panel> : null}
      {info?.mode === 'read-only' ? <p class="muted small">Los despliegues son administrados externamente. Este panel opera en modo de solo lectura.</p> : null}
    </>
  );
}
