import { useEffect, useRef, useState } from 'preact/hooks';
import { FileUp, Upload } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useAsync } from '../lib/hooks.js';
import { formatBytes } from '../lib/format.js';
import { Modal } from '../ui/Modal.jsx';
import { AsyncBlock, Button } from '../ui/Primitives.jsx';
import { useToast } from '../ui/Toast.jsx';

/** Mismo techo que `MAX_UPLOAD_BYTES` en src/http.rs. */
const MAX_BYTES = 1024 * 1024 * 1024;

/**
 * Restaura un `.sql` dentro de un contenedor PostgreSQL, MySQL o MariaDB.
 *
 * El archivo se sube tal cual y lo ejecuta el cliente que vive dentro del
 * contenedor (`psql` o `mysql`), igual que la exportación usa el `pg_dump` de
 * dentro. El panel no interpreta el SQL: lo que traiga el archivo es lo que se
 * ejecuta.
 */
export function DatabaseImportDialog({ container, onClose }) {
  const toast = useToast();
  const [schema, setSchema] = useState('');
  const [file, setFile] = useState(null);
  const [busy, setBusy] = useState(false);
  const [output, setOutput] = useState('');
  const picker = useRef(null);

  const info = useAsync(
    () => api.get(`/api/containers/${encodeURIComponent(container)}/import`),
    [container],
  );

  // Con una sola base no hay nada que elegir; con varias, gana la primera.
  useEffect(() => {
    if (info.data?.schemas?.length) setSchema((current) => current || info.data.schemas[0]);
  }, [info.data]);

  const choose = (event) => {
    const chosen = event.currentTarget.files?.[0] || null;
    event.currentTarget.value = '';
    if (chosen && chosen.size > MAX_BYTES) {
      toast.error(`El archivo pesa ${formatBytes(chosen.size)}; el máximo es 1 GB.`);
      return;
    }
    setOutput('');
    setFile(chosen);
  };

  const upload = async () => {
    setBusy(true);
    setOutput('');
    try {
      const result = await api.upload(
        `/api/containers/${encodeURIComponent(container)}/import`,
        file,
        { schema },
      );
      toast.success(`${file.name} importado en ${schema}`);
      // El cliente suele terminar en silencio; solo se pinta si dijo algo.
      if (result?.output) setOutput(result.output);
      else onClose();
    } catch (error) {
      setOutput(error.message || String(error));
      toast.error('La importación falló');
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open
      onClose={onClose}
      eyebrow="IMPORTAR"
      title={container}
      description={
        info.data
          ? `${info.data.database_label} detectado. El archivo lo ejecuta el cliente que hay dentro del contenedor.`
          : 'Comprobando el motor de base de datos…'
      }
      size="lg"
      footer={
        <>
          <Button onClick={onClose}>Cerrar</Button>
          <Button
            variant="primary"
            icon={Upload}
            loading={busy}
            disabled={!info.data || !file || !schema}
            onClick={upload}
          >
            Importar SQL
          </Button>
        </>
      }
    >
      <AsyncBlock query={info}>
        {(data) => (
          <>
            <div class="export-section">
              <span class="field-label">Base de datos de destino</span>
              <div class="export-options">
                {data.schemas.map((option) => (
                  <label key={option} class="exposure-option">
                    <input
                      type="radio"
                      name="import-schema"
                      value={option}
                      checked={schema === option}
                      onChange={() => setSchema(option)}
                    />
                    <span class="exposure-copy">
                      <strong>{option}</strong>
                    </span>
                  </label>
                ))}
              </div>
            </div>

            <div class="export-section">
              <span class="field-label">Archivo</span>
              <div class="import-file">
                <Button size="sm" icon={FileUp} onClick={() => picker.current?.click()}>
                  {file ? 'Elegir otro' : 'Elegir archivo .sql'}
                </Button>
                <span class={file ? 'import-file-name' : 'field-hint'}>
                  {file ? `${file.name} · ${formatBytes(file.size)}` : 'Ningún archivo elegido'}
                </span>
                <input
                  ref={picker}
                  type="file"
                  accept=".sql,.txt,text/plain,application/sql"
                  class="env-file"
                  onChange={choose}
                />
              </div>
            </div>

            {output ? <pre class="logs console-output import-output">{output}</pre> : null}

            <p class="muted small">
              El SQL se ejecuta tal cual sobre <code>{schema || 'la base elegida'}</code>: si el
              volcado trae <code>DROP</code> o <code>CREATE</code>, reemplaza lo que haya.{' '}
              {data.database === 'postgres'
                ? 'PostgreSQL se detiene en el primer error, así que un archivo incompatible puede dejar la restauración a medias.'
                : 'MySQL y MariaDB continúan tras un error, así que revisa la salida al terminar.'}{' '}
              Conviene <strong>exportar antes</strong> para tener vuelta atrás. En archivos grandes
              la importación tarda varios minutos: no cierres la pestaña.
            </p>
          </>
        )}
      </AsyncBlock>
    </Modal>
  );
}
