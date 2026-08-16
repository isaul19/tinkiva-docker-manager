import { useState } from 'preact/hooks';
import { HardDrive, Trash2 } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { useAsync } from '../lib/hooks.js';
import { formatBytes, formatDockerAge } from '../lib/format.js';
import {
  AsyncBlock,
  Badge,
  Button,
  EmptyState,
  Pagination,
  Panel,
} from '../ui/Primitives.jsx';
import { Modal } from '../ui/Modal.jsx';
import { useToast } from '../ui/Toast.jsx';

const PAGE_SIZE = 12;

/** Suma por id único: dos etiquetas de la misma imagen ocupan disco una vez. */
function diskTotals(images) {
  const seen = new Map();
  for (const image of images) {
    if (!seen.has(image.id)) seen.set(image.id, image);
  }
  let total = 0;
  let reclaimable = 0;
  for (const image of seen.values()) {
    total += image.size_bytes || 0;
    if (!image.in_use) reclaimable += image.size_bytes || 0;
  }
  return { total, reclaimable };
}

/**
 * Imágenes locales del host: cuánto ocupan y cuáles se pueden borrar. Solo se
 * permite eliminar las que ningún contenedor usa, ni siquiera detenido: borrar
 * la imagen de un contenedor parado lo dejaría sin poder arrancar.
 */
export function Images() {
  const { refreshToken } = useApp();
  const toast = useToast();
  const [confirming, setConfirming] = useState(null);
  const [busy, setBusy] = useState(false);
  const [page, setPage] = useState(0);
  const [onlyUnused, setOnlyUnused] = useState(false);

  const images = useAsync(() => api.get('/api/images'), [refreshToken]);

  const remove = async () => {
    setBusy(true);
    try {
      const result = await api.del('/api/images', { reference: confirming.reference });
      toast.success(result.message || 'Imagen eliminada');
      setConfirming(null);
      images.reload();
    } catch (error) {
      toast.error(error);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <p class="page-intro muted">
        Imágenes descargadas o construidas en este host. Las que no usa ningún contenedor pueden
        borrarse para recuperar disco.
      </p>

      <Panel class="table-panel">
        <AsyncBlock
          query={images}
          empty={
            <EmptyState
              icon={HardDrive}
              title="No hay imágenes"
              description="Docker no tiene ninguna imagen descargada en este host."
            />
          }
        >
          {(all) => {
            // Varias etiquetas pueden compartir capas: el total se calcula por
            // id único, si no una imagen con dos tags contaría el doble.
            const totals = diskTotals(all);
            const unused = all.filter((image) => !image.in_use).length;
            const list = onlyUnused ? all.filter((image) => !image.in_use) : all;
            const lastPage = Math.max(0, Math.ceil(list.length / PAGE_SIZE) - 1);
            const currentPage = Math.min(page, lastPage);
            const visible = list.slice(currentPage * PAGE_SIZE, (currentPage + 1) * PAGE_SIZE);
            return (
              <>
                <div class="table-summary">
                  <p class="muted small">
                    {all.length} {all.length === 1 ? 'imagen' : 'imágenes'} ·{' '}
                    <strong>{formatBytes(totals.total)}</strong> en disco ·{' '}
                    {unused} sin usar ({formatBytes(totals.reclaimable)} recuperables)
                  </p>
                  <label class="inline-field checkbox">
                    <input
                      type="checkbox"
                      checked={onlyUnused}
                      onChange={(event) => {
                        setOnlyUnused(event.currentTarget.checked);
                        setPage(0);
                      }}
                    />
                    <span>Solo sin usar</span>
                  </label>
                </div>
                <div class="table-scroll">
                  <table class="data-table">
                    <thead>
                      <tr>
                        <th>Imagen</th>
                        <th>ID</th>
                        <th>Tamaño</th>
                        <th>Creada</th>
                        <th>Estado</th>
                        <th class="align-end">Acciones</th>
                      </tr>
                    </thead>
                    <tbody>
                      {visible.map((image) => (
                        <tr key={image.id + image.reference}>
                          <td>
                            <div class="cell-stack">
                              <strong>{image.repository === '<none>' ? 'Sin etiqueta' : image.repository}</strong>
                              <span class="muted mono small">
                                {image.tag === '<none>' ? 'dangling' : image.tag}
                              </span>
                            </div>
                          </td>
                          <td class="muted mono small">{image.id}</td>
                          <td class="numeric">
                            {image.size_bytes ? formatBytes(image.size_bytes) : image.size || '—'}
                          </td>
                          <td class="muted small">{formatDockerAge(image.created_since)}</td>
                          <td>
                            {image.in_use ? (
                              <>
                                <Badge tone="ok">En uso</Badge>
                                <span class="muted block small">{image.containers.join(', ')}</span>
                              </>
                            ) : (
                              <Badge tone="neutral">Sin usar</Badge>
                            )}
                          </td>
                          <td class="align-end">
                            <Button
                              size="sm"
                              icon={Trash2}
                              disabled={image.in_use}
                              title={
                                image.in_use
                                  ? 'La usan contenedores existentes'
                                  : `Borrar ${image.reference}`
                              }
                              onClick={() => setConfirming(image)}
                            >
                              Borrar
                            </Button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                <Pagination
                  page={currentPage}
                  total={list.length}
                  pageSize={PAGE_SIZE}
                  onPageChange={setPage}
                />
              </>
            );
          }}
        </AsyncBlock>
      </Panel>

      {confirming ? (
        <Modal
          open
          onClose={() => setConfirming(null)}
          eyebrow="BORRAR IMAGEN"
          title={confirming.reference}
          description="Se elimina del disco del servidor. Puedes volver a descargarla o reconstruirla más adelante."
          footer={
            <>
              <Button onClick={() => setConfirming(null)}>Cancelar</Button>
              <Button variant="danger" icon={Trash2} loading={busy} onClick={remove}>
                Borrar imagen
              </Button>
            </>
          }
        >
          <p class="muted small">
            Libera hasta{' '}
            <strong>
              {confirming.size_bytes ? formatBytes(confirming.size_bytes) : confirming.size}
            </strong>
            , menos las capas que comparta con otras imágenes. Ningún contenedor la está usando
            ahora mismo; el panel lo vuelve a comprobar antes de borrarla.
          </p>
        </Modal>
      ) : null}
    </>
  );
}
