import { useEffect, useState } from 'preact/hooks';
import { Download } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useAsync } from '../lib/hooks.js';
import { fileTimestamp, saveBlob } from '../lib/download.js';
import { Modal } from '../ui/Modal.jsx';
import { AsyncBlock, Button } from '../ui/Primitives.jsx';
import { useToast } from '../ui/Toast.jsx';

const MODES = [
  {
    value: 'all',
    title: 'Datos y estructura',
    hint: 'Volcado completo: CREATE, filas, procedimientos, funciones y triggers.',
  },
  {
    value: 'structure',
    title: 'Solo estructura',
    hint: 'CREATE de tablas y vistas, procedimientos, funciones y triggers. Sin filas.',
  },
  {
    value: 'data',
    title: 'Solo datos',
    hint: 'Solo las filas, como INSERT. El destino ya debe tener la estructura creada.',
  },
];

/**
 * Exporta a `.sql` las bases de datos de un contenedor PostgreSQL, MySQL o
 * MariaDB. El volcado lo genera el cliente que vive dentro del contenedor
 * (`pg_dump` o `mysqldump`); el panel solo lo transmite.
 */
export function DatabaseExportDialog({ container, onClose }) {
  const toast = useToast();
  const [selected, setSelected] = useState([]);
  const [mode, setMode] = useState('all');
  const [busy, setBusy] = useState(false);

  const info = useAsync(
    () => api.get(`/api/containers/${encodeURIComponent(container)}/export`),
    [container],
  );

  // Por defecto se exporta todo lo que el motor haya reportado.
  useEffect(() => {
    if (info.data?.schemas) setSelected(info.data.schemas);
  }, [info.data]);

  const toggle = (schema) =>
    setSelected((current) =>
      current.includes(schema)
        ? current.filter((value) => value !== schema)
        : [...current, schema],
    );

  const download = async () => {
    setBusy(true);
    try {
      const blob = await api.download(`/api/containers/${encodeURIComponent(container)}/export`, {
        mode,
        schemas: selected.join(','),
      });
      saveBlob(blob, `${container}_${fileTimestamp()}.sql`);
      toast.success(`Exportación de ${container} descargada`);
      onClose();
    } catch (error) {
      toast.error(error);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open
      onClose={onClose}
      eyebrow="EXPORTAR"
      title={container}
      description={
        info.data
          ? `${info.data.database_label} detectado. El archivo se descarga como ${container}_AAAAMMDDHHMMSS.sql`
          : 'Comprobando el motor de base de datos…'
      }
      size="lg"
      footer={
        <>
          <Button onClick={onClose}>Cancelar</Button>
          <Button
            variant="primary"
            icon={Download}
            loading={busy}
            disabled={!info.data || selected.length === 0}
            onClick={download}
          >
            Descargar SQL
          </Button>
        </>
      }
    >
      <AsyncBlock query={info}>
        {(data) => (
          <>
            <div class="export-section">
              <div class="export-head">
                <span class="field-label">Bases de datos</span>
                <Button
                  size="sm"
                  onClick={() =>
                    setSelected(selected.length === data.schemas.length ? [] : data.schemas)
                  }
                >
                  {selected.length === data.schemas.length ? 'Quitar todas' : 'Seleccionar todas'}
                </Button>
              </div>
              <div class="export-options">
                {data.schemas.map((schema) => (
                  <label key={schema} class="exposure-option">
                    <input
                      type="checkbox"
                      checked={selected.includes(schema)}
                      onChange={() => toggle(schema)}
                    />
                    <span class="exposure-copy">
                      <strong>{schema}</strong>
                    </span>
                  </label>
                ))}
              </div>
            </div>

            <div class="export-section">
              <span class="field-label">Contenido</span>
              <div class="export-options">
                {MODES.map((option) => (
                  <label key={option.value} class="exposure-option">
                    <input
                      type="radio"
                      name="export-mode"
                      value={option.value}
                      checked={mode === option.value}
                      onChange={() => setMode(option.value)}
                    />
                    <span class="exposure-copy">
                      <strong>{option.title}</strong>
                      <span class="field-hint">{option.hint}</span>
                    </span>
                  </label>
                ))}
              </div>
            </div>

            <p class="muted small">
              Las filas se escriben como <code>INSERT INTO tabla (columnas) VALUES …</code>, con las
              columnas nombradas. El volcado se genera dentro del contenedor y se transmite sin
              guardarse en el panel: en bases grandes puede tardar varios minutos, no cierres la
              pestaña.
            </p>
          </>
        )}
      </AsyncBlock>
    </Modal>
  );
}
