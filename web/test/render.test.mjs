// Prueba de humo de la interfaz: monta el bundle real de web/dist en un DOM de
// linkedom con `fetch` simulado y comprueba que cada vista pinta lo esperado.
//
// No sustituye a una prueba en navegador, pero sí detecta lo que más duele en un
// bundle sin tipos: importaciones inexistentes, hooks mal usados y vistas que
// revientan al recibir los datos de la API.
//
// Ejecutar con: npm test

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { strict as assert } from 'node:assert';
import { parseHTML } from 'linkedom';

const here = dirname(fileURLToPath(import.meta.url));
const bundle = readFileSync(resolve(here, '../dist/app.js'), 'utf8');

const API = {
  '/api/info': {
    name: 'Tinkiva Docker Manager',
    version: '0.2.0',
    started_at: 1_700_000_000,
    allowed_root: '/opt/tinkiva/apps',
    data_dir: '/var/lib/tinkiva-docker-manager',
    workers: 2,
    projects: 2,
    deployments: 3,
    capabilities: { curl: true, openssl: true, git: true },
    github_connected: false,
    docker: {
      available: true,
      server_version: '27.5.1',
      compose_version: 'v2.32.4',
      error: null,
    },
  },
  '/api/catalog': {
    engines: [
      {
        id: 'postgres',
        label: 'PostgreSQL',
        image: 'postgres:17-alpine',
        port: 5432,
        scheme: 'postgresql',
        needs_database: true,
        needs_username: true,
        default_memory_mb: 512,
        icon: 'postgresql',
        accent: '#4169e1',
        description: 'Relacional',
      },
      {
        id: 'redis',
        label: 'Redis',
        image: 'redis:7-alpine',
        port: 6379,
        scheme: 'redis',
        needs_database: false,
        needs_username: false,
        default_memory_mb: 256,
        icon: 'redis',
        accent: '#ff4438',
        description: 'Caché',
      },
    ],
    popular_images: [
      { name: 'nginx', icon: 'nginx', description: 'Servidor web', official: true },
    ],
    capabilities: { curl: true, openssl: true, git: true },
    allowed_root: '/opt/tinkiva/apps',
  },
  '/api/system': {
    hostname: 'vps-1',
    cpu_percent: 26.7,
    cpu_threads: 2,
    load_1: 0.01,
    load_5: 0.02,
    load_15: 0.03,
    memory_total: 1_932_735_283,
    memory_used: 920_000_000,
    memory_available: 1_012_735_283,
    swap_total: 0,
    swap_used: 0,
    disk_total: 32_212_254_720,
    disk_used: 7_944_890_000,
    disk_available: 24_267_364_720,
    disk_percent: 24.66,
    uptime_seconds: 186_000,
    process_rss: 786_432,
  },
  '/api/containers': [
    {
      id: 'abc',
      name: 'storagia-postgres',
      image: 'pgvector/pgvector:pg17-trixie',
      status: 'Up 10 minutes (healthy)',
      state: 'running',
      ports: '5432/tcp',
      created_at: '2026-08-14 10:00:00',
      cpu: '0.20%',
      memory: '124.2MiB / 1.798GiB',
      memory_percent: '6.74%',
      network_io: '1kB / 2kB',
      block_io: '0B / 0B',
      pids: '12',
    },
  ],
  '/api/projects': [
    {
      slug: 'storagia-db',
      name: 'Storagia PostgreSQL',
      compose_file: '/opt/tinkiva/apps/storagia-db/compose.yaml',
      env_file: '/opt/tinkiva/apps/storagia-db/.env',
      image_env: null,
      branch: null,
      webhook_token: 'token-de-prueba-0123456789',
      current_image: 'postgres:17-alpine',
      created_at: 1_700_000_000,
      kind: 'database',
      engine: 'postgres',
      repository: null,
      installation_id: null,
    },
    {
      slug: 'storagia-api',
      name: 'Storagia API',
      compose_file: '/opt/tinkiva/apps/storagia-api/compose.yaml',
      env_file: '/opt/tinkiva/apps/storagia-api/.env',
      image_env: 'APP_IMAGE',
      branch: 'main',
      webhook_token: 'token-de-prueba-9876543210',
      current_image: 'ghcr.io/isaul19/api:sha',
      created_at: 1_700_000_100,
      kind: 'repository',
      engine: null,
      repository: 'isaul19/storagia',
      installation_id: 42,
    },
  ],
  '/api/history': [
    {
      id: 3,
      project: 'storagia-api',
      created_at: 1_700_000_200,
      status: 'success',
      branch: 'main',
      commit: 'a1b2c3d4e5f6',
      image: 'ghcr.io/isaul19/api:sha',
      previous_image: null,
      message: 'Deployment completado.',
      duration_ms: 4200,
      trigger: 'webhook',
    },
  ],
  '/api/processes': [
    {
      pid: 1,
      name: 'systemd',
      user: 'root',
      state: 'S',
      cpu_percent: 0.1,
      memory_bytes: 12_000_000,
      command: '/sbin/init',
    },
  ],
  '/api/github': { connected: false, public_url: 'http://127.0.0.1:8787' },
  '/api/github/installations': [],
};

/** Monta el bundle en un DOM limpio y devuelve utilidades de inspección. */
async function mount({ token = null, hash = '#/dashboard' } = {}) {
  const { window, document } = parseHTML(
    '<!doctype html><html><body><div id="root"></div></body></html>',
  );

  const store = new Map();
  if (token) store.set('tdm-token', token);
  const sessionStorage = {
    getItem: (key) => (store.has(key) ? store.get(key) : null),
    setItem: (key, value) => store.set(key, String(value)),
    removeItem: (key) => store.delete(key),
  };

  const calls = [];
  const fetchStub = async (path) => {
    calls.push(path);
    const [base] = String(path).split('?');
    const body = API[base];
    if (body === undefined) {
      return {
        ok: false,
        status: 404,
        text: async () => JSON.stringify({ error: `sin mock para ${base}` }),
      };
    }
    return { ok: true, status: 200, text: async () => JSON.stringify(body) };
  };

  Object.assign(window, {
    sessionStorage,
    fetch: fetchStub,
    location: { hash, search: '', pathname: '/', origin: 'http://127.0.0.1:8787' },
    history: { replaceState: () => {} },
    matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }),
  });
  document.visibilityState = 'visible';

  const globals = {
    window,
    document,
    sessionStorage,
    fetch: fetchStub,
    location: window.location,
    navigator: { clipboard: { writeText: async () => {} } },
    requestAnimationFrame: (callback) => setTimeout(callback, 0),
    cancelAnimationFrame: (handle) => clearTimeout(handle),
    addEventListener: () => {},
    removeEventListener: () => {},
    Node: window.Node,
    Element: window.Element,
    Text: window.Text,
  };
  const saved = {};
  const install = () => {
    for (const [key, value] of Object.entries(globals)) {
      if (!(key in saved)) saved[key] = globalThis[key];
      Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
    }
  };
  const restore = () => {
    for (const [key, value] of Object.entries(saved)) {
      if (value === undefined) delete globalThis[key];
      else Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
    }
  };
  const flush = async (rounds = 12) => {
    for (let round = 0; round < rounds; round++) {
      await new Promise((done) => setTimeout(done, 0));
    }
  };

  install();
  try {
    // eslint-disable-next-line no-new-func
    new Function(bundle)();
    await flush();
  } finally {
    restore();
  }

  const root = () => document.getElementById('root');
  const text = () => root().textContent;

  /** Busca un elemento por su texto visible. */
  const find = (selector, needle) =>
    [...document.querySelectorAll(selector)].find((node) =>
      node.textContent.includes(needle),
    );

  /** Dispara un click con los globals instalados y espera al rerender. */
  const click = async (selector, needle) => {
    const target = find(selector, needle);
    assert.ok(target, `no se encontró «${needle}» (${selector})`);
    install();
    try {
      target.dispatchEvent(new window.Event('click', { bubbles: true }));
      await flush();
    } finally {
      restore();
    }
  };

  return { html: root().innerHTML, text: text(), currentText: text, calls, click, find, document };
}

const tests = [];
const test = (name, body) => tests.push([name, body]);

test('sin token muestra el acceso y la firma Rust + Preact', async () => {
  const { text, html } = await mount();
  assert.match(text, /Docker Manager/);
  assert.match(text, /Token administrador/);
  assert.match(text, /Construido con/);
  assert.match(text, /Rust/);
  assert.match(text, /Preact/);
  // Los logos de simple-icons deben haberse inlineado, no quedarse vacíos.
  assert.match(html, /<svg[^>]*class="brand-icon"/);
});

test('con token pinta el panel y las métricas del host', async () => {
  const { text, calls } = await mount({ token: 'x'.repeat(40) });
  assert.ok(calls.includes('/api/info'), 'debe pedir /api/info');
  assert.ok(calls.includes('/api/catalog'), 'debe pedir /api/catalog');

  assert.match(text, /Resumen/);
  assert.match(text, /Contenedores/);
  assert.match(text, /Recursos/);
  assert.match(text, /GitHub/);
  assert.match(text, /Sistema/);

  assert.match(text, /26\.7%/, 'CPU del host');
  assert.match(text, /Docker 27\.5\.1/, 'estado de Docker en el pie del menú');
  assert.match(text, /storagia-postgres/, 'contenedor en la vista previa');
  assert.match(text, /v0\.2\.0/, 'versión en el pie');
});

test('la vista de recursos distingue base de datos y repositorio', async () => {
  const { text } = await mount({ token: 'x'.repeat(40), hash: '#/resources' });
  assert.match(text, /Storagia PostgreSQL/);
  assert.match(text, /Base de datos/);
  assert.match(text, /Storagia API/);
  assert.match(text, /Repositorio/);
  assert.match(text, /isaul19\/storagia/);
});

test('la vista de despliegues formatea duración y estado', async () => {
  const { text } = await mount({ token: 'x'.repeat(40), hash: '#/deployments' });
  assert.match(text, /storagia-api/);
  assert.match(text, /success/);
  assert.match(text, /4\.2 s/);
  assert.match(text, /webhook/);
});

test('la vista de GitHub ofrece el alta de un clic cuando no hay App', async () => {
  const { text } = await mount({ token: 'x'.repeat(40), hash: '#/github' });
  assert.match(text, /Conectar con GitHub/);
  assert.match(text, /Ya tengo una GitHub App/);
});

test('la vista de sistema lista las herramientas externas', async () => {
  const { text } = await mount({ token: 'x'.repeat(40), hash: '#/system' });
  assert.match(text, /vps-1/);
  assert.match(text, /openssl/);
  assert.match(text, /git/);
  assert.match(text, /Disponible/);
  assert.match(text, /Construido con/);
});

test('la vista de procesos ordena por consumo', async () => {
  const { text } = await mount({ token: 'x'.repeat(40), hash: '#/processes' });
  assert.match(text, /systemd/);
  assert.match(text, /11\.4 MB/);
});

test('«Añadir recurso» ofrece los cuatro orígenes', async () => {
  const app = await mount({ token: 'x'.repeat(40) });
  await app.click('button', 'Añadir recurso');

  const text = app.currentText();
  assert.match(text, /Base de datos/);
  assert.match(text, /Imagen de Docker Hub/);
  assert.match(text, /Repositorio de GitHub/);
  assert.match(text, /Compose existente/);
});

test('el paso de base de datos pinta los motores del catálogo', async () => {
  const app = await mount({ token: 'x'.repeat(40) });
  await app.click('button', 'Añadir recurso');
  await app.click('.type-card', 'Base de datos');

  const text = app.currentText();
  assert.match(text, /PostgreSQL/);
  assert.match(text, /postgres:17-alpine/);
  assert.match(text, /Redis/);
  assert.match(text, /redis:7-alpine/);
  // PostgreSQL pide base de datos y usuario; el formulario debe mostrarlos.
  assert.match(text, /Base de datos/);
  assert.match(text, /Usuario/);
  assert.match(text, /Crear y desplegar/);
});

test('el paso de imagen arranca en el buscador de Docker Hub', async () => {
  const app = await mount({ token: 'x'.repeat(40) });
  await app.click('button', 'Añadir recurso');
  await app.click('.type-card', 'Imagen de Docker Hub');

  const search = app.document.querySelector('input[type="search"]');
  assert.ok(search, 'debe haber un campo de búsqueda');
  assert.match(search.getAttribute('placeholder'), /Docker Hub/);

  const text = app.currentText();
  assert.match(text, /Sugerencias populares/);
  assert.match(text, /nginx/);
  assert.match(text, /oficial/);
});

test('el origen de repositorio se bloquea si GitHub no está conectado', async () => {
  const app = await mount({ token: 'x'.repeat(40) });
  await app.click('button', 'Añadir recurso');

  const card = app.find('.type-card', 'Repositorio de GitHub');
  assert.ok(card, 'debe existir la tarjeta de repositorio');
  assert.ok(card.disabled, 'debe estar deshabilitada sin GitHub conectado');
  assert.match(card.textContent, /Conecta GitHub primero/);
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

console.log(`\n${tests.length - failures}/${tests.length} pruebas de interfaz correctas`);
process.exit(failures ? 1 : 0);
