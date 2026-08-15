import { useEffect, useState } from 'preact/hooks';
import { Search, Star } from 'lucide-preact';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { useDebounced } from '../lib/hooks.js';
import { formatCompact, toSlug } from '../lib/format.js';
import { BrandIcon, hasBrand } from '../ui/BrandIcon.jsx';
import { Button, Spinner } from '../ui/Primitives.jsx';
import { Field, FormGrid, Input, Select, TextArea } from '../ui/Form.jsx';
import { useToast } from '../ui/Toast.jsx';

/** Buscador de Docker Hub con sugerencias mientras no se escribe nada. */
function ImagePicker({ onPick }) {
  const { catalog } = useApp();
  const [query, setQuery] = useState('');
  const debounced = useDebounced(query, 350);
  const [results, setResults] = useState(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);

  useEffect(() => {
    let cancelled = false;
    if (debounced.trim().length < 2) {
      setResults(null);
      setError(null);
      return () => {
        cancelled = true;
      };
    }
    setLoading(true);
    api
      .get('/api/registry/search', { q: debounced })
      .then((data) => {
        if (!cancelled) {
          setResults(data);
          setError(null);
        }
      })
      .catch((cause) => {
        if (!cancelled) setError(cause);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [debounced]);

  const popular = catalog?.popular_images || [];
  const showing = results || popular;

  return (
    <div class="image-picker">
      <div class="input-with-icon">
        <Search size={16} />
        <input
          class="input"
          type="search"
          placeholder="Busca en Docker Hub: nginx, n8n, minio…"
          value={query}
          onInput={(event) => setQuery(event.currentTarget.value)}
          autofocus
        />
      </div>

      {loading ? <Spinner label="Buscando en Docker Hub…" /> : null}
      {error ? <p class="field-message">{error.message}</p> : null}

      {debounced.includes('/') && !debounced.includes(' ') ? (
        <Button onClick={() => onPick(debounced.trim())}>
          Usar referencia exacta: {debounced.trim()}
        </Button>
      ) : null}

      {!loading ? (
        <>
          {!results ? <p class="picker-hint muted small">Sugerencias populares</p> : null}
          <ul class="image-results">
            {showing.map((item) => (
              <li key={item.name}>
                <button type="button" onClick={() => onPick(item.name)}>
                  <span class="image-logo">
                    {hasBrand(item.icon) ? (
                      <BrandIcon slug={item.icon} size={20} />
                    ) : (
                      <BrandIcon slug="docker" size={20} />
                    )}
                  </span>
                  <span class="compact-main">
                    <strong class="mono">{item.name}</strong>
                    <span class="muted small truncate">{item.description || 'Sin descripción'}</span>
                  </span>
                  {item.official ? <span class="chip">oficial</span> : null}
                  {item.stars ? (
                    <span class="muted small stars">
                      <Star size={12} /> {formatCompact(item.stars)}
                    </span>
                  ) : null}
                </button>
              </li>
            ))}
          </ul>
          {results && results.length === 0 ? (
            <p class="muted small">Docker Hub no devolvió resultados para «{debounced}».</p>
          ) : null}
        </>
      ) : null}
    </div>
  );
}

export function ImageForm({ onCreated }) {
  const toast = useToast();
  const [repository, setRepository] = useState(null);
  const [tags, setTags] = useState([]);
  const [loadingTags, setLoadingTags] = useState(false);
  const [slugTouched, setSlugTouched] = useState(false);
  const [busy, setBusy] = useState(false);
  const [form, setForm] = useState({
    name: '',
    slug: '',
    tag: 'latest',
    container_port: '',
    published_port: '',
    memory_mb: '512',
    volume_path: '',
    environment: '',
    auto_deploy: true,
  });

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

  const pick = async (name) => {
    const lastSlash = name.lastIndexOf('/');
    const lastColon = name.lastIndexOf(':');
    const hasTag = lastColon > lastSlash;
    const imageName = hasTag ? name.slice(0, lastColon) : name;
    const requestedTag = hasTag ? name.slice(lastColon + 1) : null;
    setRepository(imageName);
    const shortName = imageName.split('/').pop();
    setForm((current) => ({
      ...current,
      name: current.name || shortName,
      slug: slugTouched ? current.slug : toSlug(shortName),
      tag: requestedTag || current.tag,
    }));

    const firstSegment = imageName.split('/')[0];
    const externalRegistry = firstSegment.includes('.') || firstSegment.includes(':');
    if (externalRegistry) {
      setTags([]);
      return;
    }

    setLoadingTags(true);
    try {
      const data = await api.get('/api/registry/tags', { image: imageName });
      setTags(data.tags || []);
      const preferred = (data.tags || []).find((tag) => tag.name === 'latest') || data.tags?.[0];
      if (preferred) setForm((current) => ({ ...current, tag: preferred.name }));
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
      const result = await api.post('/api/resources/image', {
        ...form,
        image: `${repository}:${form.tag}`,
      });
      onCreated(result);
    } catch (error) {
      toast.error(error);
    } finally {
      setBusy(false);
    }
  };

  if (!repository) return <ImagePicker onPick={pick} />;

  return (
    <form onSubmit={submit} autocomplete="off">
      <div class="selected-image">
        <BrandIcon slug="docker" size={22} />
        <code>
          {repository}:{form.tag}
        </code>
        <Button size="sm" onClick={() => setRepository(null)}>
          Cambiar imagen
        </Button>
      </div>

      <FormGrid>
        <Field label="Nombre">
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

        <Field label="Etiqueta" hint={loadingTags ? 'Cargando etiquetas…' : `${tags.length} disponibles`}>
          {tags.length ? (
            <Select
              value={form.tag}
              onChange={update('tag')}
              options={tags.map((tag) => ({ value: tag.name, label: tag.name }))}
            />
          ) : (
            <Input value={form.tag} onInput={update('tag')} required />
          )}
        </Field>
        <Field label="RAM máxima (MB)">
          <Input type="number" min="64" max="16384" value={form.memory_mb} onInput={update('memory_mb')} />
        </Field>

        <Field label="Puerto del contenedor" hint="El que expone la imagen.">
          <Input
            type="number"
            min="1"
            max="65535"
            value={form.container_port}
            onInput={update('container_port')}
            placeholder="80"
          />
        </Field>
        <Field label="Puerto local" hint="Se publica solo en 127.0.0.1.">
          <Input
            type="number"
            min="1"
            max="65535"
            value={form.published_port}
            onInput={update('published_port')}
            placeholder="8080"
          />
        </Field>

        <Field label="Volumen persistente" hint="Ruta dentro del contenedor, p. ej. /data." wide>
          <Input value={form.volume_path} onInput={update('volume_path')} placeholder="/data" />
        </Field>

        <Field label="Variables de entorno" hint="Una por línea, CLAVE=valor." wide>
          <TextArea
            rows={4}
            value={form.environment}
            onInput={update('environment')}
            placeholder={'LOG_LEVEL=info\nTZ=America/Lima'}
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
        Auto Deploy cuando cambie el digest de la imagen
      </label>

      <div class="form-actions">
        <Button variant="primary" type="submit" loading={busy}>
          Crear y desplegar
        </Button>
      </div>
    </form>
  );
}
