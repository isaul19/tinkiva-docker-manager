import { useState } from 'preact/hooks';
import { Container, FileText, Play, RotateCw, Square } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { useAsync, usePolling } from '../lib/hooks.js';
import { parsePercent } from '../lib/format.js';
import {
  AsyncBlock,
  Badge,
  Button,
  EmptyState,
  Meter,
  Pagination,
  Panel,
  stateTone,
} from '../ui/Primitives.jsx';
import { useToast } from '../ui/Toast.jsx';
import { LogsDialog } from './LogsDialog.jsx';

const PAGE_SIZE = 10;

export function Containers() {
  const { refreshToken, openAddResource } = useApp();
  const toast = useToast();
  const [busy, setBusy] = useState(null);
  const [logsFor, setLogsFor] = useState(null);
  const [page, setPage] = useState(0);

  const containers = useAsync(() => api.get('/api/containers'), [refreshToken]);
  usePolling(containers.reload, 10_000);

  const act = async (name, action) => {
    setBusy(`${name}:${action}`);
    try {
      const result = await api.post(`/api/containers/${encodeURIComponent(name)}/${action}`);
      toast.success(result.message || `${action} aplicado a ${name}`);
      containers.reload();
    } catch (error) {
      toast.error(error);
    } finally {
      setBusy(null);
    }
  };

  return (
    <>
      <p class="page-intro muted">
        CPU, memoria, estado, puertos y logs de todos los contenedores del host.
      </p>

      <Panel class="table-panel">
        <AsyncBlock
          query={containers}
          empty={
            <EmptyState
              icon={Container}
              title="No hay contenedores"
              description="Crea un recurso para que Docker levante el primero."
              action={
                <Button variant="primary" size="sm" onClick={openAddResource}>
                  Añadir recurso
                </Button>
              }
            />
          }
        >
          {(list) => {
            const lastPage = Math.max(0, Math.ceil(list.length / PAGE_SIZE) - 1);
            const currentPage = Math.min(page, lastPage);
            const visible = list.slice(currentPage * PAGE_SIZE, (currentPage + 1) * PAGE_SIZE);
            return <>
              <div class="table-scroll">
                <table class="data-table">
                <thead>
                  <tr>
                    <th>Contenedor</th>
                    <th>Estado</th>
                    <th>CPU</th>
                    <th>Memoria</th>
                    <th>Puertos</th>
                    <th class="align-end">Acciones</th>
                  </tr>
                </thead>
                <tbody>
                  {visible.map((container) => {
                    const running = String(container.state).toLowerCase().includes('running');
                    const cpu = parsePercent(container.cpu);
                    const memory = parsePercent(container.memory_percent);
                    return (
                      <tr key={container.id}>
                        <td>
                          <div class="cell-stack">
                            <strong>{container.name}</strong>
                            <span class="muted mono">{container.image}</span>
                          </div>
                        </td>
                        <td>
                          <Badge tone={stateTone(container.state)}>{container.state}</Badge>
                          <span class="muted block">{container.status}</span>
                        </td>
                        <td class="numeric">
                          {container.cpu || '—'}
                          {cpu !== null ? <Meter value={cpu} /> : null}
                        </td>
                        <td class="numeric">
                          {container.memory || '—'}
                          {memory !== null ? <Meter value={memory} /> : null}
                        </td>
                        <td class="muted mono small">{container.ports || '—'}</td>
                        <td class="align-end">
                          <div class="row-actions">
                            <Button
                              size="sm"
                              icon={FileText}
                              onClick={() => setLogsFor(container.name)}
                              title="Ver logs"
                            />
                            {running ? (
                              <>
                                <Button
                                  size="sm"
                                  icon={RotateCw}
                                  loading={busy === `${container.name}:restart`}
                                  onClick={() => act(container.name, 'restart')}
                                  title="Reiniciar"
                                />
                                <Button
                                  size="sm"
                                  icon={Square}
                                  loading={busy === `${container.name}:stop`}
                                  onClick={() => act(container.name, 'stop')}
                                  title="Detener"
                                />
                              </>
                            ) : (
                              <Button
                                size="sm"
                                icon={Play}
                                loading={busy === `${container.name}:start`}
                                onClick={() => act(container.name, 'start')}
                                title="Arrancar"
                              />
                            )}
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
                </table>
              </div>
              <Pagination page={currentPage} total={list.length} pageSize={PAGE_SIZE} onPageChange={setPage} />
            </>;
          }}
        </AsyncBlock>
      </Panel>

      <LogsDialog
        open={Boolean(logsFor)}
        onClose={() => setLogsFor(null)}
        source="container"
        target={logsFor}
        title={logsFor}
      />
    </>
  );
}
