import { useEffect, useRef, useState } from 'preact/hooks';
import { Code, Eye, EyeOff, FileUp, Plus, Rows3, Trash2 } from 'lucide-preact';
import {
  MAX_VARIABLES,
  envRow,
  insertEnvRows,
  mergeEnvRows,
  parseEnvText,
  serializeEnvText,
  shouldExplodePaste,
  validateEnvRows,
} from '../lib/env.js';
import { Button } from './Primitives.jsx';

/**
 * Editor de variables de entorno en filas clave/valor.
 *
 * Hacia fuera sigue hablando el mismo idioma que el resto del panel —un texto
 * `CLAVE=valor` por línea—, así que sustituye a un `<TextArea>` sin tocar el
 * formulario que lo usa ni el endpoint.
 *
 * Pegar el .env de tu proyecto en cualquier campo de clave lo reparte en filas;
 * el modo texto sigue disponible para quien prefiera editar el bloque entero.
 */
export function EnvEditor({
  value,
  onChange,
  reserved = [],
  label = 'Variables de entorno',
  hint,
  wide = false,
}) {
  const [rows, setRows] = useState(() => withBlank(parseEnvText(value)));
  const [raw, setRaw] = useState(false);
  const [masked, setMasked] = useState(false);
  const emitted = useRef(value ?? '');
  const file = useRef(null);

  // El valor puede cambiar desde fuera: al abrir el diálogo con otro recurso o
  // al resetear el formulario. Solo se re-parsea cuando no es nuestro propio
  // eco, o cada pulsación reconstruiría las filas y perdería el foco.
  useEffect(() => {
    const incoming = value ?? '';
    if (incoming === emitted.current) return;
    emitted.current = incoming;
    setRows(withBlank(parseEnvText(incoming)));
  }, [value]);

  const emit = (next) => {
    const filled = withBlank(next);
    setRows(filled);
    const text = serializeEnvText(filled);
    emitted.current = text;
    onChange(text);
  };

  const editRow = (index, field) => (event) => {
    const next = rows.slice();
    next[index] = { ...next[index], [field]: event.currentTarget.value };
    emit(next);
  };

  const removeRow = (index) => () => emit(rows.filter((_, position) => position !== index));

  const pasteRow = (index, field) => (event) => {
    const text = event.clipboardData?.getData('text') ?? '';
    if (!shouldExplodePaste(text, field)) return;
    const incoming = parseEnvText(text);
    if (!incoming.length) return;
    event.preventDefault();
    emit(insertEnvRows(rows, index, incoming));
  };

  const importFile = async (event) => {
    const chosen = event.currentTarget.files?.[0];
    // Se limpia el input para poder reimportar el mismo archivo tras editarlo.
    event.currentTarget.value = '';
    if (!chosen) return;
    const incoming = parseEnvText(await chosen.text());
    if (!incoming.length) return;
    emit(mergeEnvRows([...rows.filter(isFilled), ...incoming]));
  };

  const editRaw = (event) => {
    const text = event.currentTarget.value;
    emitted.current = text;
    onChange(text);
  };

  const toggleRaw = () => {
    if (raw) setRows(withBlank(parseEnvText(value)));
    setRaw((on) => !on);
  };

  const errors = validateEnvRows(rows, reserved);
  const count = rows.filter(isFilled).length;

  return (
    <div class={`env-editor${wide ? ' field-wide' : ''}`}>
      <div class="env-head">
        <span class="field-label">{label}</span>
        <span class={`env-count${count > MAX_VARIABLES ? ' is-over' : ''}`}>
          {count} de {MAX_VARIABLES}
        </span>
      </div>

      {raw ? (
        <textarea
          class="input textarea"
          rows={10}
          spellcheck={false}
          placeholder={'NODE_ENV=production\nPORT=3000'}
          value={value}
          onInput={editRaw}
        />
      ) : (
        <div class="env-rows">
          <div class="env-row env-row-head">
            <span>Clave</span>
            <span>Valor</span>
            <span />
          </div>
          {rows.map((row, index) => (
            <div class={`env-row${errors[index] ? ' is-invalid' : ''}`} key={row.id}>
              <input
                class="input env-key"
                spellcheck={false}
                autocomplete="off"
                aria-label="Clave"
                placeholder={index === 0 ? 'NODE_ENV' : 'CLAVE'}
                value={row.key}
                onInput={editRow(index, 'key')}
                onPaste={pasteRow(index, 'key')}
              />
              <input
                class="input env-value"
                type={masked ? 'password' : 'text'}
                spellcheck={false}
                autocomplete="off"
                aria-label="Valor"
                placeholder={index === 0 ? 'production' : 'valor'}
                value={row.value}
                onInput={editRow(index, 'value')}
                onPaste={pasteRow(index, 'value')}
              />
              <button
                type="button"
                class="icon-button env-remove"
                aria-label={`Eliminar ${row.key || 'variable'}`}
                onClick={removeRow(index)}
              >
                <Trash2 size={15} />
              </button>
              {errors[index] ? <span class="env-error">{errors[index]}</span> : null}
            </div>
          ))}
        </div>
      )}

      <div class="env-actions">
        {!raw ? (
          <Button size="sm" icon={Plus} onClick={() => emit([...rows, envRow()])}>
            Añadir variable
          </Button>
        ) : null}
        <Button size="sm" icon={FileUp} onClick={() => file.current?.click()}>
          Importar .env
        </Button>
        <Button size="sm" icon={raw ? Rows3 : Code} onClick={toggleRaw}>
          {raw ? 'Editar en filas' : 'Editar como texto'}
        </Button>
        {!raw ? (
          <Button size="sm" icon={masked ? Eye : EyeOff} onClick={() => setMasked((on) => !on)}>
            {masked ? 'Mostrar valores' : 'Ocultar valores'}
          </Button>
        ) : null}
        <input
          ref={file}
          type="file"
          accept=".env,.txt,text/plain"
          class="env-file"
          onChange={importFile}
        />
      </div>

      <span class="field-hint">
        {hint ? `${hint} ` : ''}
        Pega tu .env completo en cualquier campo de clave y se reparte en filas.
      </span>
      {count > MAX_VARIABLES ? (
        <span class="field-message">
          El panel guarda como máximo {MAX_VARIABLES} variables; quita algunas antes de guardar.
        </span>
      ) : null}
    </div>
  );
}

const isFilled = (row) => Boolean(row.key.trim() || row.value.trim());

/** El editor nunca se queda sin filas: siempre hay un hueco donde escribir. */
function withBlank(rows) {
  return rows.length ? rows : [envRow()];
}
