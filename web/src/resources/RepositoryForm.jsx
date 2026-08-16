import { useEffect, useMemo, useState } from 'preact/hooks';
import { GitBranch, Lock, Search } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useAsync } from '../lib/hooks.js';
import { formatIsoRelative, toSlug } from '../lib/format.js';
import { BrandIcon } from '../ui/BrandIcon.jsx';
import { AsyncBlock, Button, EmptyState, Spinner } from '../ui/Primitives.jsx';
import { Field, FormGrid, Input, MemoryUnlimited, Select, TextArea } from '../ui/Form.jsx';
import { useToast } from '../ui/Toast.jsx';

export function RepositoryForm({ onCreated }) {
  const toast = useToast();
  const [installationId, setInstallationId] = useState(null);
  const [repository, setRepository] = useState(null);
  const [filter, setFilter] = useState('');
  const [branches, setBranches] = useState([]);
  const [busy, setBusy] = useState(false);
  const [slugTouched, setSlugTouched] = useState(false);
  const [form, setForm] = useState({
    name: '',
    slug: '',
    branch: 'main',
    build_mode: 'auto',
    dockerfile: 'Dockerfile',
    build_context: '.',
    container_port: '',
    published_port: '',
    memory_mb: '512',
    memory_unlimited: false,
    environment: '',
    external_access: false,
    auto_deploy: true,
  });

  const installations = useAsync(() => api.get('/api/github/installations'), []);

  useEffect(() => {
    if (installationId === null && installations.data?.length) {
      setInstallationId(installations.data[0].id);
    }
  }, [installations.data, installationId]);

  const repositories = useAsync(
    () =>
      installationId
        ? api.get('/api/github/repositories', { installation_id: installationId })
        : Promise.resolve([]),
    [installationId],
  );

  const update = (key) => (event) =>
    setForm((current) => ({ ...current, [key]: event.currentTarget.value }));

  const onName = (event) => {
    const value = event.currentTarget.value;
    setForm((current) => ({
      ...current,
      name: value,
      slug: slugTouched ? current.slug : toSlug(value),
    }));
  };

  const visible = useMemo(() => {
    const list = repositories.data || [];
    const needle = filter.trim().toLowerCase();
    return needle ? list.filter((item) => item.full_name.toLowerCase().includes(needle)) : list;
  }, [repositories.data, filter]);

  const pick = async (item) => {
    setRepository(item);
    const shortName = item.full_name.split('/').pop();
    setForm((current) => ({
      ...current,
      name: current.name || shortName,
      slug: slugTouched ? current.slug : toSlug(shortName),
      branch: item.default_branch || 'main',
    }));
    try {
      const list = await api.get('/api/github/branches', {
        installation_id: installationId,
        repository: item.full_name,
      });
      setBranches(list);
    } catch {
      setBranches([item.default_branch || 'main']);
    }
  };

  const submit = async (event) => {
    event.preventDefault();
    setBusy(true);
    try {
      const result = await api.post('/api/resources/repository', {
        ...form,
        repository: repository.full_name,
        installation_id: String(installationId),
      });
      onCreated(result);
    } catch (error) {
      toast.error(error);
    } finally {
      setBusy(false);
    }
  };

  if (!repository) {
    return (
      <div class="repo-picker">
        <AsyncBlock
          query={installations}
          empty={
            <EmptyState
              icon={GitBranch}
              title="La GitHub App no está instalada en ninguna cuenta"
              description="Ve a la sección GitHub e instálala en los repositorios que quieras desplegar."
            />
          }
        >
          {(list) => (
            <>
              {list.length > 1 ? (
                <Field label="Cuenta" hint="Cuenta de GitHub donde está instalado el acceso de Tinkiva.">
                  <Select
                    value={String(installationId ?? '')}
                    onChange={(event) => {
                      setInstallationId(Number(event.currentTarget.value));
                      setFilter('');
                    }}
                    options={list.map((item) => ({
                      value: String(item.id),
                      label: `${item.login} (${item.type})`,
                    }))}
                  />
                </Field>
              ) : null}

              <Field label="Buscar repositorio" hint="Escribe parte del propietario o nombre del repositorio.">
                <div class="input-with-icon">
                  <Search size={16} />
                  <input
                    class="input"
                    type="search"
                    placeholder="Filtrar repositorios"
                    value={filter}
                    onInput={(event) => setFilter(event.currentTarget.value)}
                  />
                </div>
              </Field>

              {repositories.loading ? <Spinner label="Leyendo repositorios…" /> : null}
              {repositories.error ? (
                <p class="field-message">{repositories.error.message}</p>
              ) : null}

              <ul class="repo-results">
                {visible.map((item) => (
                  <li key={item.full_name}>
                    <button type="button" onClick={() => pick(item)}>
                      <BrandIcon slug="github" size={18} color="#c8d0da" />
                      <span class="compact-main">
                        <strong class="mono">{item.full_name}</strong>
                        <span class="muted small truncate">
                          {item.description || 'Sin descripción'}
                        </span>
                      </span>
                      {item.language ? <span class="chip">{item.language}</span> : null}
                      {item.private ? <Lock size={13} class="muted" /> : null}
                      <span class="muted small">{formatIsoRelative(item.updated_at)}</span>
                    </button>
                  </li>
                ))}
              </ul>

              {!repositories.loading && visible.length === 0 ? (
                <p class="muted small">
                  Ningún repositorio coincide. Recuerda que la App solo ve aquellos donde la
                  instalaste.
                </p>
              ) : null}
            </>
          )}
        </AsyncBlock>
      </div>
    );
  }

  return (
    <form onSubmit={submit} autocomplete="off">
      <div class="selected-image">
        <BrandIcon slug="github" size={20} color="#f4f6f8" />
        <code>{repository.full_name}</code>
        <Button size="sm" onClick={() => setRepository(null)}>
          Cambiar repositorio
        </Button>
      </div>

      <FormGrid>
        <Field label="Nombre" hint="Nombre visible del recurso en el panel.">
          <Input value={form.name} onInput={onName} required maxLength={100} />
        </Field>
        <Field label="Slug" hint="Se genera desde el nombre; puedes editarlo si lo necesitas.">
          <Input
            value={form.slug}
            onInput={(event) => {
              setSlugTouched(true);
              update('slug')(event);
            }}
            required
            pattern="[a-z0-9](?:[a-z0-9-]{0,46}[a-z0-9])?"
          />
        </Field>

        <Field label="Rama" hint="Cada push a esta rama redespliega el recurso.">
          {branches.length ? (
            <Select
              value={form.branch}
              onChange={update('branch')}
              options={branches.map((branch) => ({ value: branch, label: branch }))}
            />
          ) : (
            <Input value={form.branch} onInput={update('branch')} required />
          )}
        </Field>
        <Field
          label="RAM máxima (MB)"
          hint={form.memory_unlimited ? 'Sin límite: este campo no se aplica.' : 'Entre 64 y 16384 MB para el contenedor.'}
        >
          <Input type="number" min="64" max="16384" value={form.memory_mb} onInput={update('memory_mb')} disabled={form.memory_unlimited} />
        </Field>
        <MemoryUnlimited
          checked={form.memory_unlimited}
          onChange={(checked) => setForm((current) => ({ ...current, memory_unlimited: checked }))}
        />

        <Field label="Tipo de aplicación" hint="Auto detecta Dockerfile, Node, Python o un sitio estático.">
          <Select
            value={form.build_mode}
            onChange={update('build_mode')}
            options={[
              { value: 'auto', label: 'Detectar automáticamente' },
              { value: 'dockerfile', label: 'Usar Dockerfile del repositorio' },
            ]}
          />
        </Field>
        <Field label="Contexto de build" hint="Carpeta de la app dentro del repo; «.» usa la raíz.">
          <Input value={form.build_context} onInput={update('build_context')} required />
        </Field>

        {form.build_mode === 'dockerfile' ? (
          <Field label="Dockerfile" hint="Relativo al contexto.">
            <Input value={form.dockerfile} onInput={update('dockerfile')} required />
          </Field>
        ) : null}

        <Field label="Puerto del contenedor" hint="Vacío = se detecta desde EXPOSE o usa 80/3000/8000 según el runtime.">
          <Input
            type="number"
            min="1"
            max="65535"
            value={form.container_port}
            onInput={update('container_port')}
            placeholder="3000"
          />
        </Field>
        <Field label="Puerto del VPS" hint="Vacío = usa el mismo puerto que el contenedor.">
          <Input
            type="number"
            min="1"
            max="65535"
            value={form.published_port}
            onInput={update('published_port')}
            placeholder="3000"
          />
        </Field>

        <label class={`exposure-option field-wide${form.external_access ? ' is-public' : ''}`}>
          <input
            type="checkbox"
            checked={form.external_access}
            onChange={(event) =>
              setForm((current) => ({ ...current, external_access: event.currentTarget.checked }))
            }
          />
          <span class="exposure-copy">
            <strong>Permitir acceso externo al VPS</strong>
            <span class="field-hint">
              {form.external_access
                ? 'Escucha en 0.0.0.0. Debes permitir este puerto en el firewall o Security Group.'
                : 'Escucha en 127.0.0.1. Solo el VPS, un proxy local o un túnel SSH pueden acceder.'}
            </span>
          </span>
        </label>

        <Field label="Variables de entorno" hint="Una por línea, CLAVE=valor." wide>
          <TextArea
            rows={4}
            value={form.environment}
            onInput={update('environment')}
            placeholder={'NODE_ENV=production\nPORT=3000'}
          />
        </Field>
      </FormGrid>

      <p class="muted small">
        El primer despliegue clona el repositorio y construye la imagen en el servidor; en máquinas
        pequeñas puede tardar varios minutos.
      </p>

      <label class="inline-field checkbox">
        <input
          type="checkbox"
          checked={form.auto_deploy}
          onChange={(event) =>
            setForm((current) => ({ ...current, auto_deploy: event.currentTarget.checked }))
          }
        />
        <span>
          Auto Deploy por polling de GitHub
          <span class="field-hint">Revisa la rama elegida y redespliega cuando detecta un commit nuevo.</span>
        </span>
      </label>

      <div class="form-actions">
        <Button variant="primary" type="submit" loading={busy}>
          Clonar, construir y desplegar
        </Button>
      </div>
    </form>
  );
}
