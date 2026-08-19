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
    {
      id: 'def',
      name: 'storagia-api',
      image: 'ghcr.io/example/app:one',
      status: 'Up 10 minutes',
      state: 'running',
      ports: '127.0.0.1:3000->3000/tcp',
      created_at: '2026-08-14 10:00:00',
      cpu: '1.20%',
      memory: '48MiB / 384MiB',
      memory_percent: '12.50%',
      network_io: '1kB / 2kB',
      block_io: '0B / 0B',
      pids: '8',
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
      runtime_status: 'running',
      can_rollback: false,
      rollback_reason: 'Este recurso no usa una imagen configurable mediante archivo .env.',
      last_deployment: { created_at: 1_700_000_300, status: 'success' },
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
      runtime_status: 'stopped',
      can_rollback: true,
      rollback_reason: '',
      last_deployment: { created_at: 1_700_000_200, status: 'failed' },
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
  '/api/history/page': {
    items: [{ id: 3, project: 'storagia-api', created_at: 1_700_000_200, status: 'success', branch: 'main', commit: 'a1b2c3d4e5f6', image: 'ghcr.io/isaul19/api:sha', previous_image: null, message: 'Deployment completado.', duration_ms: 4200, trigger: 'webhook' }],
    total: 1, offset: 0, limit: 10,
  },
  '/api/projects/storagia-api/environment': { environment: 'NODE_ENV=production\nPORT=3000', managed_keys: ['TDM_MEMORY_LIMIT', 'APP_IMAGE'] },
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
  // Caso habitual: se entra por un túnel SSH, así que no hay URL pública.
  '/api/github': {
    connected: false,
    panel_url: 'http://localhost:8787',
    webhook_url: null,
  },
  '/api/github/installations': [],
  '/api/ecr': { connected: false },
  '/api/images': [
    {
      id: '111111111111',
      reference: 'pgvector/pgvector:pg17-trixie',
      repository: 'pgvector/pgvector',
      tag: 'pg17-trixie',
      size: '431MB',
      size_bytes: 431_000_000,
      created_since: '3 weeks ago',
      in_use: true,
      protected_by: null,
      containers: ['storagia-postgres'],
    },
    {
      id: '222222222222',
      reference: 'nginx:1.27',
      repository: 'nginx',
      tag: '1.27',
      size: '142MB',
      size_bytes: 142_000_000,
      created_since: '2 days ago',
      in_use: false,
      protected_by: null,
      containers: [],
    },
    {
      id: '333333333333',
      reference: 'tinkiva/storagia-api:sha-abc',
      repository: 'tinkiva/storagia-api',
      tag: 'sha-abc',
      size: '95MB',
      size_bytes: 95_000_000,
      created_since: '1 hour ago',
      in_use: false,
      protected_by: 'storagia-api',
      containers: [],
    },
  ],
  '/api/containers/storagia-postgres/export': {
    database: 'postgres',
    database_label: 'PostgreSQL',
    schemas: ['storagia', 'postgres'],
  },
};

/** Monta el bundle en un DOM limpio y devuelve utilidades de inspección. */
async function mount({ token = null, hash = '#/dashboard', api = {} } = {}) {
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
    // Se busca primero la ruta completa: hay endpoints cuya respuesta depende
    // del query string, como el listado de repositorios de ECR.
    const key = String(path) in api ? String(path) : base;
    const body = key in api ? api[key] : API[base];
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

  /** Busca un elemento por su texto visible o, si no lo tiene, por aria-label. */
  const find = (selector, needle) =>
    [...document.querySelectorAll(selector)].find(
      (node) =>
        node.textContent.includes(needle) ||
        (node.getAttribute('aria-label') || '').includes(needle),
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

  /**
   * Marca o desmarca un checkbox y dispara su `change`. En un navegador basta
   * con pulsar la etiqueta; linkedom no propaga el click al input asociado.
   */
  const toggle = async (selector, needle) => {
    const container = find(selector, needle);
    assert.ok(container, `no se encontró «${needle}» (${selector})`);
    const input = container.querySelector('input[type=checkbox]') || container;
    input.checked = !input.checked;
    install();
    try {
      input.dispatchEvent(new window.Event('change', { bubbles: true }));
      await flush();
    } finally {
      restore();
    }
  };

  return {
    html: root().innerHTML,
    text: text(),
    currentText: text,
    calls,
    click,
    toggle,
    find,
    document,
  };
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
  assert.match(text, /Corriendo/);
  assert.match(text, /Apagado/);
  assert.match(text, /Rollback no disponible/);
  assert.match(text, /Último despliegue/);
  assert.match(text, /falló/);
});

test('la vista de despliegues formatea duración y estado', async () => {
  const { text } = await mount({ token: 'x'.repeat(40), hash: '#/deployments' });
  assert.match(text, /storagia-api/);
  assert.match(text, /Completado/);
  assert.match(text, /4\.2 s/);
  assert.match(text, /webhook/);
});

test('la vista de GitHub ofrece el alta de un clic cuando no hay App', async () => {
  const { text } = await mount({ token: 'x'.repeat(40), hash: '#/github' });
  assert.match(text, /Conectar con GitHub/);
  assert.match(text, /Ya tengo una GitHub App/);
});

test('sobre localhost explica que no necesita webhook ni puerto público', async () => {
  // El auto-deploy es por polling saliente: la App se crea sin webhook y el
  // panel puede quedarse en localhost sin configurar nada más.
  const app = await mount({ token: 'x'.repeat(40), hash: '#/github' });
  const text = app.currentText();

  assert.match(text, /http:\/\/localhost:8787/);
  assert.match(text, /Sin webhook ni puerto público/);
  assert.match(text, /HTTPS saliente/);

  // El botón de conectar está habilitado: no hay ningún requisito extra.
  const connect = app.find('button', 'Conectar con GitHub');
  assert.ok(connect && !connect.disabled, 'debe poder conectarse igualmente');
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

test('«Añadir recurso» ya no ofrece el origen de Docker Hub', async () => {
  const app = await mount({ token: 'x'.repeat(40) });
  await app.click('button', 'Añadir recurso');

  const text = app.currentText();
  assert.ok(!text.includes('Imagen de Docker Hub'), 'el origen de imagen se retiró');
  assert.match(text, /Base de datos/);
  assert.match(text, /Repositorio de GitHub/);
  assert.doesNotMatch(text, /Compose existente/);
  assert.match(text, /Crear Docker Compose/, 'Compose cubre el caso de una imagen suelta');
});

test('el origen de repositorio se bloquea si GitHub no está conectado', async () => {
  const app = await mount({ token: 'x'.repeat(40) });
  await app.click('button', 'Añadir recurso');

  const card = app.find('.type-card', 'Repositorio de GitHub');
  assert.ok(card, 'debe existir la tarjeta de repositorio');
  assert.ok(card.disabled, 'debe estar deshabilitada sin GitHub conectado');
  assert.match(card.textContent, /Conecta GitHub primero/);
});

test('el origen de ECR se bloquea mientras no haya credenciales', async () => {
  const app = await mount({ token: 'x'.repeat(40) });
  await app.click('button', 'Añadir recurso');

  const card = app.find('.type-card', 'Imagen de Amazon ECR');
  assert.ok(card, 'debe existir la tarjeta de ECR');
  assert.ok(card.disabled, 'sin claves guardadas no se puede desplegar de ECR');
  assert.match(card.textContent, /Conecta Amazon ECR primero/);
});

test('con ECR conectado se eligen repositorio y etiqueta de una lista', async () => {
  const registry = '123456789012.dkr.ecr.us-east-1.amazonaws.com';
  const app = await mount({
    token: 'x'.repeat(40),
    api: {
      '/api/info': { ...API['/api/info'], ecr_registry: registry },
      '/api/ecr/repositories': { repositories: ['api', 'web'] },
      '/api/ecr/repositories?repository=api': {
        tags: [
          {
            tag: 'latest',
            image: `${registry}/api:latest`,
            pushed_at: 1_750_000_000,
            size_bytes: 104_857_600,
          },
          {
            tag: 'sha-a1b2c3',
            image: `${registry}/api:sha-a1b2c3`,
            pushed_at: 1_740_000_000,
            size_bytes: 104_000_000,
          },
        ],
      },
    },
  });
  await app.click('button', 'Añadir recurso');

  const card = app.find('.type-card', 'Imagen de Amazon ECR');
  assert.ok(!card.disabled, 'con credenciales la tarjeta se habilita');

  await app.click('.type-card', 'Imagen de Amazon ECR');
  assert.match(app.currentText(), /api/, 'debe listar los repositorios del registro');

  await app.click('.repo-results button', 'api');
  assert.ok(
    app.calls.some((path) => path.includes('repository=api')),
    'al elegir un repositorio se piden sus etiquetas',
  );

  const options = [...app.document.querySelectorAll('option')].map((node) => node.textContent);
  assert.ok(
    options.some((label) => label.startsWith('latest')),
    'la etiqueta más reciente encabeza la lista',
  );
  assert.ok(
    options.some((label) => label.includes('100 MB')),
    'cada etiqueta muestra su peso',
  );

  const text = app.currentText();
  assert.doesNotMatch(text, /docker-compose\.yml/, 'este formulario no pide YAML');
  assert.match(text, /Puerto del contenedor/, 'sí pide cómo exponerlo');
});

test('el menú de un contenedor de base de datos abre el diálogo de exportación', async () => {
  const app = await mount({ token: 'x'.repeat(40), hash: '#/containers' });
  assert.match(app.currentText(), /storagia-postgres/);

  await app.click('button', 'Acciones para storagia-postgres');
  assert.match(app.currentText(), /Exportar SQL/, 'el menú debe ofrecer la exportación');

  await app.click('button', 'Exportar SQL');
  assert.ok(
    app.calls.includes('/api/containers/storagia-postgres/export'),
    'debe consultar el motor y los esquemas',
  );

  const text = app.currentText();
  assert.match(text, /PostgreSQL detectado/);
  assert.match(text, /storagia/, 'debe listar los esquemas');
  assert.match(text, /Datos y estructura/);
  assert.match(text, /Solo estructura/);
  assert.match(text, /Solo datos/);
});

test('un contenedor que no es base de datos no ofrece exportar', async () => {
  const app = await mount({ token: 'x'.repeat(40), hash: '#/containers' });
  await app.click('button', 'Acciones para storagia-api');

  const menu = app.find('.action-menu-popover', 'Ver logs');
  assert.ok(menu, 'el menú debe estar abierto');
  assert.ok(!menu.textContent.includes('Exportar SQL'));
});

test('la vista de imágenes marca cuáles están en uso y bloquea su borrado', async () => {
  const app = await mount({ token: 'x'.repeat(40), hash: '#/images' });
  const text = app.currentText();
  assert.match(text, /pgvector\/pgvector/);
  assert.match(text, /411 MB/, 'tamaño calculado desde los bytes exactos');
  assert.match(text, /hace 3 semanas/, 'la antigüedad de Docker se traduce');
  assert.match(text, /En uso/);
  assert.match(text, /storagia-postgres/, 'debe decir qué contenedor la usa');
  assert.match(text, /Sin usar/);
  assert.match(text, /3 imágenes/);
  assert.match(text, /637 MB.*en disco/s, 'total por id único');
  assert.match(text, /226 MB.*recuperables/s);

  const rows = [...app.document.querySelectorAll('tbody tr')];
  assert.equal(rows.length, 3);
  const inUse = rows.find((row) => row.textContent.includes('pgvector'));
  const free = rows.find((row) => row.textContent.includes('nginx'));
  assert.ok(inUse.querySelector('button').disabled, 'la imagen en uso no se puede borrar');
  assert.ok(!free.querySelector('button').disabled, 'la imagen sin usar sí');
});

test('la limpieza masiva respeta las imágenes de rollback', async () => {
  const app = await mount({ token: 'x'.repeat(40), hash: '#/images' });

  // La versión anterior de un recurso se marca aparte, no como «sin usar».
  const rows = [...app.document.querySelectorAll('tbody tr')];
  const rollback = rows.find((row) => row.textContent.includes('tinkiva/storagia-api'));
  assert.match(rollback.textContent, /Rollback/);
  assert.match(rollback.textContent, /versión anterior de storagia-api/);

  await app.click('button', 'Limpiar sin usar');
  const text = app.currentText();
  assert.match(text, /Borrar 1 imagen\(es\) sin usar/, 'solo cuenta la que no es rollback');
  assert.match(text, /Se conservan/);
  assert.match(text, /botón «Rollback»/);
});

test('el formulario de base de datos deshabilita la RAM al marcar sin límite', async () => {
  const app = await mount({ token: 'x'.repeat(40) });
  await app.click('button', 'Añadir recurso');
  await app.click('.type-card', 'Base de datos');

  const memory = app.document.querySelector('input[type=number][max="16384"]');
  assert.ok(memory, 'debe existir el campo de RAM');
  assert.ok(!memory.disabled, 'por defecto se puede escribir');
  assert.match(app.currentText(), /Sin límite de RAM/);

  await app.toggle('label', 'Sin límite de RAM');
  const after = app.document.querySelector('input[type=number][max="16384"]');
  assert.ok(after.disabled, 'al marcar sin límite el campo se deshabilita');
  assert.match(app.currentText(), /toda la memoria disponible del VPS/);
});

test('Amazon ECR aparece en integraciones y pide unas claves de solo lectura', async () => {
  const app = await mount({ token: 'x'.repeat(40), hash: '#/ecr' });
  const text = app.currentText();
  assert.match(text, /Integraciones/);
  assert.match(text, /Amazon ECR/);
  assert.match(text, /Access key ID/);
  assert.match(text, /Secret access key/);
  assert.match(text, /ecr:GetAuthorizationToken/, 'debe mostrar la política mínima');
  assert.doesNotMatch(text, /ecr:PutImage/, 'el panel no necesita permisos de escritura');
  assert.doesNotMatch(text, /ID de cuenta/, 'la cuenta la deduce del token, no se pregunta');

  const secret = app.document.querySelector('input[type=password]');
  assert.ok(secret, 'el secret no debe escribirse en claro');
});

test('el editor de variables reparte el .env en filas de clave y valor', async () => {
  const app = await mount({
    token: 'x'.repeat(40),
    hash: '#/resources',
    api: {
      '/api/projects/storagia-db/environment': {
        environment: "MAIL_ENABLED=true\nSES_FROM_EMAIL=no-reply@tinkiva.com",
        managed_keys: ['TDM_MEMORY_LIMIT'],
      },
    },
  });
  await app.click('button', 'Variables');

  const values = (label) =>
    [...app.document.querySelectorAll(`input[aria-label="${label}"]`)].map(
      (node) => node.value || node.getAttribute('value') || '',
    );

  assert.deepEqual(values('Clave'), ['MAIL_ENABLED', 'SES_FROM_EMAIL'], 'una fila por variable');
  assert.deepEqual(values('Valor'), ['true', 'no-reply@tinkiva.com']);

  const text = app.currentText();
  assert.match(text, /Añadir variable/);
  assert.match(text, /Importar \.env/);
  assert.match(text, /Editar como texto/);
  assert.match(text, /TDM_MEMORY_LIMIT/, 'sigue avisando de las claves gestionadas');
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
