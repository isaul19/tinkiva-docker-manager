import { CheckCircle2, XCircle } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { useAsync, usePolling } from '../lib/hooks.js';
import { formatBytes, formatNumber } from '../lib/format.js';
import { BrandIcon } from '../ui/BrandIcon.jsx';
import { AsyncBlock, Badge, Meter, Panel } from '../ui/Primitives.jsx';

function Rows({ entries }) { return <dl class="definition-list">{entries.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>; }
function formatUptime(seconds) { if (!seconds) return '—'; const days = Math.floor(seconds / 86400); const hours = Math.floor((seconds % 86400) / 3600); return days ? `${days} d ${hours} h` : `${hours} h ${Math.floor((seconds % 3600) / 60)} min`; }

export function System() {
  const { info, refreshToken } = useApp();
  const metrics = useAsync(() => api.get('/api/system'), [refreshToken]);
  usePolling(metrics.reload, 15_000);
  const dockerAvailable = Boolean(info?.docker?.available);
  return <>
    <p class="page-intro muted">Consumo del host y estado del acceso de solo lectura a Docker.</p>
    <div class="two-column">
      <Panel eyebrow="HOST" title="Recursos"><AsyncBlock query={metrics}>{(system) => <><Rows entries={[
        ['Hostname', system.hostname], ['Uptime', formatUptime(system.uptime_seconds)], ['Hilos de CPU', formatNumber(system.cpu_threads)],
        ['Carga', `${system.load_1.toFixed(2)} · ${system.load_5.toFixed(2)} · ${system.load_15.toFixed(2)}`],
        ['Memoria', `${formatBytes(system.memory_used)} de ${formatBytes(system.memory_total)}`],
        ['Swap', system.swap_total ? `${formatBytes(system.swap_used)} de ${formatBytes(system.swap_total)}` : 'Sin swap'],
        ['Disco raíz', `${formatBytes(system.disk_used)} de ${formatBytes(system.disk_total)}`],
      ]} /><Meter value={system.disk_percent} tone={system.disk_percent > 90 ? 'danger' : 'accent'} /></>}</AsyncBlock></Panel>
      <Panel eyebrow="PANEL" title="TinkivaCreateApp Monitor"><Rows entries={[
        ['Versión', info ? `v${info.version}` : '—'], ['Edición', 'TinkivaCreateApp'], ['Modo', 'Solo lectura'],
        ['Workers HTTP', info ? formatNumber(info.workers) : '—'], ['Memoria residente', metrics.data ? formatBytes(metrics.data.process_rss) : '—'],
        ['Directorio de datos', <code>{info?.data_dir || '—'}</code>],
      ]} /><p class="built-with"><span>Construido con</span><BrandIcon slug="rust" size={15} title="Rust" /><strong>Rust</strong><span>+</span><BrandIcon slug="preact" size={15} title="Preact" /><strong>Preact</strong></p></Panel>
    </div>
    <Panel eyebrow="DOCKER" title="Conexión al motor"><ul class="tool-list"><li>{dockerAvailable ? <CheckCircle2 size={17} class="ok-icon" /> : <XCircle size={17} class="error-icon" />}<div class="compact-main"><strong class="mono">docker</strong><span class="muted small">{info?.docker?.server_version || info?.docker?.error || 'Comprobando'}</span></div><Badge tone={dockerAvailable ? 'ok' : 'danger'}>{dockerAvailable ? 'Disponible' : 'No disponible'}</Badge></li></ul></Panel>
  </>;
}
