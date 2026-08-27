import { useCallback, useMemo, useState } from 'preact/hooks';
import { Activity, Container, LogOut, RefreshCw, SlidersHorizontal } from 'lucide-preact';
import { api, clearToken } from '../lib/api.js';
import { AppContext } from '../lib/context.js';
import { navigate, useAsync, usePolling, useRoute } from '../lib/hooks.js';
import { BrandIcon } from '../ui/BrandIcon.jsx';
import { Button } from '../ui/Primitives.jsx';
import { Dashboard } from './Dashboard.jsx';
import { Containers } from './Containers.jsx';
import { System } from './System.jsx';

const SECTIONS = [
  {
    label: 'Observabilidad',
    items: [
      { id: 'dashboard', title: 'Resumen', kicker: 'MONITOREO', icon: Activity },
      { id: 'containers', title: 'Contenedores', kicker: 'DOCKER', icon: Container },
      { id: 'system', title: 'Sistema', kicker: 'HOST', icon: SlidersHorizontal },
    ],
  },
];

const PAGES = { dashboard: Dashboard, containers: Containers, system: System };
const ALL_ITEMS = SECTIONS.flatMap((section) => section.items);

export function Shell() {
  const route = useRoute();
  const [refreshToken, setRefreshToken] = useState(0);
  const info = useAsync(() => api.get('/api/info'), []);
  usePolling(info.reload, 30_000);

  const active = ALL_ITEMS.find((item) => item.id === route) || ALL_ITEMS[0];
  const Page = PAGES[active.id] || Dashboard;
  const refresh = useCallback(() => {
    setRefreshToken((value) => value + 1);
    info.reload();
  }, [info.reload]);
  const context = useMemo(
    () => ({ info: info.data, refreshToken, refresh, reloadInfo: info.reload }),
    [info.data, refreshToken, refresh, info.reload],
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
            <div class="brand-mark small" aria-hidden="true">T</div>
            <div><strong>TinkivaCreateApp</strong><span>Monitor</span></div>
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
                    <item.icon size={17} /><span>{item.title}</span>
                  </button>
                ))}
              </div>
            ))}
          </nav>
          <div class="sidebar-foot">
            <div class="docker-status"><span class={`status-dot ${dockerTone}`} /><span>{dockerLabel}</span></div>
            <p class="built-with small">
              <BrandIcon slug="rust" size={13} title="Rust" /> Rust
              <span aria-hidden="true">·</span>
              <BrandIcon slug="preact" size={13} title="Preact" /> Preact
              {info.data ? <span class="version">v{info.data.version}</span> : null}
            </p>
          </div>
        </aside>
        <main class="main">
          <header class="topbar">
            <div><p class="eyebrow">{active.kicker}</p><h2>{active.title}</h2></div>
            <div class="top-actions">
              <Button icon={RefreshCw} onClick={refresh}>Actualizar</Button>
              <Button icon={LogOut} onClick={clearToken} aria-label="Salir" />
            </div>
          </header>
          <div class="page"><Page /></div>
        </main>
      </div>
    </AppContext.Provider>
  );
}
