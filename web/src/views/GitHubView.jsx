import { useState } from 'preact/hooks';
import {
  AlertTriangle,
  CheckCircle2,
  ExternalLink,
  Github,
  Plug,
  RefreshCw,
  Unplug,
} from 'lucide-preact';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { useAsync } from '../lib/hooks.js';
import { formatRelative } from '../lib/format.js';
import { AsyncBlock, Badge, Button, EmptyState, Panel, Spinner } from '../ui/Primitives.jsx';
import { CopyValue, Field, FormGrid, Input, TextArea } from '../ui/Form.jsx';
// `Field` e `Input` también se usan fuera del diálogo manual, para la URL pública.
import { Modal } from '../ui/Modal.jsx';
import { useToast } from '../ui/Toast.jsx';

/**
 * Envía el manifiesto a GitHub con un POST de navegador. GitHub crea la App,
 * y vuelve a `/github/callback`, donde el panel canjea el código.
 */
function submitManifest(action, manifest) {
  const form = document.createElement('form');
  form.method = 'POST';
  form.action = action;
  const input = document.createElement('input');
  input.type = 'hidden';
  input.name = 'manifest';
  input.value = manifest;
  form.appendChild(input);
  document.body.appendChild(form);
  form.submit();
}

export function GitHubView() {
  const { capabilities, refreshToken, reloadInfo } = useApp();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const [manual, setManual] = useState(false);
  const [publicUrl, setPublicUrl] = useState('');

  const status = useAsync(() => api.get('/api/github'), [refreshToken]);
  const connected = status.data?.connected;
  // Sin URL pública GitHub rechazaría el manifiesto, así que la App se crea sin webhook.
  const reachable = Boolean(status.data?.webhook_url) || publicUrl.trim().length > 0;

  const connect = async () => {
    setBusy(true);
    try {
      const { action, manifest } = await api.post('/api/github/manifest', {
        webhook_url: publicUrl.trim(),
      });
      submitManifest(action, manifest);
    } catch (error) {
      toast.error(error);
      setBusy(false);
    }
  };

  const install = async () => {
    setBusy(true);
    try {
      const { url } = await api.post('/api/github/install');
      window.location.href = url;
    } catch (error) {
      toast.error(error);
      setBusy(false);
    }
  };

  const disconnect = async () => {
    setBusy(true);
    try {
      const result = await api.del('/api/github');
      toast.success(result.message);
      status.reload();
      reloadInfo();
    } catch (error) {
      toast.error(error);
    } finally {
      setBusy(false);
    }
  };

  const missing = [
    !capabilities.curl && 'curl',
    !capabilities.openssl && 'openssl',
    !capabilities.git && 'git',
  ].filter(Boolean);

  return (
    <>
      {missing.length ? (
        <div class="notice notice-warning">
          <AlertTriangle size={17} />
          <div>
            <strong>Faltan herramientas en el servidor: {missing.join(', ')}.</strong>
            <p class="muted">
              El panel las invoca como subprocesos en lugar de enlazar librerías, por eso el binario
              sigue pesando poco. Instálalas con el gestor de paquetes de tu distribución.
            </p>
          </div>
        </div>
      ) : null}

      {status.loading && !status.data ? <Spinner /> : null}

      {status.data && !connected ? (
        <div class="two-column">
          <Panel eyebrow="UN CLIC" title="Conectar con GitHub">
            <p class="muted">
              Se abrirá GitHub con una App preconfigurada. Al aceptar, GitHub la crea, te devuelve
              aquí con las credenciales y después eliges en qué repositorios instalarla: todos o solo
              los que elijas.
            </p>
            <ol class="steps">
              <li>Creas la GitHub App desde tu cuenta u organización.</li>
              <li>Eliges los repositorios a los que dar acceso de lectura.</li>
              <li>El panel clona, construye y redespliega en cada <code>push</code>.</li>
            </ol>

            <Field
              label="URL pública del panel (opcional)"
              hint={
                reachable
                  ? 'GitHub usará esta dirección para entregar los webhooks de push.'
                  : 'Solo hace falta si quieres redespliegues automáticos.'
              }
            >
              <Input
                type="url"
                value={publicUrl}
                onInput={(event) => setPublicUrl(event.currentTarget.value)}
                placeholder={status.data?.webhook_url || 'https://panel.tudominio.com'}
              />
            </Field>

            {!reachable ? (
              <div class="notice notice-warning">
                <AlertTriangle size={17} />
                <div>
                  <strong>Estás entrando por {status.data?.panel_url || 'una dirección privada'}.</strong>
                  <p class="muted">
                    GitHub no puede llamar a una dirección privada, así que la App se creará{' '}
                    <strong>sin webhook</strong>: todo lo demás funciona —listar repositorios,
                    clonar y desplegar a mano— pero no habrá redespliegue automático al hacer{' '}
                    <code>push</code>. Puedes añadir el webhook más tarde desde los ajustes de la
                    App en GitHub, o rellenar arriba tu dominio público ahora.
                  </p>
                </div>
              </div>
            ) : null}

            <Button
              variant="primary"
              icon={Github}
              loading={busy}
              disabled={!capabilities.curl || !capabilities.openssl}
              onClick={connect}
            >
              Conectar con GitHub
            </Button>
            <p class="muted small">
              El retorno desde GitHub lo hace tu navegador, así que{' '}
              <code>{status.data?.panel_url || 'localhost'}</code> sirve perfectamente aunque no
              sea pública.
            </p>
          </Panel>

          <Panel eyebrow="ALTERNATIVA" title="Ya tengo una GitHub App">
            <p class="muted">
              Si prefieres crearla a mano en GitHub, pega aquí el App ID, el slug y la clave privada
              en formato PEM.
            </p>
            <Button icon={Plug} onClick={() => setManual(true)}>
              Introducir credenciales
            </Button>
          </Panel>
        </div>
      ) : null}

      {connected ? (
        <>
          <Panel
            eyebrow="CONECTADO"
            title={status.data.name || status.data.slug}
            action={
              <div class="row-actions">
                <Button icon={RefreshCw} size="sm" onClick={status.reload} />
                <Button icon={Unplug} size="sm" variant="danger" loading={busy} onClick={disconnect}>
                  Desconectar
                </Button>
              </div>
            }
          >
            <div class="github-summary">
              <CheckCircle2 size={18} class="ok-icon" />
              <div>
                <p>
                  App <strong>#{status.data.app_id}</strong> conectada{' '}
                  {formatRelative(status.data.connected_at)}.
                </p>
                <a class="text-link" href={status.data.html_url} target="_blank" rel="noreferrer">
                  Ver en GitHub <ExternalLink size={13} />
                </a>
              </div>
              <Button variant="primary" icon={Github} loading={busy} onClick={install}>
                Instalar en repositorios
              </Button>
            </div>
            {status.data.webhook_url ? (
              <CopyValue label="URL del webhook" value={status.data.webhook_url} />
            ) : (
              <div class="notice notice-warning">
                <AlertTriangle size={17} />
                <div>
                  <strong>Sin webhook configurado</strong>
                  <p class="muted">
                    El panel no es alcanzable desde internet en{' '}
                    <code>{status.data.panel_url}</code>, así que no hay redespliegue automático
                    al hacer <code>push</code>. Cuando tengas un dominio público, añádelo en{' '}
                    <a class="text-link" href={status.data.settings_url} target="_blank" rel="noreferrer">
                      los ajustes de la App <ExternalLink size={12} />
                    </a>{' '}
                    como <code>https://tu-dominio/hooks/github</code>, o define{' '}
                    <code>TDM_PUBLIC_URL</code> en la configuración del panel.
                  </p>
                </div>
              </div>
            )}
          </Panel>

          <Installations refreshToken={refreshToken} />
        </>
      ) : null}

      <ManualDialog
        open={manual}
        onClose={() => setManual(false)}
        onSaved={() => {
          setManual(false);
          status.reload();
          reloadInfo();
        }}
      />
    </>
  );
}

function Installations({ refreshToken }) {
  const installations = useAsync(() => api.get('/api/github/installations'), [refreshToken]);

  return (
    <Panel eyebrow="ACCESO" title="Instalaciones">
      <AsyncBlock
        query={installations}
        empty={
          <EmptyState
            icon={Github}
            title="La App todavía no está instalada"
            description="Pulsa «Instalar en repositorios» para elegir a qué repos dar acceso."
          />
        }
      >
        {(list) => (
          <ul class="installation-list">
            {list.map((installation) => (
              <li key={installation.id}>
                {installation.avatar_url ? (
                  <img src={installation.avatar_url} alt="" width="32" height="32" />
                ) : null}
                <div class="compact-main">
                  <strong>{installation.login}</strong>
                  <span class="muted small">
                    {installation.type} · {installation.id}
                  </span>
                </div>
                <Badge tone={installation.repository_selection === 'all' ? 'ok' : 'neutral'}>
                  {installation.repository_selection === 'all'
                    ? 'Todos los repos'
                    : 'Repos seleccionados'}
                </Badge>
                <a
                  class="text-link"
                  href={installation.html_url}
                  target="_blank"
                  rel="noreferrer"
                  aria-label="Configurar en GitHub"
                >
                  <ExternalLink size={15} />
                </a>
              </li>
            ))}
          </ul>
        )}
      </AsyncBlock>
    </Panel>
  );
}

function ManualDialog({ open, onClose, onSaved }) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const [form, setForm] = useState({
    app_id: '',
    slug: '',
    private_key: '',
    webhook_secret: '',
  });

  const update = (key) => (event) =>
    setForm((current) => ({ ...current, [key]: event.currentTarget.value }));

  const save = async () => {
    setBusy(true);
    try {
      await api.post('/api/github/manual', form);
      toast.success('GitHub App guardada.');
      onSaved();
    } catch (error) {
      toast.error(error);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      eyebrow="GITHUB APP"
      title="Credenciales manuales"
      description="Los encuentras en github.com/settings/apps tras crear la App."
      footer={
        <>
          <Button onClick={onClose}>Cancelar</Button>
          <Button variant="primary" loading={busy} onClick={save}>
            Guardar
          </Button>
        </>
      }
    >
      <FormGrid>
        <Field label="App ID">
          <Input value={form.app_id} onInput={update('app_id')} placeholder="123456" />
        </Field>
        <Field label="Slug de la App">
          <Input value={form.slug} onInput={update('slug')} placeholder="tinkiva-dm-abc123" />
        </Field>
        <Field label="Secreto del webhook" hint="Necesario para los redespliegues automáticos.">
          <Input value={form.webhook_secret} onInput={update('webhook_secret')} />
        </Field>
        <Field label="Clave privada (PEM)" wide hint="Incluye las líneas BEGIN y END.">
          <TextArea
            rows={7}
            value={form.private_key}
            onInput={update('private_key')}
            placeholder="-----BEGIN RSA PRIVATE KEY-----"
          />
        </Field>
      </FormGrid>
    </Modal>
  );
}
