import { useMemo, useState } from 'preact/hooks';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { toSlug } from '../lib/format.js';
import { BrandIcon } from '../ui/BrandIcon.jsx';
import { Button } from '../ui/Primitives.jsx';
import { Field, FormGrid, Input, MemoryUnlimited } from '../ui/Form.jsx';
import { useToast } from '../ui/Toast.jsx';

export function DatabaseForm({ onCreated }) {
  const { catalog } = useApp();
  const toast = useToast();
  const engines = catalog?.engines || [];

  const [engineId, setEngineId] = useState(engines[0]?.id || 'postgres');
  const [name, setName] = useState('');
  const [slugTouched, setSlugTouched] = useState(false);
  const [form, setForm] = useState({
    slug: '',
    database: 'app',
    username: 'app',
    password: '',
    published_port: '',
    external_access: false,
    memory_mb: '',
    memory_unlimited: false,
  });
  const [busy, setBusy] = useState(false);

  const engine = useMemo(
    () => engines.find((item) => item.id === engineId) || engines[0],
    [engines, engineId],
  );

  const update = (key) => (event) =>
    setForm((current) => ({ ...current, [key]: event.currentTarget.value }));

  const onName = (event) => {
    const value = event.currentTarget.value;
    setName(value);
    if (!slugTouched) setForm((current) => ({ ...current, slug: toSlug(value) }));
  };

  const submit = async (event) => {
    event.preventDefault();
    setBusy(true);
    try {
      const result = await api.post('/api/resources/database', {
        engine: engineId,
        name,
        slug: form.slug,
        database: engine.needs_database ? form.database : '',
        username: engine.needs_username ? form.username : '',
        password: form.password,
        published_port: form.published_port,
        external_access: form.external_access,
        memory_mb: form.memory_mb || String(engine.default_memory_mb),
        memory_unlimited: form.memory_unlimited,
      });
      onCreated(result);
    } catch (error) {
      toast.error(error);
    } finally {
      setBusy(false);
    }
  };

  if (!engine) return <p class="muted">Cargando catálogo…</p>;

  return (
    <form onSubmit={submit} autocomplete="off">
      <div class="engine-grid">
        {engines.map((item) => (
          <button
            key={item.id}
            type="button"
            class={`engine-card${item.id === engineId ? ' selected' : ''}`}
            style={`--engine-accent:${item.accent}`}
            onClick={() => setEngineId(item.id)}
          >
            <BrandIcon slug={item.icon} size={26} />
            <strong>{item.label}</strong>
            <span class="muted small">{item.description}</span>
            <code class="small">{item.image}</code>
          </button>
        ))}
      </div>

      <FormGrid>
        <Field label="Nombre" hint="Como lo verás en el panel.">
          <Input value={name} onInput={onName} required maxLength={100} placeholder={`${engine.label} de Storagia`} />
        </Field>
        <Field label="Slug" hint="Minúsculas, números y guiones.">
          <Input
            value={form.slug}
            onInput={(event) => {
              setSlugTouched(true);
              update('slug')(event);
            }}
            required
            pattern="[a-z0-9](?:[a-z0-9-]{0,46}[a-z0-9])?"
            placeholder="storagia-db"
          />
        </Field>

        {engine.needs_database ? (
          <Field label="Base de datos" hint="Nombre interno; usa letras, números o guion bajo.">
            <Input value={form.database} onInput={update('database')} required />
          </Field>
        ) : null}
        {engine.needs_username ? (
          <Field label="Usuario" hint="Cuenta que usará la aplicación para conectarse.">
            <Input value={form.username} onInput={update('username')} required />
          </Field>
        ) : null}

        <Field
          label="Contraseña"
          wide={!engine.needs_username}
          hint="Vacío = se genera una de 48 caracteres."
        >
          <Input type="password" value={form.password} onInput={update('password')} minLength={12} />
        </Field>

        <Field
          label="RAM máxima (MB)"
          hint={
            form.memory_unlimited
              ? 'Sin límite: este campo no se aplica.'
              : `Entre 64 y 16384 MB. Vacío = ${engine.default_memory_mb} MB.`
          }
        >
          <Input
            type="number"
            min="64"
            max="16384"
            value={form.memory_mb}
            onInput={update('memory_mb')}
            placeholder={String(engine.default_memory_mb)}
            disabled={form.memory_unlimited}
          />
        </Field>
        <MemoryUnlimited
          checked={form.memory_unlimited}
          onChange={(checked) => setForm((current) => ({ ...current, memory_unlimited: checked }))}
          engine={engine.label}
        />
        <Field
          label="Puerto local (opcional)"
          hint={`Interno: ${engine.port}. Se limita al VPS salvo que permitas acceso externo.`}
        >
          <Input
            type="number"
            min="1"
            max="65535"
            value={form.published_port}
            onInput={update('published_port')}
            placeholder={String(engine.port)}
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
                ? 'La base de datos escuchará en 0.0.0.0. Protege el puerto con firewall, Security Group o una red privada.'
                : 'La base de datos escuchará en 127.0.0.1. Solo el VPS, un proxy local o un túnel SSH podrán conectarse.'}
            </span>
          </span>
        </label>
      </FormGrid>

      <div class="form-actions">
        <Button variant="primary" type="submit" loading={busy}>
          Crear y desplegar
        </Button>
      </div>
    </form>
  );
}
