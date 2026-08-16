import { useState } from 'preact/hooks';
import { api } from '../lib/api.js';
import { toSlug } from '../lib/format.js';
import { Button } from '../ui/Primitives.jsx';
import { Field, FormGrid, Input, TextArea } from '../ui/Form.jsx';
import { useToast } from '../ui/Toast.jsx';

/** Crea un recurso gestionado a partir de un docker-compose pegado como YAML. */
export function ComposeTextForm({ onCreated }) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const [slugTouched, setSlugTouched] = useState(false);
  const [form, setForm] = useState({ name: '', slug: '', compose: '', watch_image: '' });

  const update = (key) => (event) =>
    setForm((current) => ({ ...current, [key]: event.currentTarget.value }));

  const updateName = (event) => {
    const name = event.currentTarget.value;
    setForm((current) => ({ ...current, name, slug: slugTouched ? current.slug : toSlug(name) }));
  };

  const submit = async (event) => {
    event.preventDefault();
    setBusy(true);
    try {
      const project = await api.post('/api/resources/compose', form);
      onCreated({
        project,
        deployment: null,
        error: 'Compose guardado como recurso. Puedes editarlo o desplegarlo cuando quieras.',
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

  return <form onSubmit={submit} autocomplete="off">
    <p class="muted small">Pega el contenido de tu <code>docker-compose.yml</code>. Se guardará en un recurso propio y podrás editarlo como texto después.</p>
    <FormGrid>
      <Field label="Nombre" hint="Nombre visible del recurso en el panel.">
        <Input value={form.name} onInput={updateName} required maxLength={100} placeholder="Mi aplicación" />
      </Field>
      <Field label="Slug" hint="Minúsculas, números y guiones; se usa para crear la carpeta del recurso.">
        <Input value={form.slug} onInput={(event) => { setSlugTouched(true); update('slug')(event); }} required pattern="[a-z0-9](?:[a-z0-9-]{0,46}[a-z0-9])?" placeholder="mi-aplicacion" />
      </Field>
      <Field
        label="Imagen a vigilar (opcional)"
        hint="Si la indicas, el panel comprueba su digest cada pocos minutos y redespliega cuando cambie. Útil con ECR o GHCR."
        wide
      >
        <Input
          value={form.watch_image}
          onInput={update('watch_image')}
          placeholder="123456789012.dkr.ecr.us-east-1.amazonaws.com/api:latest"
        />
      </Field>
      <Field label="docker-compose.yml" hint="YAML válido para Docker Compose; máximo 128 KiB." wide>
        <TextArea rows={16} spellcheck={false} value={form.compose} onInput={update('compose')} required placeholder={'services:\n  app:\n    image: nginx:alpine\n    ports:\n      - "8080:80"'} />
      </Field>
    </FormGrid>
    <div class="form-actions">
      <Button variant="primary" type="submit" loading={busy}>Guardar recurso</Button>
    </div>
  </form>;
}
