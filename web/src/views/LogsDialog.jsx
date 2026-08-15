import { useEffect, useRef, useState } from 'preact/hooks';
import { RefreshCw } from 'lucide-preact';
import { api } from '../lib/api.js';
import { usePolling } from '../lib/hooks.js';
import { Modal } from '../ui/Modal.jsx';
import { Button, Spinner } from '../ui/Primitives.jsx';

/**
 * Visor de logs para contenedores (`source="container"`) y stacks Compose
 * (`source="project"`). Sigue la salida en vivo si el usuario lo activa.
 */
export function LogsDialog({ open, onClose, source, target, title }) {
  const [logs, setLogs] = useState('');
  const [loading, setLoading] = useState(false);
  const [follow, setFollow] = useState(false);
  const [tail, setTail] = useState(300);
  const output = useRef(null);

  const path =
    source === 'container'
      ? `/api/containers/${encodeURIComponent(target)}/logs`
      : `/api/projects/${encodeURIComponent(target)}/logs`;

  const load = async () => {
    if (!open || !target) return;
    setLoading(true);
    try {
      const text = await api.text(path, { tail });
      setLogs(text || 'Sin salida.');
    } catch (error) {
      setLogs(`No se pudieron leer los logs: ${error.message}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (open) load();
    else {
      setLogs('');
      setFollow(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, target, tail]);

  useEffect(() => {
    if (follow && output.current) output.current.scrollTop = output.current.scrollHeight;
  }, [logs, follow]);

  usePolling(() => {
    if (open && follow) load();
  }, follow ? 4000 : 0);

  return (
    <Modal
      open={open}
      onClose={onClose}
      eyebrow="LOGS"
      title={title || target}
      size="lg"
      footer={
        <>
          <label class="inline-field">
            <span>Líneas</span>
            <select
              class="input"
              value={String(tail)}
              onChange={(event) => setTail(Number(event.currentTarget.value))}
            >
              <option value="100">100</option>
              <option value="300">300</option>
              <option value="1000">1000</option>
              <option value="2000">2000</option>
            </select>
          </label>
          <label class="inline-field checkbox">
            <input
              type="checkbox"
              checked={follow}
              onChange={(event) => setFollow(event.currentTarget.checked)}
            />
            <span>Seguir en vivo</span>
          </label>
          <Button icon={RefreshCw} onClick={load} loading={loading}>
            Actualizar
          </Button>
        </>
      }
    >
      {loading && !logs ? <Spinner /> : <pre class="logs" ref={output} tabIndex={0}>{logs}</pre>}
    </Modal>
  );
}
