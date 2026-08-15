import { useState } from 'preact/hooks';
import { api } from '../lib/api.js';
import { useApp } from '../lib/context.js';
import { toSlug } from '../lib/format.js';
import { Button } from '../ui/Primitives.jsx';
import { Field, FormGrid, Input } from '../ui/Form.jsx';
import { useToast } from '../ui/Toast.jsx';

/** Alta de un stack Compose que ya existe en disco bajo la raíz permitida. */
export function ComposeForm({ onCreated }) {
  const { allowedRoot } = useApp();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const [slugTouched, setSlugTouched] = useState(false);
  const [form, setForm] = useState({
    name: '',
    slug: '',
    compose_file: '',
    env_file: '',
    image_env: '',
    branch: '',
    webhook_token: '',
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

  const submit = async (event) => {
    event.preventDefault();
    setBusy(true);
    try {
      const project = await api.post('/api/projects', form);
      onCreated({
        project,
        deployment: null,
        error: 'Proyecto registrado. Usa «Desplegar» cuando quieras aplicarlo.',
        connection_uri: '',
        host: '',
        password: null,
        published_port: null,
      });
    } catch (error) {
      toast.error(error);
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} autocomplete="off">
      <p class="muted">
        Los archivos deben existir ya bajo <code>{allowedRoot || '/opt/tinkiva/apps'}</code>. Las
        rutas pueden ser relativas a esa raíz.
      </p>

      <FormGrid>
        <Field label="Nombre">
          <Input value={form.name} onInput={onName} required maxLength={100} placeholder="Storagia API" />
        </Field>
        <Field label="Slug">
          <Input
            value={form.slug}
            onInput={(event) => {
              setSlugTouched(true);
              update('slug')(event);
            }}
            required
            pattern="[a-z0-9](?:[a-z0-9-]{0,46}[a-z0-9])?"
            placeholder="storagia-api"
          />
        </Field>

        <Field label="Archivo Compose" wide>
          <Input
            value={form.compose_file}
            onInput={update('compose_file')}
            required
            placeholder="storagia/compose.yaml"
          />
        </Field>
        <Field label="Archivo .env (opcional)" wide>
          <Input value={form.env_file} onInput={update('env_file')} placeholder="storagia/.env" />
        </Field>

        <Field label="Variable de imagen" hint="Necesaria para desplegar por imagen y hacer rollback.">
          <Input value={form.image_env} onInput={update('image_env')} placeholder="APP_IMAGE" />
        </Field>
        <Field label="Rama permitida" hint="Restringe qué rama puede desplegar el webhook.">
          <Input value={form.branch} onInput={update('branch')} placeholder="main" />
        </Field>

        <Field label="Token de webhook" hint="Vacío = se genera automáticamente." wide>
          <Input value={form.webhook_token} onInput={update('webhook_token')} />
        </Field>
      </FormGrid>

      <div class="form-actions">
        <Button variant="primary" type="submit" loading={busy}>
          Registrar proyecto
        </Button>
      </div>
    </form>
  );
}
