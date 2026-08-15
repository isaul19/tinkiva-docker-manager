'use strict';

const TOKEN_KEY = 'tdm_admin_token';
const titles = {
  dashboard: ['OPERACIONES', 'Resumen'],
  containers: ['DOCKER', 'Contenedores'],
  projects: ['COMPOSE', 'Proyectos'],
  history: ['AUDITORÍA', 'Despliegues'],
  postgres: ['PLANTILLA', 'PostgreSQL'],
  settings: ['SERVIDOR', 'Sistema'],
};

const state = {
  token: sessionStorage.getItem(TOKEN_KEY) || '',
  page: 'dashboard',
  info: null,
  metrics: null,
  containers: [],
  projects: [],
  history: [],
  busy: 0,
  logsPath: '',
  logsTitle: '',
  timer: null,
};

const $ = (selector, root = document) => root.querySelector(selector);
const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];

function escapeHtml(value) {
  return String(value ?? '')
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#039;');
}

function formatBytes(bytes) {
  const value = Number(bytes || 0);
  if (!Number.isFinite(value) || value <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  const scaled = value / (1024 ** index);
  return `${scaled.toFixed(index > 1 ? 1 : 0)} ${units[index]}`;
}

function formatDate(epochSeconds) {
  if (!epochSeconds) return '—';
  return new Intl.DateTimeFormat('es-PE', {
    dateStyle: 'short',
    timeStyle: 'medium',
  }).format(new Date(Number(epochSeconds) * 1000));
}

function formatDuration(milliseconds) {
  const value = Number(milliseconds || 0);
  if (value < 1000) return `${value} ms`;
  if (value < 60000) return `${(value / 1000).toFixed(1)} s`;
  return `${(value / 60000).toFixed(1)} min`;
}

function clampPercent(value) {
  return Math.max(0, Math.min(100, Number(value || 0)));
}

function toast(message, type = 'success') {
  const element = document.createElement('div');
  element.className = `toast ${type}`;
  element.textContent = message;
  $('#toast-region').append(element);
  window.setTimeout(() => element.remove(), 5200);
}

function setBusy(active) {
  state.busy += active ? 1 : -1;
  state.busy = Math.max(0, state.busy);
  $('#loading-bar').classList.toggle('active', state.busy > 0);
}

async function api(path, options = {}) {
  const { method = 'GET', form, expect = 'json', anonymous = false } = options;
  const headers = {};
  if (!anonymous && state.token) headers.Authorization = `Bearer ${state.token}`;
  let body;
  if (form !== undefined) {
    headers['Content-Type'] = 'application/x-www-form-urlencoded;charset=UTF-8';
    body = form instanceof URLSearchParams ? form.toString() : new URLSearchParams(form).toString();
  }

  setBusy(true);
  try {
    const response = await fetch(path, { method, headers, body, cache: 'no-store' });
    const contentType = response.headers.get('content-type') || '';
    let payload;
    if (expect === 'text' && response.ok) {
      payload = await response.text();
    } else if (contentType.includes('application/json')) {
      payload = await response.json().catch(() => ({}));
    } else {
      payload = await response.text();
    }

    if (response.status === 401) {
      logout(false);
      throw new Error('Token inválido o sesión vencida.');
    }
    if (!response.ok) {
      const message = typeof payload === 'object' && payload?.error
        ? payload.error
        : (String(payload || `HTTP ${response.status}`));
      throw new Error(message);
    }
    return payload;
  } finally {
    setBusy(false);
  }
}

function showLogin() {
  $('#login-view').classList.remove('hidden');
  $('#app-shell').classList.add('hidden');
  $('#token').value = state.token;
}

function showApp() {
  $('#login-view').classList.add('hidden');
  $('#app-shell').classList.remove('hidden');
}

function logout(notify = true) {
  state.token = '';
  sessionStorage.removeItem(TOKEN_KEY);
  clearInterval(state.timer);
  showLogin();
  if (notify) toast('Sesión cerrada.', 'success');
}

async function authenticate(token) {
  state.token = token.trim();
  const info = await api('/api/info');
  state.info = info;
  sessionStorage.setItem(TOKEN_KEY, state.token);
  showApp();
  updateDockerStatus();
  $('#allowed-root-hint').textContent = info.allowed_root;
  await goTo('dashboard');
  startAutoRefresh();
}

function updateDockerStatus() {
  const dot = $('#docker-dot');
  const label = $('#docker-label');
  dot.classList.remove('ok', 'error');
  if (state.info?.docker?.available) {
    dot.classList.add('ok');
    label.textContent = `Docker ${state.info.docker.server_version || ''}`.trim();
  } else {
    dot.classList.add('error');
    label.textContent = state.info?.docker?.error || 'Docker no disponible';
  }
}

async function goTo(page) {
  if (!titles[page]) return;
  state.page = page;
  $$('.nav-item').forEach((button) => button.classList.toggle('active', button.dataset.page === page));
  $$('.page').forEach((section) => section.classList.toggle('active', section.id === `page-${page}`));
  $('#page-kicker').textContent = titles[page][0];
  $('#page-title').textContent = titles[page][1];
  await refreshPage();
}

async function refreshPage() {
  try {
    if (state.page === 'dashboard') await loadDashboard();
    if (state.page === 'containers') await loadContainers(true);
    if (state.page === 'projects') await loadProjects(true);
    if (state.page === 'history') await loadHistory(true);
    if (state.page === 'settings') await loadSystem();
  } catch (error) {
    toast(error.message, 'error');
  }
}

function startAutoRefresh() {
  clearInterval(state.timer);
  state.timer = window.setInterval(() => {
    if (document.hidden || state.busy > 0 || !['dashboard', 'containers'].includes(state.page)) return;
    refreshPage();
  }, 15000);
}

async function loadDashboard() {
  const [info, metrics, containers, history] = await Promise.all([
    api('/api/info'),
    api('/api/system'),
    api('/api/containers'),
    api('/api/history?limit=6'),
  ]);
  state.info = info;
  state.metrics = metrics;
  state.containers = containers;
  state.history = history;
  updateDockerStatus();
  $('#allowed-root-hint').textContent = info.allowed_root;
  renderMetrics(metrics, info);
  renderDashboardContainers(containers);
  renderDashboardHistory(history);
}

function renderMetrics(metrics, info) {
  const memoryPercent = metrics.memory_total
    ? (metrics.memory_used / metrics.memory_total) * 100
    : 0;
  const diskPercent = clampPercent(metrics.disk_percent);
  const cpuPercent = clampPercent(metrics.cpu_percent);
  const rssPercent = metrics.memory_total
    ? (metrics.process_rss / metrics.memory_total) * 100
    : 0;
  const cards = [
    ['CPU del host', `${cpuPercent.toFixed(1)}%`, `${metrics.cpu_threads} hilos · carga ${metrics.load_1}`, cpuPercent],
    ['Memoria del host', formatBytes(metrics.memory_used), `${formatBytes(metrics.memory_total)} total`, memoryPercent],
    ['Disco raíz', formatBytes(metrics.disk_used), `${formatBytes(metrics.disk_available)} libres`, diskPercent],
    ['Panel Rust', formatBytes(metrics.process_rss), `${info.workers} workers · ${rssPercent.toFixed(2)}% del host`, rssPercent],
  ];
  const grid = $('#metrics-grid');
  grid.classList.remove('skeleton-grid');
  grid.innerHTML = cards.map(([label, value, detail, percent]) => `
    <article class="metric-card">
      <span class="metric-label">${escapeHtml(label)}</span>
      <strong class="metric-value">${escapeHtml(value)}</strong>
      <span class="metric-detail">${escapeHtml(detail)}</span>
      <progress class="meter" max="100" value="${clampPercent(percent).toFixed(2)}" aria-label="${escapeHtml(label)}"></progress>
    </article>`).join('');
}

function renderDashboardContainers(containers) {
  const target = $('#dashboard-containers');
  if (!containers.length) {
    target.className = 'compact-list empty-state';
    target.textContent = 'No hay contenedores Docker.';
    return;
  }
  target.className = 'compact-list';
  target.innerHTML = containers.slice(0, 6).map((container) => `
    <div class="compact-row">
      <div><strong>${escapeHtml(container.name)}</strong><small>${escapeHtml(container.image)}</small></div>
      <div><span class="badge ${escapeHtml(container.state)}">${escapeHtml(container.state)}</span><small>${escapeHtml(container.memory || 'sin métricas')}</small></div>
    </div>`).join('');
}

function renderDashboardHistory(history) {
  const target = $('#dashboard-history');
  if (!history.length) {
    target.className = 'compact-list empty-state';
    target.textContent = 'Todavía no hay despliegues.';
    return;
  }
  target.className = 'compact-list';
  target.innerHTML = history.map((item) => `
    <div class="compact-row">
      <div><strong>${escapeHtml(item.project)}</strong><small>${escapeHtml(item.image || item.message)}</small></div>
      <div><span class="badge ${escapeHtml(item.status)}">${escapeHtml(item.status)}</span><small>${escapeHtml(formatDate(item.created_at))}</small></div>
    </div>`).join('');
}

async function loadContainers(render = true) {
  state.containers = await api('/api/containers');
  if (render) renderContainers();
}

function renderContainers() {
  const target = $('#containers-content');
  if (!state.containers.length) {
    target.innerHTML = '<div class="empty-state">No hay contenedores Docker.</div>';
    return;
  }
  target.innerHTML = `<div class="table-wrap"><table>
    <thead><tr><th>Contenedor</th><th>Estado</th><th>CPU</th><th>Memoria</th><th>Puertos</th><th>Acciones</th></tr></thead>
    <tbody>${state.containers.map((container) => {
      const running = container.state === 'running';
      return `<tr>
        <td><span class="cell-primary">${escapeHtml(container.name)}</span><span class="cell-secondary">${escapeHtml(container.image)}</span></td>
        <td><span class="badge ${escapeHtml(container.state)}">${escapeHtml(container.state)}</span><span class="cell-secondary">${escapeHtml(container.status)}</span></td>
        <td>${escapeHtml(container.cpu || '—')}</td>
        <td>${escapeHtml(container.memory || '—')}<span class="cell-secondary">${escapeHtml(container.memory_percent || '')}</span></td>
        <td>${escapeHtml(container.ports || '—')}</td>
        <td><div class="action-group">
          <button class="ghost small-button" data-container-logs="${escapeHtml(container.name)}">Logs</button>
          ${running
            ? `<button class="ghost small-button" data-container-action="restart" data-container="${escapeHtml(container.name)}">Reiniciar</button><button class="ghost small-button danger-text" data-container-action="stop" data-container="${escapeHtml(container.name)}">Detener</button>`
            : `<button class="ghost small-button" data-container-action="start" data-container="${escapeHtml(container.name)}">Iniciar</button>`}
        </div></td>
      </tr>`;
    }).join('')}</tbody>
  </table></div>`;
}

async function containerAction(name, action) {
  const labels = { start: 'iniciar', stop: 'detener', restart: 'reiniciar' };
  if (!confirm(`¿Deseas ${labels[action] || action} ${name}?`)) return;
  const result = await api(`/api/containers/${encodeURIComponent(name)}/${action}`, { method: 'POST', form: {} });
  toast(result.message || 'Acción completada.');
  await loadContainers(true);
}

async function loadProjects(render = true) {
  state.projects = await api('/api/projects');
  if (render) renderProjects();
  updateHistoryFilter();
}

function renderProjects() {
  const target = $('#projects-grid');
  if (!state.projects.length) {
    target.innerHTML = '<div class="empty-state panel">No hay proyectos registrados. Agrega un archivo Compose existente.</div>';
    return;
  }
  target.innerHTML = state.projects.map((project) => {
    const webhookUrl = `${location.origin}/hooks/deploy/${project.slug}`;
    return `<article class="panel project-card">
      <div class="project-title"><div><p class="eyebrow">${escapeHtml(project.slug)}</p><h3>${escapeHtml(project.name)}</h3></div><span class="badge">${escapeHtml(project.branch || 'cualquier rama')}</span></div>
      <div class="project-meta">
        <div class="meta-row"><span>Compose</span><code title="${escapeHtml(project.compose_file)}">${escapeHtml(project.compose_file)}</code></div>
        <div class="meta-row"><span>Imagen</span><code title="${escapeHtml(project.current_image || '')}">${escapeHtml(project.current_image || 'sin imagen registrada')}</code></div>
        <div class="meta-row"><span>Variable</span><code>${escapeHtml(project.image_env || '—')}</code></div>
      </div>
      <div class="webhook-box"><small>Webhook</small><code>${escapeHtml(webhookUrl)}</code><button class="ghost small-button" data-copy="${escapeHtml(webhookUrl)}">Copiar URL</button> <button class="ghost small-button" data-copy="${escapeHtml(project.webhook_token || '')}">Copiar token</button></div>
      <div class="project-actions">
        <button class="primary small-button" data-project-deploy="${escapeHtml(project.slug)}">Desplegar</button>
        <button class="ghost small-button" data-project-logs="${escapeHtml(project.slug)}" data-project-name="${escapeHtml(project.name)}">Logs</button>
        <button class="ghost small-button" data-project-rollback="${escapeHtml(project.slug)}">Rollback</button>
        <button class="ghost small-button danger-text" data-project-delete="${escapeHtml(project.slug)}">Quitar</button>
      </div>
    </article>`;
  }).join('');
}

function openDeploy(slug) {
  const project = state.projects.find((item) => item.slug === slug);
  if (!project) return;
  const form = $('#deploy-form');
  form.reset();
  form.elements.slug.value = project.slug;
  form.elements.image.value = project.current_image || '';
  form.elements.branch.value = project.branch || '';
  $('#deploy-title').textContent = `Desplegar ${project.name}`;
  $('#deploy-dialog').showModal();
}

async function rollbackProject(slug) {
  if (!confirm(`¿Hacer rollback de ${slug} a su imagen anterior?`)) return;
  const result = await api(`/api/projects/${encodeURIComponent(slug)}/rollback`, { method: 'POST', form: {} });
  toast(`Rollback ${result.status}: ${result.message}`);
  await loadProjects(true);
}

async function deleteProject(slug) {
  if (!confirm(`¿Desregistrar ${slug}? No se eliminarán archivos ni contenedores.`)) return;
  const result = await api(`/api/projects/${encodeURIComponent(slug)}`, { method: 'DELETE' });
  toast(result.message || 'Proyecto desregistrado.');
  await loadProjects(true);
}

async function loadHistory(render = true) {
  const filter = $('#history-filter').value;
  const query = filter ? `?limit=200&project=${encodeURIComponent(filter)}` : '?limit=200';
  state.history = await api(`/api/history${query}`);
  if (!state.projects.length) {
    try { await loadProjects(false); } catch (_) { /* el historial puede mostrarse sin proyectos */ }
  }
  updateHistoryFilter();
  if (render) renderHistory();
}

function updateHistoryFilter() {
  const select = $('#history-filter');
  const selected = select.value;
  const options = ['<option value="">Todos los proyectos</option>', ...state.projects.map((project) => `<option value="${escapeHtml(project.slug)}">${escapeHtml(project.name)}</option>`)].join('');
  select.innerHTML = options;
  if ([...select.options].some((option) => option.value === selected)) select.value = selected;
}

function renderHistory() {
  const target = $('#history-content');
  if (!state.history.length) {
    target.innerHTML = '<div class="empty-state">No hay despliegues para este filtro.</div>';
    return;
  }
  target.innerHTML = `<div class="table-wrap"><table>
    <thead><tr><th>#</th><th>Proyecto</th><th>Resultado</th><th>Imagen / commit</th><th>Origen</th><th>Fecha</th></tr></thead>
    <tbody>${state.history.map((item) => `<tr>
      <td>${item.id}</td>
      <td><span class="cell-primary">${escapeHtml(item.project)}</span><span class="cell-secondary">${escapeHtml(formatDuration(item.duration_ms))}</span></td>
      <td><span class="badge ${escapeHtml(item.status)}">${escapeHtml(item.status)}</span><span class="cell-secondary" title="${escapeHtml(item.message)}">${escapeHtml(item.message)}</span></td>
      <td><span>${escapeHtml(item.image || '—')}</span><span class="cell-secondary">${escapeHtml(item.commit || item.branch || '')}</span></td>
      <td>${escapeHtml(item.trigger)}</td>
      <td>${escapeHtml(formatDate(item.created_at))}</td>
    </tr>`).join('')}</tbody>
  </table></div>`;
}

async function loadSystem() {
  const [info, metrics] = await Promise.all([api('/api/info'), api('/api/system')]);
  state.info = info;
  state.metrics = metrics;
  updateDockerStatus();
  $('#system-content').innerHTML = `
    <article class="panel settings-card"><p class="eyebrow">PANEL</p><h3>Tinkiva Docker Manager</h3><dl>
      <div><dt>Versión</dt><dd>${escapeHtml(info.version)}</dd></div>
      <div><dt>RSS actual</dt><dd>${escapeHtml(formatBytes(metrics.process_rss))}</dd></div>
      <div><dt>Workers</dt><dd>${escapeHtml(info.workers)}</dd></div>
      <div><dt>Inicio</dt><dd>${escapeHtml(formatDate(info.started_at))}</dd></div>
      <div><dt>Historial</dt><dd>${escapeHtml(info.deployments)} registros</dd></div>
    </dl></article>
    <article class="panel settings-card"><p class="eyebrow">DOCKER</p><h3>Motor</h3><dl>
      <div><dt>Disponible</dt><dd><span class="badge ${info.docker.available ? 'success' : 'failed'}">${info.docker.available ? 'sí' : 'no'}</span></dd></div>
      <div><dt>Engine</dt><dd>${escapeHtml(info.docker.server_version || '—')}</dd></div>
      <div><dt>Compose</dt><dd>${escapeHtml(info.docker.compose_version || '—')}</dd></div>
      <div><dt>Error</dt><dd>${escapeHtml(info.docker.error || '—')}</dd></div>
    </dl></article>
    <article class="panel settings-card"><p class="eyebrow">RUTAS</p><h3>Confinamiento</h3><dl>
      <div><dt>Apps</dt><dd><code>${escapeHtml(info.allowed_root)}</code></dd></div>
      <div><dt>Datos</dt><dd><code>${escapeHtml(info.data_dir)}</code></dd></div>
      <div><dt>Proyectos</dt><dd>${escapeHtml(info.projects)}</dd></div>
      <div><dt>Host</dt><dd>${escapeHtml(metrics.hostname)}</dd></div>
    </dl></article>
    <article class="panel settings-card"><p class="eyebrow">HOST</p><h3>Linux</h3><dl>
      <div><dt>Uptime</dt><dd>${escapeHtml((metrics.uptime_seconds / 86400).toFixed(1))} días</dd></div>
      <div><dt>Swap</dt><dd>${escapeHtml(formatBytes(metrics.swap_used))} / ${escapeHtml(formatBytes(metrics.swap_total))}</dd></div>
      <div><dt>Carga</dt><dd>${escapeHtml(metrics.load_1)} · ${escapeHtml(metrics.load_5)} · ${escapeHtml(metrics.load_15)}</dd></div>
      <div><dt>Disco</dt><dd>${escapeHtml(metrics.disk_percent)}%</dd></div>
    </dl></article>`;
}

async function showLogs(path, title) {
  state.logsPath = path;
  state.logsTitle = title;
  $('#logs-title').textContent = title;
  $('#logs-output').textContent = 'Cargando…';
  $('#logs-dialog').showModal();
  await refreshLogs();
}

async function refreshLogs() {
  if (!state.logsPath) return;
  try {
    const text = await api(state.logsPath, { expect: 'text' });
    $('#logs-output').textContent = text || '(sin logs)';
    $('#logs-output').scrollTop = $('#logs-output').scrollHeight;
  } catch (error) {
    $('#logs-output').textContent = `ERROR: ${error.message}`;
  }
}

async function copyText(value) {
  if (!value) return toast('No existe un valor para copiar.', 'error');
  try {
    await navigator.clipboard.writeText(value);
    toast('Copiado al portapapeles.');
  } catch (_) {
    const area = document.createElement('textarea');
    area.value = value;
    document.body.append(area);
    area.select();
    document.execCommand('copy');
    area.remove();
    toast('Copiado al portapapeles.');
  }
}

function closeDialog(button) {
  const dialog = button.closest('dialog');
  if (dialog) dialog.close();
}

$('#login-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  try {
    await authenticate(new FormData(event.currentTarget).get('token'));
  } catch (error) {
    toast(error.message, 'error');
  }
});

$('#logout-button').addEventListener('click', () => logout());
$('#refresh-button').addEventListener('click', refreshPage);
$('#refresh-logs').addEventListener('click', refreshLogs);
$('#history-filter').addEventListener('change', () => loadHistory(true).catch((error) => toast(error.message, 'error')));
$('#open-project-dialog').addEventListener('click', () => $('#project-dialog').showModal());

$$('[data-close-dialog]').forEach((button) => button.addEventListener('click', () => closeDialog(button)));
$$('dialog').forEach((dialog) => dialog.addEventListener('click', (event) => {
  if (event.target === dialog) dialog.close();
}));

$('#project-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  const form = event.currentTarget;
  try {
    const project = await api('/api/projects', { method: 'POST', form: new URLSearchParams(new FormData(form)) });
    form.reset();
    $('#project-dialog').close();
    toast(`Proyecto ${project.name} registrado.`);
    await loadProjects(true);
  } catch (error) {
    toast(error.message, 'error');
  }
});

$('#deploy-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  const form = event.currentTarget;
  const data = new URLSearchParams(new FormData(form));
  const slug = data.get('slug');
  data.delete('slug');
  try {
    const result = await api(`/api/projects/${encodeURIComponent(slug)}/deploy`, { method: 'POST', form: data });
    $('#deploy-dialog').close();
    toast(`Deployment ${result.status}: ${result.message}`, result.status === 'success' ? 'success' : 'error');
    await loadProjects(true);
  } catch (error) {
    toast(error.message, 'error');
  }
});

$('#postgres-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  const form = event.currentTarget;
  if (!confirm('¿Crear y desplegar esta base PostgreSQL?')) return;
  try {
    const result = await api('/api/templates/postgres', { method: 'POST', form: new URLSearchParams(new FormData(form)) });
    $('#postgres-result').className = 'result-secret';
    $('#postgres-result').innerHTML = `
      <div class="secret-field"><span>Contraseña — guárdala ahora</span><code>${escapeHtml(result.password)}</code></div>
      <div class="secret-field"><span>URI desde la red Docker tinkiva</span><code>${escapeHtml(result.connection_uri)}</code></div>
      <button class="ghost" type="button" data-copy="${escapeHtml(result.connection_uri)}">Copiar URI</button>`;
    toast('PostgreSQL creado y desplegado.');
    await loadProjects(false);
  } catch (error) {
    toast(error.message, 'error');
  }
});

document.addEventListener('click', async (event) => {
  const target = event.target.closest('button');
  if (!target) return;
  try {
    if (target.dataset.page) await goTo(target.dataset.page);
    if (target.dataset.go) await goTo(target.dataset.go);
    if (target.dataset.copy !== undefined) await copyText(target.dataset.copy);
    if (target.dataset.containerAction) await containerAction(target.dataset.container, target.dataset.containerAction);
    if (target.dataset.containerLogs) await showLogs(`/api/containers/${encodeURIComponent(target.dataset.containerLogs)}/logs?tail=500`, `Contenedor · ${target.dataset.containerLogs}`);
    if (target.dataset.projectDeploy) openDeploy(target.dataset.projectDeploy);
    if (target.dataset.projectLogs) await showLogs(`/api/projects/${encodeURIComponent(target.dataset.projectLogs)}/logs?tail=500`, `Proyecto · ${target.dataset.projectName}`);
    if (target.dataset.projectRollback) await rollbackProject(target.dataset.projectRollback);
    if (target.dataset.projectDelete) await deleteProject(target.dataset.projectDelete);
  } catch (error) {
    toast(error.message, 'error');
  }
});

(async function boot() {
  if (!state.token) return showLogin();
  try {
    await authenticate(state.token);
  } catch (_) {
    logout(false);
  }
}());
