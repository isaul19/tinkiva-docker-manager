import { AlertTriangle, CheckCircle2 } from 'lucide-preact';
import { CopyValue } from '../ui/Form.jsx';
import { Badge, stateTone } from '../ui/Primitives.jsx';

/**
 * Resumen posterior al alta. La contraseña generada solo se muestra aquí: no
 * vuelve a salir por la API, únicamente queda en el `.env` del recurso.
 */
export function ResultPanel({ result }) {
  const {
    project,
    deployment,
    password,
    connection_uri: uri,
    host,
    published_port: port,
    external_access: externalAccess = false,
  } = result;
  const failed = !deployment || deployment.status !== 'success';

  return (
    <div class="result-panel">
      <div class={`notice ${failed ? 'notice-warning' : 'notice-success'}`}>
        {failed ? <AlertTriangle size={17} /> : <CheckCircle2 size={17} />}
        <div>
          <strong>
            {failed
              ? 'Recurso registrado, pero el despliegue no terminó bien'
              : `${project.name} está en marcha`}
          </strong>
          {deployment ? (
            <p class="muted small">{deployment.message}</p>
          ) : (
            <p class="muted small">{result.error}</p>
          )}
        </div>
        {deployment ? <Badge tone={stateTone(deployment.status)}>{deployment.status}</Badge> : null}
      </div>

      {password ? (
        <div class="notice notice-accent">
          <div>
            <strong>Guarda esta contraseña ahora</strong>
            <p class="muted small">
              No se volverá a mostrar. Queda escrita en el archivo <code>.env</code> del recurso.
            </p>
          </div>
        </div>
      ) : null}

      <div class="result-values">
        {password ? <CopyValue label="Contraseña" value={password} masked /> : null}
        {uri ? <CopyValue label="Cadena de conexión" value={uri} masked={Boolean(password)} /> : null}
        {host ? <CopyValue label="Host interno" value={host} /> : null}
        {port ? (
          <CopyValue
            label={externalAccess ? 'Puerto externo' : 'Puerto del VPS'}
            value={`${externalAccess ? '0.0.0.0' : '127.0.0.1'}:${port}`}
          />
        ) : null}
        <CopyValue label="Compose" value={project.compose_file} />
      </div>

      <p class="muted small">
        Otros contenedores de la red <code>tinkiva</code> pueden llegar a este recurso por su host
        interno. {externalAccess && port
          ? 'El puerto del VPS escucha en 0.0.0.0; para entrar desde Internet usa la IP o dominio del VPS y permite el puerto en el firewall o Security Group.'
          : 'El puerto del VPS, si lo pediste, solo escucha en 127.0.0.1.'}
      </p>
    </div>
  );
}
