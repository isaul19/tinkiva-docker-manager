# Validación de la edición TinkivaCreateApp

## Interfaz

```bash
cd web
npm ci
npm test
```

Las pruebas montan el bundle real, verifican CPU/RAM/disco, contenedores y logs, y confirman que
no aparecen controles de despliegue, GitHub, ECR ni acciones sobre Docker.

## Rust

En Linux:

```bash
cargo fmt --check
cargo clippy --all-targets
cargo test
cargo build --release
```

## Smoke test

```bash
./scripts/smoke-test.sh target/release/tmanager
```

El smoke test usa `tests/mock-docker.sh`, valida autenticación, métricas, inventario y logs, y
exige `404` para las rutas históricas de CI/CD y escritura.

## Revisión manual

- Abrir Resumen y confirmar CPU, RAM, disco y contenedores activos/detenidos.
- Abrir Contenedores, revisar backend y base de datos, y consultar sus logs.
- Confirmar que no existen Recursos, Despliegues, Imágenes, GitHub, ECR ni botones de acción.
- Confirmar que un despliegue externo nuevo aparece en el siguiente refresco sin registrarlo.
