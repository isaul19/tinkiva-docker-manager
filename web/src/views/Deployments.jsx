import { useState } from 'preact/hooks';
import { Rocket } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { useAsync } from '../lib/hooks.js';
import { formatDateTime, formatDuration, formatRelative } from '../lib/format.js';
import { AsyncBlock, Badge, EmptyState, Panel, stateTone } from '../ui/Primitives.jsx';

export function Deployments() {
  const { refreshToken } = useApp();
  const [project, setProject] = useState('');

  const projects = useAsync(() => api.get('/api/projects'), [refreshToken]);
  const history = useAsync(
    () => api.get('/api/history', { project, limit: 200 }),
    [refreshToken, project],
  );

  return (
    <>
      <div class="section-head">
        <p class="muted">
          Historial local de despliegues manuales, webhooks, rollbacks y altas de recursos.
        </p>
        <select
          class="input"
          value={project}
          onChange={(event) => setProject(event.currentTarget.value)}
          aria-label="Filtrar por proyecto"
        >
          <option value="">Todos los recursos</option>
          {(projects.data || []).map((item) => (
            <option key={item.slug} value={item.slug}>
              {item.name}
            </option>
          ))}
        </select>
      </div>

      <Panel class="table-panel">
        <AsyncBlock
          query={history}
          empty={
            <EmptyState
              icon={Rocket}
              title="Sin despliegues registrados"
              description="Aquí aparecerá cada intento, con su duración y su resultado."
            />
          }
        >
          {(list) => (
            <div class="table-scroll">
              <table class="data-table">
                <thead>
                  <tr>
                    <th>Cuándo</th>
                    <th>Recurso</th>
                    <th>Estado</th>
                    <th>Origen</th>
                    <th>Imagen</th>
                    <th>Duración</th>
                    <th>Detalle</th>
                  </tr>
                </thead>
                <tbody>
                  {list.map((deployment) => (
                    <tr key={deployment.id}>
                      <td>
                        <span title={formatDateTime(deployment.created_at)}>
                          {formatRelative(deployment.created_at)}
                        </span>
                      </td>
                      <td>
                        <strong>{deployment.project}</strong>
                        {deployment.branch ? (
                          <span class="muted block small mono">{deployment.branch}</span>
                        ) : null}
                      </td>
                      <td>
                        <Badge tone={stateTone(deployment.status)}>{deployment.status}</Badge>
                      </td>
                      <td class="muted">{deployment.trigger}</td>
                      <td class="mono small truncate" title={deployment.image || ''}>
                        {deployment.image || '—'}
                        {deployment.commit ? (
                          <span class="muted block">{deployment.commit}</span>
                        ) : null}
                      </td>
                      <td class="numeric">{formatDuration(deployment.duration_ms)}</td>
                      <td class="detail-cell">
                        <details>
                          <summary>Ver mensaje</summary>
                          <pre>{deployment.message}</pre>
                        </details>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </AsyncBlock>
      </Panel>
    </>
  );
}
