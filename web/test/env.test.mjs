// Pruebas del editor de variables: parseo, serialización y las reglas que
// deciden cuándo un pegado se reparte en filas. Se ejecutan contra el módulo
// fuente (es JS puro, sin JSX) y no necesitan DOM.
//
// Ejecutar con: npm test

import { strict as assert } from 'node:assert';
import {
  RESERVED_KEYS,
  insertEnvRows,
  mergeEnvRows,
  parseEnvText,
  serializeEnvText,
  shouldExplodePaste,
  validateEnvRows,
} from '../src/lib/env.js';

const tests = [];
const test = (name, body) => tests.push([name, body]);
const pairs = (rows) => rows.map((row) => [row.key, row.value]);

test('parsea un .env con comentarios, huecos y export', () => {
  const rows = parseEnvText('# correo\nMAIL_ENABLED=true\n\nexport PORT=3000\n');
  assert.deepEqual(pairs(rows), [
    ['MAIL_ENABLED', 'true'],
    ['PORT', '3000'],
  ]);
});

test('el valor conserva los = que vengan dentro', () => {
  const rows = parseEnvText('KMS_KEY_ID=arn:aws:kms:us-east-1:1234:key/29=abc');
  assert.deepEqual(pairs(rows), [['KMS_KEY_ID', 'arn:aws:kms:us-east-1:1234:key/29=abc']]);
});

test('serializar deja fuera las filas en blanco', () => {
  const rows = [...parseEnvText('A=1'), { id: 99, key: '', value: '' }];
  assert.equal(serializeEnvText(rows), 'A=1');
});

test('ida y vuelta de un bloque real', () => {
  const source = 'MAIL_ENABLED=true\nSES_FROM_EMAIL=no-reply@tinkiva.com';
  assert.equal(serializeEnvText(parseEnvText(source)), source);
});

test('valida las mismas reglas que el backend', () => {
  const rows = parseEnvText('ok=1\nBIEN=1\nBIEN=2\nTDM_MEMORY_LIMIT=512m');
  const errors = validateEnvRows(rows, RESERVED_KEYS);
  assert.match(errors[0], /MAY/, 'una clave en minúsculas es inválida');
  assert.equal(errors[1], null);
  assert.match(errors[2], /repetida/);
  assert.match(errors[3], /gestiona el panel/);
});

test('una fila totalmente vacía no da error', () => {
  assert.deepEqual(validateEnvRows([{ id: 1, key: '', value: '' }]), [null]);
});

test('pegar un bloque en la clave lo reparte', () => {
  const block = 'MAIL_ENABLED=true\nSES_FROM_EMAIL=no-reply@tinkiva.com\nSES_CONFIGURATION_SET=x';
  assert.equal(shouldExplodePaste(block, 'key'), true);
  assert.equal(parseEnvText(block).length, 3);
});

test('pegar CLAVE=valor en la clave rellena las dos columnas', () => {
  assert.equal(shouldExplodePaste('PORT=3000', 'key'), true);
  assert.deepEqual(pairs(parseEnvText('PORT=3000')), [['PORT', '3000']]);
});

test('un secreto de una línea se pega tal cual en el valor', () => {
  // Lo que más duele: un ARN o un token con «=» no debe convertirse en filas.
  assert.equal(shouldExplodePaste('arn:aws:kms:us-east-1:160:key/29=abc', 'value'), false);
  assert.equal(shouldExplodePaste('tS2qzObNj__Of1GD3IJbAE6CVkWvO7qFHOxcwm=', 'value'), false);
});

test('un bloque entero sí se reparte desde el valor', () => {
  assert.equal(shouldExplodePaste('A=1\nB=2', 'value'), true);
  // Basta con que una línea no sea asignación para no tocar el pegado.
  assert.equal(shouldExplodePaste('-----BEGIN KEY-----\nAAAA=\n', 'value'), false);
});

test('al pegar sobre una fila vacía la sustituye', () => {
  const rows = [...parseEnvText('A=1'), { id: 50, key: '', value: '' }];
  const next = insertEnvRows(rows, 1, parseEnvText('B=2\nC=3'));
  assert.deepEqual(pairs(next), [
    ['A', '1'],
    ['B', '2'],
    ['C', '3'],
  ]);
});

test('al pegar sobre una fila con contenido inserta debajo', () => {
  const next = insertEnvRows(parseEnvText('A=1\nZ=9'), 0, parseEnvText('B=2'));
  assert.deepEqual(pairs(next), [
    ['A', '1'],
    ['B', '2'],
    ['Z', '9'],
  ]);
});

test('una clave repetida se actualiza en su sitio', () => {
  const next = mergeEnvRows(parseEnvText('A=1\nB=2\nA=nuevo'));
  assert.deepEqual(pairs(next), [
    ['A', 'nuevo'],
    ['B', '2'],
  ]);
});

let failures = 0;
for (const [name, body] of tests) {
  try {
    await body();
    console.log(`  ok   ${name}`);
  } catch (error) {
    failures += 1;
    console.error(`  FALLO ${name}`);
    console.error(`       ${error.message}`);
  }
}

console.log(`\n${tests.length - failures}/${tests.length} pruebas del editor de entorno correctas`);
process.exit(failures ? 1 : 0);
