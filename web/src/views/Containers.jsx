import { useState } from 'preact/hooks';
import { Container, FileText } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { useAsync, usePolling } from '../lib/hooks.js';
import { AsyncBlock, Badge, Button, EmptyState, Meter, Pagination, Panel, containerStateLabel, stateTone } from '../ui/Primitives.jsx';
import { LogsDialog } from './LogsDialog.jsx';

const PAGE_SIZE = 10;
const parsePercent = (value) => {
  const parsed = Number.parseFloat(String(value || '').replace('%', ''));
  return Number.isFinite(parsed) ? parsed : null;
};

export function Containers() {
  const { refreshToken } = useApp();
  const [logsFor, setLogsFor] = useState(null);
  const [page, setPage] = useState(0);
  const containers = useAsync(() => api.get('/api/containers'), [refreshToken]);
  usePolling(containers.reload, 10_000);

  return (
    <>
      <p class="page-intro muted">Estado, imagen, CPU, memoria, puertos y logs. Los despliegues y cambios se realizan fuera de este panel.</p>
      <Panel class="table-panel">
        <AsyncBlock query={containers} empty={<EmptyState icon={Container} title="No hay contenedores" description="Los contenedores creados por TinkivaCreateApp aparecerán aquí automáticamente." />}>
          {(list) => {
            const lastPage = Math.max(0, Math.ceil(list.length / PAGE_SIZE) - 1);
            const currentPage = Math.min(page, lastPage);
            const visible = list.slice(currentPage * PAGE_SIZE, (currentPage + 1) * PAGE_SIZE);
            return <>
              <div class="table-scroll"><table class="data-table">
                <thead><tr><th>Contenedor</th><th>Estado</th><th>CPU</th><th>Memoria</th><th>Puertos</th><th class="align-end">Logs</th></tr></thead>
                <tbody>{visible.map((container) => {
                  const cpu = parsePercent(container.cpu);
                  const memory = parsePercent(container.memory_percent);
                  return <tr key={container.id}>
                    <td><div class="cell-stack"><strong>{container.name}</strong><span class="muted mono">{container.image}</span></div></td>
                    <td><Badge tone={stateTone(container.state)}>{containerStateLabel(container.state)}</Badge><span class="muted block">{container.status || 'Sin información'}</span></td>
                    <td class="numeric">{container.cpu || '—'}{cpu !== null ? <Meter value={cpu} /> : null}</td>
                    <td class="numeric">{container.memory || '—'}{memory !== null ? <Meter value={memory} /> : null}</td>
                    <td class="muted mono small">{container.ports || '—'}</td>
                    <td class="align-end"><Button size="sm" icon={FileText} onClick={() => setLogsFor(container.name)}>Ver logs</Button></td>
                  </tr>;
                })}</tbody>
              </table></div>
              <Pagination page={currentPage} total={list.length} pageSize={PAGE_SIZE} onPageChange={setPage} />
            </>;
          }}
        </AsyncBlock>
      </Panel>
      <LogsDialog open={Boolean(logsFor)} onClose={() => setLogsFor(null)} target={logsFor} title={logsFor} />
    </>
  );
}
