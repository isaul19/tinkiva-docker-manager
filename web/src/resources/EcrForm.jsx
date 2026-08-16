import { useMemo, useState } from 'preact/hooks';
import { ChevronRight, Cloud, Search } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { useAsync } from '../lib/hooks.js';
import { formatBytes, formatRelative, toSlug } from '../lib/format.js';
import { AsyncBlock, Button, EmptyState, Spinner } from '../ui/Primitives.jsx';
import { Field, FormGrid, Input, MemoryUnlimited, Select, TextArea } from '../ui/Form.jsx';
import { useToast } from '../ui/Toast.jsx';

/**
 * Despliegue de una imagen que ya vive en el ECR conectado.
 *
 * A diferencia del formulario de Compose, aquí no se pega YAML: el panel lo
 * genera con el mismo endurecimiento que el resto de recursos. Lo único que se
 * elige es qué imagen y cómo exponerla.
 */
export function EcrForm({ onCreated }) {
  const toast = useToast();
  const { info } = useApp();
  const registry = info?.ecr_registry || '';
  const [repository, setRepository] = useState(null);
  const [tags, setTags] = useState([]);
  const [loadingTags, setLoadingTags] = useState(false);
  const [filter, setFilter] = useState('');
  const [busy, setBusy] = useState(false);
  const [slugTouched, setSlugTouched] = useState(false);
  const [form, setForm] = useState({
    name: '',
    slug: '',
    image: '',
    container_port: '',
    published_port: '',
    memory_mb: '512',
    memory_unlimited: false,
    environment: '',
    external_access: false,
    auto_deploy: true,
  });

  const repositories = useAsync(() => api.get('/api/ecr/repositories'), []);

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
    const list = repositories.data?.repositories || [];
    const needle = filter.trim().toLowerCase();
    return needle ? list.filter((name) => name.toLowerCase().includes(needle)) : list;
  }, [repositories.data, filter]);

  const pick = async (name) => {
    setRepository(name);
    const shortName = name.split('/').pop();
    setForm((current) => ({
      ...current,
      name: current.name || shortName,
      slug: slugTouched ? current.slug : toSlug(shortName),
    }));
    setLoadingTags(true);
    try {
      const result = await api.get('/api/ecr/repositories', { repository: name });
      setTags(result.tags);
      if (result.tags.length) {
        setForm((current) => ({ ...current, image: result.tags[0].image }));
      }
    } catch (error) {
      toast.error(error);
      setTags([]);
    } finally {
      setLoadingTags(false);
    }
  };

  const submit = async (event) => {
    event.preventDefault();
    setBusy(true);
    try {
      onCreated(await api.post('/api/resources/ecr', form));
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
          query={repositories}
          empty={
            <EmptyState
              icon={Cloud}
              title="El registro no tiene repositorios"
              description="Sube una imagen desde tu CI, o revisa que la clave IAM pueda hacer ecr:DescribeRepositories."
            />
          }
        >
          {(data) => (
            <>
              <Field
                label="Buscar repositorio"
                hint={registry ? `Repositorios de ${registry}.` : 'Los repositorios del registro conectado.'}
              >
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

              <ul class="repo-results">
                {visible.map((name) => (
                  <li key={name}>
                    <button type="button" onClick={() => pick(name)}>
                      <Cloud size={18} class="muted" />
                      <span class="compact-main">
                        <strong class="mono">{name}</strong>
                        <span class="muted small truncate">
                          Ver etiquetas publicadas
                        </span>
                      </span>
                      <ChevronRight size={16} class="muted" />
                    </button>
                  </li>
                ))}
              </ul>

              {visible.length === 0 && data.repositories.length ? (
                <p class="muted small">Ningún repositorio coincide con «{filter}».</p>
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
        <Cloud size={20} />
        <code>{registry ? `${registry}/${repository}` : repository}</code>
        <Button size="sm" onClick={() => setRepository(null)}>
          Cambiar repositorio
        </Button>
      </div>

      {loadingTags ? <Spinner label="Leyendo etiquetas…" /> : null}

      {!loadingTags && tags.length === 0 ? (
        <EmptyState
          icon={Cloud}
          title="Este repositorio aún no tiene imágenes etiquetadas"
          description="Cuando tu CI publique la primera, aparecerá aquí. También puedes elegir otro repositorio."
        />
      ) : null}

      <FormGrid>
        {tags.length ? (
          <Field
            label="Etiqueta"
            hint="La más reciente va primero. Es la que el panel vigilará."
            wide
          >
            <Select
              value={form.image}
              onChange={update('image')}
              options={tags.map((entry) => ({
                value: entry.image,
                label: `${entry.tag} — ${formatRelative(entry.pushed_at)} · ${formatBytes(entry.size_bytes)}`,
              }))}
            />
          </Field>
        ) : null}

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

        <Field label="Puerto del contenedor" hint="El que expone tu imagen. Vacío = no se publica nada.">
          <Input
            type="number"
            min="1"
            max="65535"
            value={form.container_port}
            onInput={update('container_port')}
            placeholder="8080"
          />
        </Field>
        <Field label="Puerto del VPS" hint="Vacío = usa el mismo puerto que el contenedor.">
          <Input
            type="number"
            min="1"
            max="65535"
            value={form.published_port}
            onInput={update('published_port')}
            placeholder="8080"
          />
        </Field>

        <Field
          label="RAM máxima (MB)"
          hint={form.memory_unlimited ? 'Sin límite: este campo no se aplica.' : 'Entre 64 y 16384 MB para el contenedor.'}
        >
          <Input
            type="number"
            min="64"
            max="16384"
            value={form.memory_mb}
            onInput={update('memory_mb')}
            disabled={form.memory_unlimited}
          />
        </Field>
        <MemoryUnlimited
          checked={form.memory_unlimited}
          onChange={(checked) => setForm((current) => ({ ...current, memory_unlimited: checked }))}
        />

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
            placeholder={'NODE_ENV=production\nPORT=8080'}
          />
        </Field>
      </FormGrid>

      <label class="inline-field checkbox">
        <input
          type="checkbox"
          checked={form.auto_deploy}
          onChange={(event) =>
            setForm((current) => ({ ...current, auto_deploy: event.currentTarget.checked }))
          }
        />
        <span>
          <strong>Redesplegar cuando tu CI suba esta etiqueta</strong>
          <span class="field-hint">
            El panel compara el digest cada pocos minutos y renueva el acceso a AWS solo. Con una
            etiqueta fija como <code>latest</code> es lo que hace que cada push llegue al servidor.
          </span>
        </span>
      </label>

      <div class="form-actions">
        <Button variant="primary" type="submit" loading={busy} disabled={!form.image}>
          Crear y desplegar
        </Button>
      </div>
    </form>
  );
}
