import { useMemo, useState } from 'preact/hooks';
import { ArrowDown, ArrowUp, Cpu } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { useAsync, usePolling } from '../lib/hooks.js';
import { formatBytes } from '../lib/format.js';
import { AsyncBlock, EmptyState, Pagination, Panel } from '../ui/Primitives.jsx';

const PAGE_SIZE = 10;

const COLUMNS = [
  { key: 'cpu_percent', label: 'CPU', numeric: true },
  { key: 'memory_bytes', label: 'Memoria', numeric: true },
];

export function Processes() {
  const { refreshToken } = useApp();
  const [sort, setSort] = useState({ key: 'cpu_percent', direction: 'desc' });
  const [filter, setFilter] = useState('');
  const [page, setPage] = useState(0);

  const processes = useAsync(() => api.get('/api/processes'), [refreshToken]);
  usePolling(processes.reload, 10_000);

  const rows = useMemo(() => {
    const list = processes.data || [];
    const needle = filter.trim().toLowerCase();
    const filtered = needle
      ? list.filter(
          (entry) =>
            entry.name.toLowerCase().includes(needle) ||
            entry.command.toLowerCase().includes(needle) ||
            entry.user.toLowerCase().includes(needle),
        )
      : list;
    const factor = sort.direction === 'desc' ? -1 : 1;
    return [...filtered].sort((left, right) => (left[sort.key] - right[sort.key]) * factor);
  }, [processes.data, sort, filter]);

  const toggle = (key) => {
    setPage(0);
    setSort((current) =>
      current.key === key
        ? { key, direction: current.direction === 'desc' ? 'asc' : 'desc' }
        : { key, direction: 'desc' },
    );
  };

  return (
    <>
      <div class="section-head">
        <p class="muted">Procesos del host. Pulsa CPU o memoria para reordenar.</p>
        <input
          class="input search"
          type="search"
          placeholder="Filtrar por nombre, usuario o comando"
          value={filter}
          onInput={(event) => { setFilter(event.currentTarget.value); setPage(0); }}
        />
      </div>

      <Panel class="table-panel">
        <AsyncBlock
          query={processes}
          empty={<EmptyState icon={Cpu} title="No se pudieron leer los procesos" />}
        >
          {() => {
            const lastPage = Math.max(0, Math.ceil(rows.length / PAGE_SIZE) - 1);
            const currentPage = Math.min(page, lastPage);
            const visible = rows.slice(currentPage * PAGE_SIZE, (currentPage + 1) * PAGE_SIZE);
            return <>
              <div class="table-scroll">
                <table class="data-table">
                <thead>
                  <tr>
                    <th>PID</th>
                    <th>Proceso</th>
                    <th>Usuario</th>
                    {COLUMNS.map((column) => (
                      <th key={column.key}>
                        <button type="button" class="sort-button" onClick={() => toggle(column.key)}>
                          {column.label}
                          {sort.key === column.key ? (
                            sort.direction === 'desc' ? (
                              <ArrowDown size={13} />
                            ) : (
                              <ArrowUp size={13} />
                            )
                          ) : null}
                        </button>
                      </th>
                    ))}
                    <th>Comando</th>
                  </tr>
                </thead>
                <tbody>
                  {visible.map((entry) => (
                    <tr key={entry.pid}>
                      <td class="numeric mono">{entry.pid}</td>
                      <td>
                        <strong>{entry.name}</strong>
                      </td>
                      <td class="muted">{entry.user}</td>
                      <td class="numeric">{entry.cpu_percent.toFixed(1)}%</td>
                      <td class="numeric">{formatBytes(entry.memory_bytes)}</td>
                      <td class="muted mono small" title={entry.command}>
                        <span class="command-cell">{entry.command}</span>
                      </td>
                    </tr>
                  ))}
                </tbody>
                </table>
              </div>
              <Pagination page={currentPage} total={rows.length} pageSize={PAGE_SIZE} onPageChange={setPage} />
            </>;
          }}
        </AsyncBlock>
      </Panel>
    </>
  );
}
