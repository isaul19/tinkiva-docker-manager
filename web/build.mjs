// Empaqueta la interfaz Preact en web/dist/{app.js,app.css}.
//
// El resultado se commitea en el repositorio a propósito: `cargo build` los
// embebe con `include_str!`, así que compilar el panel nunca requiere Node.
// Ejecuta `npm run build` solo cuando toques algo dentro de web/src.

import * as esbuild from 'esbuild';
import { gzipSync } from 'node:zlib';
import { readFileSync, statSync } from 'node:fs';

const watch = process.argv.includes('--watch');

/** @type {import('esbuild').BuildOptions} */
const options = {
  entryPoints: ['src/main.jsx'],
  outdir: 'dist',
  entryNames: 'app',
  bundle: true,
  format: 'iife',
  target: ['es2021'],
  jsx: 'automatic',
  jsxImportSource: 'preact',
  minify: !watch,
  sourcemap: false,
  legalComments: 'none',
  charset: 'utf8',
  // El panel se sirve con CSP `script-src 'self'`: nada de eval ni CDNs.
  supported: { 'dynamic-import': false },
  logLevel: 'info',
};

function report() {
  for (const file of ['dist/app.js', 'dist/app.css']) {
    const raw = statSync(file).size;
    const gzip = gzipSync(readFileSync(file)).length;
    const kb = (value) => `${(value / 1024).toFixed(1)} KB`;
    console.log(`  ${file.padEnd(14)} ${kb(raw).padStart(9)}  (${kb(gzip)} gzip)`);
  }
}

if (watch) {
  const context = await esbuild.context(options);
  await context.watch();
  console.log('Observando web/src…');
} else {
  await esbuild.build(options);
  report();
}
