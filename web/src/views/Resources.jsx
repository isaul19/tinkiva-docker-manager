import { useEffect, useState } from "preact/hooks";
import { Boxes, FilePenLine, FileText, Github, Layers, Rocket, Settings2, Trash2, Undo2, Webhook } from "lucide-preact";
import { api } from "../lib/api.js";
import { useApp } from "../lib/context.js";
import { useAsync, usePolling } from "../lib/hooks.js";
import { formatRelative } from "../lib/format.js";
import { BrandIcon, hasBrand } from "../ui/BrandIcon.jsx";
import { AsyncBlock, Badge, Button, EmptyState, Pagination, Panel } from "../ui/Primitives.jsx";
import { CopyValue, Field, FormGrid, Input, Select, TextArea } from "../ui/Form.jsx";
import { Modal } from "../ui/Modal.jsx";
import { useToast } from "../ui/Toast.jsx";
import { LogsDialog } from "./LogsDialog.jsx";

const PAGE_SIZE = 10;

const KIND_LABEL = {
  database: "Base de datos",
  image: "Imagen",
  repository: "Repositorio",
  compose: "Compose",
};

const STATUS = {
  running: { label: "Corriendo", tone: "ok" },
  stopped: { label: "Apagado", tone: "neutral" },
  error: { label: "Detenido", tone: "danger" },
};

function ResourceIcon({ project }) {
  if (project.kind === "database" && hasBrand(project.engine)) {
    return <BrandIcon slug={project.engine} size={22} />;
  }
  if (project.kind === "image" && hasBrand(project.engine)) {
    return <BrandIcon slug={project.engine} size={22} />;
  }
  if (project.kind === "repository") return <Github size={22} />;
  return <Layers size={22} />;
}

export function Resources() {
  const { refreshToken, refresh } = useApp();
  const toast = useToast();
  const [logsFor, setLogsFor] = useState(null);
  const [deployFor, setDeployFor] = useState(null);
  const [deleteFor, setDeleteFor] = useState(null);
  const [envFor, setEnvFor] = useState(null);
  const [composeFor, setComposeFor] = useState(null);
  const [page, setPage] = useState(0);
  const [busy, setBusy] = useState(null);

  const projects = useAsync(() => api.get("/api/projects"), [refreshToken]);
  usePolling(projects.reload, 15_000);

  const rollback = async (project) => {
    setBusy(`${project.slug}:rollback`);
    try {
      const deployment = await api.post(`/api/projects/${project.slug}/rollback`);
      toast.success(`Rollback de ${project.name}: ${deployment.status}`);
      projects.reload();
    } catch (error) {
      toast.error(error);
    } finally {
      setBusy(null);
    }
  };

  const deploy = async (project, form) => {
    setBusy(`${project.slug}:deploy`);
    try {
      const deployment = await api.post(`/api/projects/${project.slug}/deploy`, form);
      if (deployment.status === "success") toast.success(`${project.name} desplegado.`);
      else toast.error(deployment.message || "El despliegue falló.");
      setDeployFor(null);
      projects.reload();
      refresh();
    } catch (error) {
      toast.error(error);
    } finally {
      setBusy(null);
    }
  };

  const openEnvironment = async (project) => {
    setBusy(`${project.slug}:environment-read`);
    try {
      const result = await api.get(`/api/projects/${project.slug}/environment`);
      setEnvFor({ project, environment: result.environment || "", managedKeys: result.managed_keys || [] });
    } catch (error) { toast.error(error); } finally { setBusy(null); }
  };

  const saveEnvironment = async (project, environment) => {
    setBusy(`${project.slug}:environment`);
    try {
      const result = await api.post(`/api/projects/${project.slug}/environment`, { environment });
      toast.success(result.message || "Variables actualizadas.");
      setEnvFor(null); projects.reload(); refresh();
    } catch (error) { toast.error(error); } finally { setBusy(null); }
  };

  const openCompose = async (project) => {
    setBusy(`${project.slug}:compose-read`);
    try {
      const result = await api.get(`/api/projects/${project.slug}/compose`);
      setComposeFor({ project, compose: result.compose || '' });
    } catch (error) { toast.error(error); } finally { setBusy(null); }
  };

  const saveCompose = async (project, compose) => {
    setBusy(`${project.slug}:compose`);
    try {
      const result = await api.post(`/api/projects/${project.slug}/compose`, { compose });
      toast.success(result.message || 'Compose guardado.');
      setComposeFor(null); projects.reload(); refresh();
    } catch (error) { toast.error(error); } finally { setBusy(null); }
  };

  const remove = async (project, mode) => {
    setBusy(`${project.slug}:delete`);
    try {
      const result = await api.del(
        `/api/projects/${project.slug}`,
        mode === "none" ? {} : { remove: mode },
      );
      toast.success(result.message);
      setDeleteFor(null);
      projects.reload();
      refresh();
    } catch (error) {
      toast.error(error);
    } finally {
      setBusy(null);
    }
  };

  return (
    <>
      <p class="page-intro muted">
        Bases de datos, imágenes, repositorios y Docker Compose gestionados por el panel.
      </p>

      <AsyncBlock
        query={projects}
        empty={
          <Panel>
            <EmptyState
              icon={Boxes}
              title="Sin recursos"
              description="Usa «Añadir recurso» en el encabezado para crear una base de datos, imagen, repositorio o Docker Compose."
            />
          </Panel>
        }
      >
        {(list) => {
          const lastPage = Math.max(0, Math.ceil(list.length / PAGE_SIZE) - 1);
          const currentPage = Math.min(page, lastPage);
          const visible = list.slice(currentPage * PAGE_SIZE, (currentPage + 1) * PAGE_SIZE);
          return <>
            <div class="resource-grid">
              {visible.map((project) => (
              <article class="resource-card" key={project.slug}>
                <header>
                  <div class="resource-icon">
                    <ResourceIcon project={project} />
                  </div>
                  <div class="resource-title">
                    <strong>{project.name}</strong>
                    <span class="muted mono small">{project.slug}</span>
                  </div>
                  <div class="resource-badges">
                    <Badge tone={STATUS[project.runtime_status]?.tone || "neutral"}>
                      {STATUS[project.runtime_status]?.label || "Apagado"}
                    </Badge>
                    <Badge tone="neutral">{KIND_LABEL[project.kind] || project.kind}</Badge>
                  </div>
                </header>

                <dl class="resource-meta">
                  {project.repository ? (
                    <div>
                      <dt>Repositorio</dt>
                      <dd class="mono">
                        {project.repository}
                        {project.branch ? ` · ${project.branch}` : ""}
                      </dd>
                    </div>
                  ) : null}
                  {project.current_image ? (
                    <div>
                      <dt>Imagen</dt>
                      <dd class="mono truncate" title={project.current_image}>
                        {project.current_image}
                      </dd>
                    </div>
                  ) : null}
                  <div>
                    <dt>Creado</dt>
                    <dd>{formatRelative(project.created_at)}</dd>
                  </div>
                  <div>
                    <dt>Último despliegue</dt>
                    <dd>
                      {project.last_deployment ? (
                        <>
                          {formatRelative(project.last_deployment.created_at)}
                          {project.last_deployment.status !== "success" ? (
                            <span class="danger-text"> · falló</span>
                          ) : null}
                        </>
                      ) : (
                        "Todavía no"
                      )}
                    </dd>
                  </div>
                </dl>

                <details class="resource-webhook">
                  <summary>
                    <Webhook size={14} /> Webhook de despliegue
                  </summary>
                  <CopyValue
                    label="URL"
                    value={`${window.location.origin}/hooks/deploy/${project.slug}`}
                  />
                  <CopyValue label="Token" value={project.webhook_token} masked />
                </details>

                {!project.can_rollback && project.rollback_reason ? (
                  <p class="resource-capability-note">
                    <Undo2 size={13} />
                    <span>Rollback no disponible: {project.rollback_reason}</span>
                  </p>
                ) : null}

                <footer class="resource-actions">
                  <Button
                    variant="primary"
                    size="sm"
                    icon={Rocket}
                    loading={busy === `${project.slug}:deploy`}
                    onClick={() => setDeployFor(project)}
                  >
                    {project.runtime_status === "running" ? "Redesplegar" : "Desplegar"}
                  </Button>
                  <Button size="sm" icon={FileText} onClick={() => setLogsFor(project)}>Logs</Button>
                  {project.kind === "compose" ? <Button size="sm" icon={FilePenLine} loading={busy === `${project.slug}:compose-read`} onClick={() => openCompose(project)}>Editar YAML</Button> : null}
                  <Button size="sm" icon={Settings2} loading={busy === `${project.slug}:environment-read`} disabled={!project.env_file} onClick={() => openEnvironment(project)} title={project.env_file ? "Editar variables de entorno" : "Este recurso no tiene .env gestionado"}>Variables</Button>
                  <Button
                    size="sm"
                    icon={Undo2}
                    loading={busy === `${project.slug}:rollback`}
                    disabled={!project.can_rollback}
                    onClick={() => rollback(project)}
                    title={
                      project.can_rollback ? "Volver a la imagen anterior" : project.rollback_reason
                    }
                  >
                    Rollback
                  </Button>
                  <Button
                    size="sm"
                    variant="danger"
                    icon={Trash2}
                    onClick={() => setDeleteFor(project)}
                    aria-label={`Eliminar ${project.name}`}
                  />
                </footer>
              </article>
              ))}
            </div>
            <Pagination page={currentPage} total={list.length} pageSize={PAGE_SIZE} onPageChange={setPage} />
          </>;
        }}
      </AsyncBlock>

      <LogsDialog
        open={Boolean(logsFor)}
        onClose={() => setLogsFor(null)}
        source="project"
        target={logsFor?.slug}
        title={logsFor?.name}
      />

      <DeployDialog
        project={deployFor}
        busy={busy?.endsWith(":deploy")}
        onClose={() => setDeployFor(null)}
        onSubmit={deploy}
      />

      {envFor ? (
        <EnvironmentDialog state={envFor} busy={busy?.endsWith(":environment")} onClose={() => setEnvFor(null)} onSubmit={saveEnvironment} />
      ) : null}

      {composeFor ? (
        <ComposeDialog state={composeFor} busy={busy?.endsWith(":compose")} onClose={() => setComposeFor(null)} onSubmit={saveCompose} />
      ) : null}

      <DeleteDialog
        project={deleteFor}
        busy={busy?.endsWith(":delete")}
        onClose={() => setDeleteFor(null)}
        onConfirm={remove}
      />
    </>
  );
}

function ComposeDialog({ state, busy, onClose, onSubmit }) {
  const [compose, setCompose] = useState(state?.compose || "");
  if (!state) return null;
  return <Modal open onClose={onClose} eyebrow="COMPOSE" title={`Editar YAML de ${state.project.name}`} description="Edita el docker-compose.yml como texto puro. Al guardar se valida; despliega el recurso cuando quieras aplicar los cambios." footer={<>
    <Button onClick={onClose}>Cancelar</Button>
    <Button variant="primary" loading={busy} onClick={() => onSubmit(state.project, compose)}>Guardar YAML</Button>
  </>}>
    <FormGrid columns={1}>
      <Field label="docker-compose.yml" hint="Se valida con Docker Compose antes de guardar." wide>
        <TextArea rows={18} spellcheck={false} value={compose} onInput={(event) => setCompose(event.currentTarget.value)} />
      </Field>
    </FormGrid>
  </Modal>;
}

function DeployDialog({ project, busy, onClose, onSubmit }) {
  const [image, setImage] = useState("");
  const [commit, setCommit] = useState("");
  if (!project) return null;

  const supportsImage = Boolean(project.image_env);

  return (
    <Modal
      open
      onClose={onClose}
      eyebrow="DESPLIEGUE"
      title={`Desplegar ${project.name}`}
      description={
        project.kind === "repository"
          ? `Se traerá la última versión de ${project.branch || "main"} y se reconstruirá la imagen.`
          : supportsImage
            ? "Puedes fijar una imagen concreta; si el despliegue falla se restaura la anterior."
            : "Se volverá a aplicar el Compose actual."
      }
      footer={
        <>
          <Button onClick={onClose}>Cancelar</Button>
          <Button
            variant="primary"
            loading={busy}
            onClick={() => onSubmit(project, { image, commit })}
          >
            Desplegar
          </Button>
        </>
      }
    >
      <FormGrid columns={1}>
        {supportsImage ? (
          <Field
            label="Imagen (opcional)"
            hint="Déjalo vacío para reutilizar la imagen actual."
            wide
          >
            <Input
              placeholder="ghcr.io/usuario/app:sha"
              value={image}
              onInput={(event) => setImage(event.currentTarget.value)}
            />
          </Field>
        ) : null}
        <Field label="Commit (opcional)" hint="Solo se guarda en el historial." wide>
          <Input
            placeholder="a1b2c3d"
            maxLength={64}
            value={commit}
            onInput={(event) => setCommit(event.currentTarget.value)}
          />
        </Field>
      </FormGrid>
    </Modal>
  );
}

function EnvironmentDialog({ state, busy, onClose, onSubmit }) {
  const [environment, setEnvironment] = useState(state?.environment || "");
  if (!state) return null;
  const { project, managedKeys = [] } = state;
  return <Modal open onClose={onClose} eyebrow="CONFIGURACION" title={`Variables de ${project.name}`} description="Edita el archivo .env del recurso. Al guardar, Docker recreara el servicio solo si la configuracion cambio." footer={<>
    <Button onClick={onClose}>Cancelar</Button>
    <Button variant="primary" loading={busy} onClick={() => onSubmit(project, environment)}>Guardar y aplicar</Button>
  </>}>
    <FormGrid columns={1}>
      <Field label="Variables de entorno" hint="Una por linea, CLAVE=valor. Para borrar una variable, elimina su linea." wide>
        <TextArea rows={10} spellcheck={false} placeholder={'NODE_ENV=production\nPORT=3000'} value={environment} onInput={(event) => setEnvironment(event.currentTarget.value)} />
      </Field>
      <div class="environment-note">
        <span>El archivo se mantiene con permisos 0600 y los valores no se escriben en el historial.</span>
        {managedKeys.length ? <span>Gestionadas por Tinkiva y ocultas aqui: <code>{managedKeys.join(", ")}</code>.</span> : null}
        {project.kind === "database" ? <span>En bases de datos, cambiar credenciales no modifica usuarios ya creados dentro de un volumen existente.</span> : null}
      </div>
    </FormGrid>
  </Modal>;
}

function DeleteDialog({ project, busy, onClose, onConfirm }) {
  const [mode, setMode] = useState("stack");
  const [confirmation, setConfirmation] = useState("");
  // El diálogo sigue montado al cerrarse; sin este reset el siguiente recurso
  // heredaría la opción y el texto de confirmación anteriores.
  useEffect(() => {
    setMode("stack");
    setConfirmation("");
  }, [project?.slug]);
  if (!project) return null;

  const needsConfirmation = mode === "all";
  const canDelete = !needsConfirmation || confirmation === project.slug;

  return (
    <Modal
      open
      onClose={onClose}
      eyebrow="ELIMINAR"
      title={`Eliminar ${project.name}`}
      footer={
        <>
          <Button onClick={onClose}>Cancelar</Button>
          <Button
            variant="danger"
            loading={busy}
            disabled={!canDelete}
            onClick={() => onConfirm(project, mode)}
          >
            Eliminar
          </Button>
        </>
      }
    >
      <FormGrid columns={1}>
        <Field label="Qué hacer" wide>
          <Select
            value={mode}
            onChange={(event) => setMode(event.currentTarget.value)}
            options={[
              { value: "none", label: "Solo desregistrar del panel" },
              { value: "stack", label: "Desregistrar y detener los contenedores" },
              { value: "all", label: "Borrar todo: contenedores, volúmenes y archivos" },
            ]}
          />
        </Field>
        {needsConfirmation ? (
          <Field
            label={`Escribe «${project.slug}» para confirmar`}
            hint="Se perderán los datos del volumen de forma irreversible."
            wide
          >
            <Input
              value={confirmation}
              onInput={(event) => setConfirmation(event.currentTarget.value)}
            />
          </Field>
        ) : null}
      </FormGrid>
    </Modal>
  );
}
