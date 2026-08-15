// Logos de marca. Se importan uno a uno desde simple-icons para que esbuild
// descarte los ~3.370 restantes del paquete en el tree-shaking.

import {
  siBitwarden,
  siDocker,
  siGithub,
  siGrafana,
  siMariadb,
  siMeilisearch,
  siMetabase,
  siMinio,
  siMongodb,
  siMysql,
  siN8n,
  siNginx,
  siPortainer,
  siPostgresql,
  siPreact,
  siRabbitmq,
  siRedis,
  siRust,
  siTraefikproxy,
  siUptimekuma,
} from 'simple-icons';

const BRANDS = {
  bitwarden: siBitwarden,
  docker: siDocker,
  github: siGithub,
  grafana: siGrafana,
  mariadb: siMariadb,
  meilisearch: siMeilisearch,
  metabase: siMetabase,
  minio: siMinio,
  mongodb: siMongodb,
  mysql: siMysql,
  n8n: siN8n,
  nginx: siNginx,
  portainer: siPortainer,
  postgresql: siPostgresql,
  preact: siPreact,
  rabbitmq: siRabbitmq,
  redis: siRedis,
  rust: siRust,
  traefikproxy: siTraefikproxy,
  uptimekuma: siUptimekuma,
};

export function hasBrand(slug) {
  return Boolean(BRANDS[slug]);
}

export function brandColor(slug, fallback = 'currentColor') {
  const icon = BRANDS[slug];
  return icon ? `#${icon.hex}` : fallback;
}

/**
 * @param {{slug: string, size?: number, color?: string, title?: string}} props
 */
export function BrandIcon({ slug, size = 20, color, title }) {
  const icon = BRANDS[slug];
  if (!icon) return null;
  return (
    <svg
      class="brand-icon"
      viewBox="0 0 24 24"
      width={size}
      height={size}
      role={title ? 'img' : 'presentation'}
      aria-hidden={title ? undefined : 'true'}
      fill={color || `#${icon.hex}`}
    >
      {title ? <title>{title}</title> : null}
      <path d={icon.path} />
    </svg>
  );
}
