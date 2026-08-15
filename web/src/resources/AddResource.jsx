import { useEffect, useState } from 'preact/hooks';
import { ArrowLeft, Database, Github, Layers } from 'lucide-preact';
import { useApp } from '../lib/context.js';
import { BrandIcon } from '../ui/BrandIcon.jsx';
import { Modal } from '../ui/Modal.jsx';
import { Button } from '../ui/Primitives.jsx';
import { DatabaseForm } from './DatabaseForm.jsx';
import { ImageForm } from './ImageForm.jsx';
import { RepositoryForm } from './RepositoryForm.jsx';
import { ComposeForm } from './ComposeForm.jsx';
import { ResultPanel } from './ResultPanel.jsx';

const TYPES = [
  {
    id: 'database',
    title: 'Base de datos',
    description: 'PostgreSQL, MySQL, MariaDB, MongoDB o Redis con volumen y healthcheck.',
    icon: <Database size={24} />,
    form: DatabaseForm,
  },
  {
    id: 'image',
    title: 'Imagen de Docker Hub',
    description: 'Busca la imagen, elige la etiqueta y el panel escribe el Compose.',
    icon: <BrandIcon slug="docker" size={26} />,
    form: ImageForm,
  },
  {
    id: 'repository',
    title: 'Repositorio de GitHub',
    description: 'Clona, construye la imagen y redespliega en cada push.',
    icon: <BrandIcon slug="github" size={24} color="#f4f6f8" />,
    form: RepositoryForm,
  },
  {
    id: 'compose',
    title: 'Compose existente',
    description: 'Registra un compose.yaml que ya está en la raíz permitida.',
    icon: <Layers size={24} />,
    form: ComposeForm,
  },
];

export function AddResource({ open, onClose, onCreated }) {
  const { capabilities, info } = useApp();
  const [type, setType] = useState(null);
  const [result, setResult] = useState(null);

  useEffect(() => {
    if (!open) {
      setType(null);
      setResult(null);
    }
  }, [open]);

  const selected = TYPES.find((entry) => entry.id === type);
  const Form = selected?.form;

  const disabledReason = (id) => {
    if (id === 'image' && !capabilities.curl) return 'Requiere curl en el servidor';
    if (id === 'repository' && !capabilities.git) return 'Requiere git en el servidor';
    if (id === 'repository' && !info?.github_connected) return 'Conecta GitHub primero';
    return null;
  };

  const title = result
    ? 'Recurso creado'
    : selected
      ? selected.title
      : 'Añadir recurso';

  return (
    <Modal
      open={open}
      onClose={onClose}
      eyebrow="NUEVO"
      title={title}
      description={result ? null : selected?.description || 'Elige qué quieres desplegar.'}
      size={result || selected ? 'lg' : 'md'}
      footer={
        result ? (
          <Button variant="primary" onClick={onCreated}>
            Listo
          </Button>
        ) : selected ? (
          <Button icon={ArrowLeft} onClick={() => setType(null)}>
            Cambiar tipo
          </Button>
        ) : null
      }
    >
      {result ? (
        <ResultPanel result={result} />
      ) : selected ? (
        <Form onCreated={setResult} />
      ) : (
        <div class="type-grid">
          {TYPES.map((entry) => {
            const reason = disabledReason(entry.id);
            return (
              <button
                key={entry.id}
                type="button"
                class="type-card"
                disabled={Boolean(reason)}
                onClick={() => setType(entry.id)}
              >
                <span class="type-icon">{entry.icon}</span>
                <strong>{entry.title}</strong>
                <span class="muted small">{reason || entry.description}</span>
              </button>
            );
          })}
        </div>
      )}
    </Modal>
  );
}
