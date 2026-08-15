import { useCallback, useEffect, useMemo, useState } from 'preact/hooks';
import {
  Activity,
  Boxes,
  Container,
  Cpu,
  Github,
  History,
  LogOut,
  Plus,
  RefreshCw,
  SlidersHorizontal,
} from 'lucide-preact';
import { api, clearToken } from '../lib/api.js';
import { AppContext } from '../lib/context.js';
import { navigate, useAsync, usePolling, useRoute } from '../lib/hooks.js';
import { BrandIcon } from '../ui/BrandIcon.jsx';
import { Button } from '../ui/Primitives.jsx';
import { useToast } from '../ui/Toast.jsx';
import { Dashboard } from './Dashboard.jsx';
import { Containers } from './Containers.jsx';
import { Processes } from './Processes.jsx';
import { Resources } from './Resources.jsx';
import { Deployments } from './Deployments.jsx';
import { GitHubView } from './GitHubView.jsx';
import { System } from './System.jsx';
import { AddResource } from '../resources/AddResource.jsx';

const SECTIONS = [
  {
    label: 'Operación',
    items: [
      { id: 'dashboard', title: 'Resumen', kicker: 'OPERACIONES', icon: Activity },
      { id: 'resources', title: 'Recursos', kicker: 'PROYECTOS', icon: Boxes },
      { id: 'containers', title: 'Contenedores', kicker: 'DOCKER', icon: Container },
      { id: 'deployments', title: 'Despliegues', kicker: 'ACTIVIDAD', icon: History },
    ],
  },
  {
    label: 'Integraciones',
    items: [{ id: 'github', title: 'GitHub', kicker: 'ORÍGENES', icon: Github }],
  },
  {
    label: 'Servidor',
    items: [
      { id: 'processes', title: 'Procesos', kicker: 'HOST', icon: Cpu },
      { id: 'system', title: 'Sistema', kicker: 'HOST', icon: SlidersHorizontal },
    ],
  },
];

const PAGES = {
  dashboard: Dashboard,
  resources: Resources,
  containers: Containers,
  deployments: Deployments,
  github: GitHubView,
  processes: Processes,
  system: System,
};

const ALL_ITEMS = SECTIONS.flatMap((section) => section.items);

export function Shell() {
  const route = useRoute();
  const toast = useToast();
  const [adding, setAdding] = useState(false);
  const [refreshToken, setRefreshToken] = useState(0);

  const info = useAsync(() => api.get('/api/info'), []);
  const catalog = useAsync(() => api.get('/api/catalog'), []);

  usePolling(info.reload, 30_000);

  // Mensaje de vuelta desde GitHub tras crear o instalar la App.
  useEffect(() => {
    const status = new URLSearchParams(window.location.search).get('github');
    if (!status) return;
    const messages = {
      conectado: ['success', 'GitHub App creada y conectada.'],
      instalado: ['success', 'Instalación de GitHub actualizada.'],
      estado_invalido: ['error', 'El enlace de retorno caducó; vuelve a intentarlo.'],
      error: ['error', 'GitHub no devolvió las credenciales de la App.'],
    };
    const [tone, message] = messages[status] || ['info', status];
    toast[tone === 'error' ? 'error' : 'success'](message);
    window.history.replaceState({}, '', `${window.location.pathname}${window.location.hash}`);
  }, [toast]);

  const active = ALL_ITEMS.find((item) => item.id === route) || ALL_ITEMS[0];
  const Page = PAGES[active.id] || Dashboard;

  const refresh = useCallback(() => {
    setRefreshToken((value) => value + 1);
    info.reload();
  }, [info]);

  const context = useMemo(
    () => ({
      info: info.data,
      catalog: catalog.data,
      capabilities: catalog.data?.capabilities || info.data?.capabilities || {},
      allowedRoot: catalog.data?.allowed_root || info.data?.allowed_root || '',
      refreshToken,
      refresh,
      reloadInfo: info.reload,
      openAddResource: () => setAdding(true),
    }),
    [info.data, catalog.data, refreshToken, refresh, info.reload],
  );

  const docker = info.data?.docker;
  const dockerTone = !info.data ? '' : docker?.available ? 'ok' : 'error';
  const dockerLabel = !info.data
    ? 'Comprobando Docker'
    : docker?.available
      ? `Docker ${docker.server_version || ''}`.trim()
      : 'Docker no disponible';

  return (
    <AppContext.Provider value={context}>
      <div class="app-shell">
        <aside class="sidebar">
          <div class="brand-row">
            <div class="brand-mark small" aria-hidden="true">
              T
            </div>
            <div>
              <strong>Tinkiva</strong>
              <span>Docker Manager</span>
            </div>
          </div>

          <nav aria-label="Navegación principal">
            {SECTIONS.map((section) => (
              <div class="nav-group" key={section.label}>
                <p class="nav-label">{section.label}</p>
                {section.items.map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    class={`nav-item${item.id === active.id ? ' active' : ''}`}
                    onClick={() => navigate(item.id)}
                    aria-current={item.id === active.id ? 'page' : undefined}
                  >
                    <item.icon size={17} />
                    <span>{item.title}</span>
                  </button>
                ))}
              </div>
            ))}
          </nav>

          <div class="sidebar-foot">
            <div class="docker-status">
              <span class={`status-dot ${dockerTone}`} />
              <span>{dockerLabel}</span>
            </div>
            <p class="built-with small">
              <BrandIcon slug="rust" size={13} title="Rust" />
              Rust
              <span aria-hidden="true">·</span>
              <BrandIcon slug="preact" size={13} title="Preact" />
              Preact
              {info.data ? <span class="version">v{info.data.version}</span> : null}
            </p>
          </div>
        </aside>

        <main class="main">
          <header class="topbar">
            <div>
              <p class="eyebrow">{active.kicker}</p>
              <h2>{active.title}</h2>
            </div>
            <div class="top-actions">
              <Button variant="primary" icon={Plus} onClick={() => setAdding(true)}>
                Añadir recurso
              </Button>
              <Button icon={RefreshCw} onClick={refresh}>
                Actualizar
              </Button>
              <Button icon={LogOut} onClick={clearToken} aria-label="Salir" />
            </div>
          </header>

          <div class="page">
            <Page />
          </div>
        </main>
      </div>

      <AddResource
        open={adding}
        onClose={() => setAdding(false)}
        onCreated={() => {
          setAdding(false);
          refresh();
          navigate('resources');
        }}
      />
    </AppContext.Provider>
  );
}
