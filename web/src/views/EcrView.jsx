import { useState } from 'preact/hooks';
import { Cloud, Trash2 } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useAsync } from '../lib/hooks.js';
import { formatDateTime } from '../lib/format.js';
import { AsyncBlock, Button, Panel } from '../ui/Primitives.jsx';
import { CopyValue, Field, FormGrid, Input } from '../ui/Form.jsx';
import { useToast } from '../ui/Toast.jsx';

// `registry_id` sigue viajando vacío a propósito: el backend lo acepta, pero la
// cuenta la dice el propio token, así que no hay por qué preguntarla.
const EMPTY = { access_key_id: '', secret_access_key: '', region: 'us-east-1', registry_id: '' };

/**
 * Conexión con Amazon ECR. El panel solo necesita leer: pide un token de doce
 * horas con la API de AWS y lo usa para `docker login` antes de cada pull.
 */
export function EcrView() {
  const toast = useToast();
  const [form, setForm] = useState(EMPTY);
  const [busy, setBusy] = useState(false);

  const status = useAsync(() => api.get('/api/ecr'), []);

  const update = (key) => (event) =>
    setForm((current) => ({ ...current, [key]: event.currentTarget.value }));

  const connect = async (event) => {
    event.preventDefault();
    setBusy(true);
    try {
      await api.post('/api/ecr', form);
      setForm(EMPTY);
      toast.success('ECR conectado. Docker ya puede descargar de tu registro.');
      status.reload();
    } catch (error) {
      toast.error(error);
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async () => {
    setBusy(true);
    try {
      await api.del('/api/ecr');
      toast.success('ECR desconectado y credenciales borradas del servidor.');
      status.reload();
    } catch (error) {
      toast.error(error);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <p class="page-intro muted">
        Descarga imágenes privadas de Amazon ECR. Pensado para el flujo en que tu CI construye y
        sube la imagen, y el panel solo la despliega.
      </p>

      <AsyncBlock query={status}>
        {(data) =>
          data.connected ? (
            <Panel eyebrow="AMAZON ECR" title="Registro conectado">
              <FormGrid>
                <Field label="Registro">
                  <CopyValue value={data.registry} />
                </Field>
                <Field label="Región">
                  <Input value={data.region} readonly />
                </Field>
                <Field label="Access key" hint="Solo se guardan los últimos caracteres a la vista.">
                  <Input value={data.access_key_id} readonly />
                </Field>
                <Field
                  label="Token actual"
                  hint="AWS emite tokens de 12 h; el panel los renueva solo antes de cada descarga."
                >
                  <Input
                    value={data.token_expires_at ? formatDateTime(data.token_expires_at) : 'Se pedirá al desplegar'}
                    readonly
                  />
                </Field>
              </FormGrid>

              <p class="muted small">
                Para desplegar, crea un recurso con «Crear Docker Compose» apuntando a{' '}
                <code>{data.registry}/tu-imagen:tag</code>. Si además rellenas «Imagen a vigilar»,
                el panel redesplegará solo cuando tu CI suba una versión nueva.
              </p>

              <div class="form-actions">
                <Button variant="danger" icon={Trash2} loading={busy} onClick={disconnect}>
                  Desconectar
                </Button>
              </div>
            </Panel>
          ) : (
            <Panel eyebrow="AMAZON ECR" title="Conectar un registro">
              <form onSubmit={connect} autocomplete="off">
                <p class="muted small">
                  Crea un usuario IAM <strong>de solo lectura</strong> y pega aquí sus claves. El
                  panel solo descarga: nunca sube imágenes ni borra nada en AWS.
                </p>
                <p class="muted small">
                  La vía rápida es adjuntarle la política gestionada{' '}
                  <code>AmazonEC2ContainerRegistryReadOnly</code>. Concede más de lo necesario
                  (listar repositorios, ver escaneos), así que si prefieres lo mínimo, usa esta
                  política en línea:
                </p>
                <pre class="logs small">{`{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": "ecr:GetAuthorizationToken",
      "Resource": "*"
    },
    {
      "Effect": "Allow",
      "Action": [
        "ecr:DescribeRepositories",
        "ecr:DescribeImages",
        "ecr:BatchCheckLayerAvailability",
        "ecr:GetDownloadUrlForLayer",
        "ecr:BatchGetImage"
      ],
      "Resource": "*"
    }
  ]
}`}</pre>
                <p class="muted small">
                  <code>GetAuthorizationToken</code> es de cuenta y siempre exige <code>"*"</code>.
                  Las otras cinco valen para todos tus repositorios tal cual están; si algún día
                  quieres acotarlas, cambia ese <code>"*"</code> por{' '}
                  <code>arn:aws:ecr:REGION:CUENTA:repository/TU-REPO</code> y repite la línea por
                  cada repositorio que despliegues. Las dos <code>Describe*</code> son las que
                  permiten elegir imagen desde una lista en vez de escribirla a mano.
                </p>

                <FormGrid>
                  <Field label="Access key ID">
                    <Input
                      value={form.access_key_id}
                      onInput={update('access_key_id')}
                      required
                      placeholder="AKIAIOSFODNN7EXAMPLE"
                    />
                  </Field>
                  <Field label="Secret access key">
                    <Input
                      type="password"
                      value={form.secret_access_key}
                      onInput={update('secret_access_key')}
                      required
                    />
                  </Field>
                  <Field
                    label="Región"
                    hint="La del registro, por ejemplo us-east-1. La cuenta la deduce del token."
                  >
                    <Input value={form.region} onInput={update('region')} required />
                  </Field>
                </FormGrid>

                <p class="muted small">
                  Las claves se guardan en el servidor con permisos 0600 y no vuelven a mostrarse.
                  Se comprueban ahora mismo contra AWS: si la política es insuficiente, la conexión
                  falla aquí en lugar de a mitad de un despliegue.
                </p>

                <div class="form-actions">
                  <Button variant="primary" icon={Cloud} type="submit" loading={busy}>
                    Conectar
                  </Button>
                </div>
              </form>
            </Panel>
          )
        }
      </AsyncBlock>
    </>
  );
}
