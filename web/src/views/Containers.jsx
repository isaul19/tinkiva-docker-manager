import { useState } from 'preact/hooks';
import { Container, FileTerminal, FileText, Play, RotateCw, Square } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { useAsync, usePolling } from '../lib/hooks.js';
import { parsePercent } from '../lib/format.js';
import {
  AsyncBlock,
  Badge,
  Button,
  containerStateLabel,
  EmptyState,
  Meter,
  Pagination,
  Panel,
  stateTone,
} from '../ui/Primitives.jsx';
import { useToast } from '../ui/Toast.jsx';
import { LogsDialog } from './LogsDialog.jsx';
import { Field, Input, TextArea } from '../ui/Form.jsx';
import { Modal } from '../ui/Modal.jsx';

const PAGE_SIZE = 10;

function containerStatusLabel(status) {
  let value = String(status || '');
  value = value.replace(/^Up\s+/i, 'En ejecución desde hace ');
  value = value.replace(/^Exited/i, 'Finalizado').replace(/^Created/i, 'Creado');
  value = value.replace(/\(healthy\)/ig, '(saludable)').replace(/\(unhealthy\)/ig, '(no saludable)');
  value = value.replace(/About an hour ago/ig, 'hace cerca de una hora');
  value = value.replace(/(\d+)\s+(minute|hour|day|week)s?\s+ago/ig, (_, count, unit) => {
    const units = { minute: 'minuto', hour: 'hora', day: 'día', week: 'semana' };
    return `hace ${count} ${units[unit.toLowerCase()]}${count === '1' ? '' : 's'}`;
  });
  value = value.replace(/(\d+)\s+(minute|hour|day|week)s?(?=\s|$|\()/ig, (_, count, unit) => {
    const units = { minute: 'minuto', hour: 'hora', day: 'día', week: 'semana' };
    return `${count} ${units[unit.toLowerCase()]}${count === '1' ? '' : 's'}`;
  });
  return value || 'Sin información';
}

export function Containers() {
  const { refreshToken, openAddResource } = useApp();
  const toast = useToast();
  const [busy, setBusy] = useState(null);
  const [logsFor, setLogsFor] = useState(null);
  const [consoleFor, setConsoleFor] = useState(null);
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

  const openConsole = async (container) => {
    setBusy(`${container.name}:console-info`);
    try {
      const info = await api.get(`/api/containers/${encodeURIComponent(container.name)}/console`);
      setConsoleFor({ container, info });
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
                          <Badge tone={stateTone(container.state)}>{containerStateLabel(container.state)}</Badge>
                          <span class="muted block">{containerStatusLabel(container.status)}</span>
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
                                <Button size="sm" icon={FileTerminal} loading={busy === `${container.name}:console-info`} onClick={() => openConsole(container)} title="Abrir consola" />
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
      <ConsoleDialog key={consoleFor?.container.id || 'closed'} state={consoleFor} onClose={() => setConsoleFor(null)} />
    </>
  );
}

function ConsoleDialog({ state, onClose }) {
  const [input, setInput] = useState('');
  const [output, setOutput] = useState('');
  const [busy, setBusy] = useState(false);
  if (!state) return null;
  const { container, info } = state;
  const database = info.database;
  const prompt = '>';

  const execute = async (event) => {
    event.preventDefault();
    if (!input.trim()) return;
    const submitted = input;
    setBusy(true);
    try {
      const result = await api.post(`/api/containers/${encodeURIComponent(container.name)}/console`, { input: submitted });
      setOutput((current) => `${current}${current ? '\n' : ''}${prompt} ${submitted}\n${result.output || '(sin salida)'}\n`);
      setInput('');
    } catch (error) {
      setOutput((current) => `${current}${current ? '\n' : ''}${prompt} ${submitted}\nError: ${error.message}\n`);
    } finally {
      setBusy(false);
    }
  };

  const description = database
    ? `${info.database_label} detectado. Ejecuta consultas con el cliente disponible dentro del contenedor.`
    : 'Shell del contenedor. Ejecuta comandos dentro de este contenedor, nunca en el host.';

  return <Modal open onClose={onClose} eyebrow="CONSOLA" title={container.name} description={description} size="xl" footer={<>
    <Button onClick={onClose}>Cerrar</Button>
    <Button variant="primary" type="submit" form="container-console" loading={busy}>Ejecutar</Button>
  </>}>
    <p class="muted small">Usuario: <code>{info.user}</code></p>
    <pre class="logs console-output" tabIndex={0}>{output || 'La salida de los comandos aparecerá aquí.'}</pre>
    <form id="container-console" onSubmit={execute}>
      <Field label={database ? 'Consulta' : 'Comando'} hint={database ? 'Admite consultas y comandos del cliente, por ejemplo \\dt en PostgreSQL.' : 'Se ejecuta con sh -lc dentro del contenedor; máximo 4096 caracteres.'}>
        {database ? (
          <TextArea rows={6} value={input} onInput={(event) => setInput(event.currentTarget.value)} placeholder={database === 'postgres' ? 'SELECT * FROM products LIMIT 10;' : 'Escribe una consulta'} autofocus />
        ) : (
          <Input value={input} onInput={(event) => setInput(event.currentTarget.value)} placeholder="npm run db:migrate" autofocus />
        )}
      </Field>
    </form>
  </Modal>;
}
