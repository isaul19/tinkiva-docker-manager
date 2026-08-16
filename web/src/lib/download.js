// Descargas iniciadas desde el panel. El token viaja en la cabecera
// Authorization, así que no sirve un enlace directo: hay que pedir el archivo
// por fetch y entregárselo al navegador desde un Blob.

/** `20260815143052` con la hora local del navegador, no la del servidor. */
export function fileTimestamp(date = new Date()) {
  const pad = (value) => String(value).padStart(2, '0');
  return [
    date.getFullYear(),
    pad(date.getMonth() + 1),
    pad(date.getDate()),
    pad(date.getHours()),
    pad(date.getMinutes()),
    pad(date.getSeconds()),
  ].join('');
}

export function saveBlob(blob, filename) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  // El objeto se libera en cuanto el navegador ha tomado el Blob.
  setTimeout(() => URL.revokeObjectURL(url), 0);
}
