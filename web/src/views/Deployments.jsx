import { useState } from 'preact/hooks';
import { Rocket } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { useAsync } from '../lib/hooks.js';
import { formatDateTime, formatDuration, formatRelative } from '../lib/format.js';
import {
  AsyncBlock,
  Badge,
  deploymentStatusLabel,
  EmptyState,
  Pagination,
  Panel,
  stateTone,
} from '../ui/Primitives.jsx';
import { Select } from '../ui/Form.jsx';

const PAGE_SIZE = 10;

export function Deployments() {
  const { refreshToken } = useApp();
  const [project, setProject] = useState('');
  const [page, setPage] = useState(0);
  const projects = useAsync(() => api.get('/api/projects'), [refreshToken]);
  const history = useAsync(
    () => api.get('/api/history/page', { project, offset: page * PAGE_SIZE, limit: PAGE_SIZE }),
    [refreshToken, project, page],
  );
  const projectOptions = [
    { value: '', label: 'Todos los recursos' },
    ...(projects.data || []).map((item) => ({ value: item.slug, label: item.name })),
  ];

  return (
    <>
      <div class="section-head">
        <p class="muted">
          Historial local de despliegues manuales, webhooks, rollbacks y cambios de configuración.
        </p>
        <div class="deployment-filter">
          <Select
            value={project}
            onChange={(event) => {
              setProject(event.currentTarget.value);
              setPage(0);
            }}
            options={projectOptions}
            aria-label="Filtrar por proyecto"
          />
        </div>
      </div>
      <Panel class="table-panel">
        <AsyncBlock query={history}>
          {(data) => {
            const list = data?.items || [];
            const total = data?.total || 0;
            if (!list.length)
              return (
                <EmptyState
                  icon={Rocket}
                  title="Sin despliegues registrados"
                  description="Aquí aparecerá cada intento, con su duración y su resultado."
                />
              );
            return (
              <>
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
                            <Badge tone={stateTone(deployment.status)}>{deploymentStatusLabel(deployment.status)}</Badge>
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
                <Pagination
                  page={page}
                  total={total}
                  pageSize={PAGE_SIZE}
                  onPageChange={setPage}
                />
              </>
            );
          }}
        </AsyncBlock>
      </Panel>
    </>
  );
}
