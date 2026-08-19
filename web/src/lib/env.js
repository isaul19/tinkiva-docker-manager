// Lógica del editor de variables de entorno: parseo, serialización y las
// mismas validaciones que aplica el backend (`parse_environment` en
// src/app.rs y `valid_env_key` en src/util.rs). Vive aparte de la interfaz
// para poder probarla sin DOM, y para que un cambio de reglas en Rust tenga un
// único sitio que actualizar aquí.

export const MAX_VARIABLES = 100;
/** Las escribe el panel en el .env generado; el backend rechaza reutilizarlas. */
export const RESERVED_KEYS = ['APP_IMAGE', 'TDM_MEMORY_LIMIT'];
export const MAX_VALUE_LENGTH = 4096;

/** Claves válidas: mayúsculas, dígitos y `_`, sin empezar por dígito. */
const VALID_KEY = /^[A-Z_][A-Z0-9_]*$/;

/** Una línea que «parece» una asignación de .env, aunque la clave sea inválida. */
const ASSIGNMENT = /^\s*(?:export\s+)?[A-Za-z_][A-Za-z0-9_]*\s*=/;

let sequence = 0;

/** Fila del editor. El `id` sobrevive a reordenaciones y evita que Preact
 *  reutilice el input equivocado al insertar filas en medio. */
export function envRow(key = '', value = '') {
  sequence += 1;
  return { id: sequence, key, value };
}

/**
 * Convierte texto de .env en filas. Es deliberadamente tolerante: acepta
 * `export CLAVE=valor`, ignora comentarios y líneas en blanco, y una línea sin
 * `=` se queda como clave suelta para que el usuario vea qué pegó en vez de
 * perderlo en silencio.
 */
export function parseEnvText(raw) {
  const rows = [];
  for (const line of String(raw ?? '').split(/\r?\n/)) {
    const trimmed = line.trim().replace(/^export\s+/, '');
    if (!trimmed || trimmed.startsWith('#')) continue;
    const cut = trimmed.indexOf('=');
    if (cut === -1) rows.push(envRow(trimmed, ''));
    else rows.push(envRow(trimmed.slice(0, cut).trim(), trimmed.slice(cut + 1).trim()));
  }
  return rows;
}

/** Serializa a `CLAVE=valor` por línea, que es lo que espera el endpoint. */
export function serializeEnvText(rows) {
  return rows
    .filter((row) => row.key.trim() || row.value.trim())
    .map((row) => `${row.key.trim()}=${row.value.trim()}`)
    .join('\n');
}

/**
 * Un error por fila (o `null`). Las filas totalmente vacías no molestan: son el
 * hueco en blanco que el editor deja siempre al final.
 */
export function validateEnvRows(rows, reserved = []) {
  const seen = new Set();
  return rows.map((row) => {
    const key = row.key.trim();
    const value = row.value.trim();
    if (!key && !value) return null;
    if (!key) return 'Falta la clave.';
    if (!VALID_KEY.test(key)) return 'Solo MAYÚSCULAS, dígitos y guion bajo.';
    if (reserved.includes(key)) return `${key} la gestiona el panel.`;
    if (value.length > MAX_VALUE_LENGTH) return `El valor supera ${MAX_VALUE_LENGTH} caracteres.`;
    if (seen.has(key)) return 'Clave repetida.';
    seen.add(key);
    return null;
  });
}

/**
 * Decide si un pegado debe repartirse en filas.
 *
 * En el campo de la clave basta con que haya una asignación: pegar
 * `CLAVE=valor` ahí siempre significa «rellena los dos campos». En el campo del
 * valor hay que ser mucho más estricto, porque un ARN de KMS, un token o un
 * base64 llevan `=` y deben pegarse tal cual: solo se reparte un bloque de
 * varias líneas en el que *todas* son asignaciones.
 */
export function shouldExplodePaste(text, field) {
  const lines = String(text ?? '')
    .split(/\r?\n/)
    .filter((line) => line.trim() && !line.trim().startsWith('#'));
  if (!lines.length) return false;
  const assignments = lines.filter((line) => ASSIGNMENT.test(line)).length;
  if (field === 'key') return assignments >= 1;
  return lines.length >= 2 && assignments === lines.length;
}

/**
 * Une filas dejando una sola por clave. Gana el valor más reciente: quien pega
 * su .env encima del que ya tenía espera que se actualicen los valores, no
 * acabar con la clave duplicada y un error al guardar.
 */
export function mergeEnvRows(rows) {
  const merged = [];
  const position = new Map();
  for (const row of rows) {
    const key = row.key.trim();
    if (key && position.has(key)) {
      const at = position.get(key);
      merged[at] = { ...merged[at], value: row.value };
      continue;
    }
    if (key) position.set(key, merged.length);
    merged.push(row);
  }
  return merged;
}

/** Inserta un bloque pegado en la fila `index`: la sustituye si estaba vacía. */
export function insertEnvRows(rows, index, incoming) {
  const next = rows.slice();
  const current = next[index];
  const blank = current && !current.key.trim() && !current.value.trim();
  next.splice(blank ? index : index + 1, blank ? 1 : 0, ...incoming);
  return mergeEnvRows(next);
}
