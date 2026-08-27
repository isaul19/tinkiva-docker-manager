import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { strict as assert } from 'node:assert';
import { parseHTML } from 'linkedom';

const here = dirname(fileURLToPath(import.meta.url));
const bundle = readFileSync(resolve(here, '../dist/app.js'), 'utf8');
const API = {
  '/api/info': {
    name: 'TinkivaCreateApp Monitor', version: '0.15.0', edition: 'createapp', mode: 'read-only',
    started_at: 1_700_000_000, data_dir: '/var/lib/tinkiva-docker-manager', workers: 2,
    docker: { available: true, server_version: '27.5.1', error: null },
  },
  '/api/system': {
    hostname: 'vps-1', cpu_percent: 26.7, cpu_threads: 2, load_1: 0.01, load_5: 0.02,
    load_15: 0.03, memory_total: 1_932_735_283, memory_used: 920_000_000,
    memory_available: 1_012_735_283, swap_total: 0, swap_used: 0,
    disk_total: 32_212_254_720, disk_used: 7_944_890_000, disk_available: 24_267_364_720,
    disk_percent: 24.66, uptime_seconds: 186_000, process_rss: 786_432,
  },
  '/api/containers': [
    { id: 'abc', name: 'app-postgres', image: 'postgres:17', status: 'Up 10 minutes (healthy)', state: 'running', ports: '5432/tcp', created_at: 'today', cpu: '0.20%', memory: '124MiB / 1GiB', memory_percent: '12.4%', network_io: '1kB / 2kB', block_io: '0B / 0B', pids: '12' },
    { id: 'def', name: 'app-backend', image: 'ghcr.io/tinkiva/app:one', status: 'Exited (1)', state: 'exited', ports: '', created_at: 'today', cpu: '0.00%', memory: '0B / 0B', memory_percent: '0.00%', network_io: '0B / 0B', block_io: '0B / 0B', pids: '0' },
  ],
  '/api/containers/app-postgres/logs': '2026-08-27T12:00:00Z database system is ready',
};

async function mount({ token = null, hash = '#/dashboard' } = {}) {
  const { window, document } = parseHTML('<!doctype html><html><body><div id="root"></div></body></html>');
  const store = new Map();
  if (token) store.set('tdm-token', token);
  const sessionStorage = { getItem: (key) => store.get(key) ?? null, setItem: (key, value) => store.set(key, String(value)), removeItem: (key) => store.delete(key) };
  const calls = [];
  const fetchStub = async (path) => {
    calls.push(String(path));
    const [base] = String(path).split('?');
    const body = API[base];
    if (body === undefined) return { ok: false, status: 404, text: async () => JSON.stringify({ error: `sin mock para ${base}` }) };
    return { ok: true, status: 200, text: async () => typeof body === 'string' ? body : JSON.stringify(body) };
  };
  Object.assign(window, {
    sessionStorage, fetch: fetchStub,
    location: { hash, search: '', pathname: '/', origin: 'http://127.0.0.1:8787' },
    history: { replaceState: () => {} },
    matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }),
  });
  document.visibilityState = 'visible';
  const globals = {
    window, document, sessionStorage, fetch: fetchStub, location: window.location,
    navigator: { clipboard: { writeText: async () => {} } },
    requestAnimationFrame: (callback) => setTimeout(callback, 0), cancelAnimationFrame: clearTimeout,
    addEventListener: () => {}, removeEventListener: () => {}, Node: window.Node,
    Element: window.Element, Text: window.Text,
  };
  const saved = {};
  const install = () => { for (const [key, value] of Object.entries(globals)) { saved[key] = globalThis[key]; Object.defineProperty(globalThis, key, { value, configurable: true, writable: true }); } };
  const restore = () => { for (const [key, value] of Object.entries(saved)) { if (value === undefined) delete globalThis[key]; else Object.defineProperty(globalThis, key, { value, configurable: true, writable: true }); } };
  const flush = async () => { for (let index = 0; index < 12; index += 1) await new Promise((done) => setTimeout(done, 0)); };
  install();
  try { new Function(bundle)(); await flush(); } finally { restore(); }
  const find = (selector, needle) => [...document.querySelectorAll(selector)].find((node) => node.textContent.includes(needle) || (node.getAttribute('aria-label') || '').includes(needle));
  const click = async (selector, needle) => {
    const target = find(selector, needle); assert.ok(target, `no se encontró «${needle}»`);
    install(); try { target.dispatchEvent(new window.Event('click', { bubbles: true })); await flush(); } finally { restore(); }
  };
  return { text: () => document.getElementById('root').textContent, html: document.getElementById('root').innerHTML, calls, click, find, document };
}

const tests = [];
const test = (name, body) => tests.push([name, body]);

test('sin token muestra el acceso protegido', async () => {
  const app = await mount();
  assert.match(app.text(), /Docker Manager/);
  assert.match(app.text(), /Usuario/);
  assert.match(app.text(), /Contraseña/);
  assert.match(app.html, /brand-icon/);
});

test('el resumen muestra CPU, RAM, disco y estado Docker', async () => {
  const app = await mount({ token: 'x'.repeat(40) });
  assert.ok(app.calls.includes('/api/info'));
  assert.ok(app.calls.includes('/api/system'));
  assert.ok(app.calls.includes('/api/containers'));
  assert.match(app.text(), /26\.7%/);
  assert.match(app.text(), /Disco raíz/);
  assert.match(app.text(), /app-postgres/);
  assert.match(app.text(), /1 requieren atención/);
  assert.match(app.text(), /modo de solo lectura/i);
});

test('la edición panel no ofrece CI/CD ni acciones sobre Docker', async () => {
  const app = await mount({ token: 'x'.repeat(40) });
  const text = app.text();
  for (const removed of ['Añadir recurso', 'Despliegues', 'GitHub', 'Amazon ECR', 'Rollback', 'Reiniciar', 'Detener']) {
    assert.ok(!text.includes(removed), `no debe aparecer ${removed}`);
  }
  assert.ok(!app.calls.some((path) => /catalog|projects|history|github|ecr|images/.test(path)));
});

test('contenedores conserva métricas y lectura de logs', async () => {
  const app = await mount({ token: 'x'.repeat(40), hash: '#/containers' });
  assert.match(app.text(), /app-postgres/);
  assert.match(app.text(), /postgres:17/);
  assert.match(app.text(), /0\.20%/);
  assert.match(app.text(), /124MiB/);
  await app.click('button', 'Ver logs');
  assert.ok(app.calls.some((path) => path.startsWith('/api/containers/app-postgres/logs')));
  assert.match(app.text(), /database system is ready/);
});

test('sistema identifica la edición estable de solo lectura', async () => {
  const app = await mount({ token: 'x'.repeat(40), hash: '#/system' });
  assert.match(app.text(), /vps-1/);
  assert.match(app.text(), /EdiciónTinkivaCreateApp/);
  assert.match(app.text(), /ModoSolo lectura/);
  assert.match(app.text(), /Docker.*27\.5\.1/is);
});

let failures = 0;
for (const [name, body] of tests) {
  try { await body(); console.log(`  ok   ${name}`); }
  catch (error) { failures += 1; console.error(`  FALLO ${name}`); console.error(`       ${error.message}`); }
}
console.log(`\n${tests.length - failures}/${tests.length} pruebas de interfaz correctas`);
process.exit(failures ? 1 : 0);
