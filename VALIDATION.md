# Estado de validación

Actualizado el 15 de agosto de 2026 para la versión `0.2.0`.

## Comprobaciones ejecutadas

Todas se ejecutaron sobre Linux x86_64 con Rust 1.97.1 y Node 22.

| Comprobación | Comando | Resultado |
| --- | --- | --- |
| Análisis estático de Rust | `cargo clippy --all-targets` | Sin errores |
| Pruebas unitarias de Rust | `cargo test` | 44/44 correctas |
| Compilación optimizada | `cargo build --release` | Binario de 841 KB |
| Ciclo completo de la API | `./scripts/smoke-test.sh target/release/tmanager` | Correcto |
| Empaquetado de la interfaz | `npm run build` (en `web/`) | 115 KB, 43 KB gzip |
| Render de la interfaz | `npm test` (en `web/`) | 11/11 correctas |

### Qué cubren las pruebas unitarias

- SHA-256 contra los vectores de FIPS 180-4, incluido el de 1 MiB que fuerza el relleno de
  longitud; HMAC-SHA256 contra los vectores de RFC 4231, incluido el de clave más larga que
  el bloque.
- Base64URL sin relleno y comparación en tiempo constante.
- Analizador JSON: documentos anidados, escapes, pares sustitutos, entradas mal formadas y
  límite de anidamiento.
- Lista blanca de hosts salientes, incluidos los intentos de evasión con sufijo
  (`api.github.com.evil.com`), credenciales en la URL y puerto explícito; e inyección de
  cabeceras mediante CRLF.
- Generación de Compose para los cinco motores: ausencia de puertos publicados si no se
  piden, `no-new-privileges`, healthcheck, `mem_limit`, red externa y cadenas de conexión.
- Contraseña de root solo en los motores que la usan.
- Nonces de un solo uso, manifiesto de GitHub apuntando al panel, ida y vuelta de las
  credenciales incluida la clave PEM, y verificación de que el endpoint de estado no filtra
  secretos.
- Firma de webhook de GitHub contra el vector público de su documentación.
- Rutas relativas que no pueden escapar del clon y rutas de volumen absolutas.
- Archivo de credenciales de git con permisos `0600` y borrado garantizado al salir de
  ámbito; redacción de tokens en los mensajes de error.
- Persistencia SQLite de proyectos y despliegues, reapertura, paginación, filtros, retención,
  actualizaciones y rechazo seguro de un archivo TDM antiguo.
- Lanzador de subprocesos: captura de salida, código de salida y muerte por timeout.

### Qué cubre el smoke test

Levanta el binario real con un Docker simulado y recorre: salud, rechazo sin token,
métricas, contenedores, alta de proyecto Compose, deploy con imagen, deploy fallido con
restauración automática del `.env`, webhook propio, rollback, historial, logs, catálogo de
recursos, alta de los cinco motores de base de datos, alta de servicio desde imagen,
rechazo de variables reservadas y rutas de escape, estado de GitHub sin App conectada,
rechazo de webhook de GitHub sin firma, y los tres niveles de borrado.

### Qué cubren las pruebas de interfaz

Montan el bundle real en un DOM con `fetch` simulado y comprueban que renderizan el acceso,
el panel con métricas, recursos, despliegues, GitHub, sistema y procesos; que el asistente
«Añadir recurso» ofrece los cuatro orígenes; que el paso de base de datos pinta los motores
del catálogo; que el de imagen arranca en el buscador; y que el de repositorio queda
bloqueado mientras GitHub no esté conectado.

## Medición de memoria

Desglose de `smaps_rollup` tras el arranque, con 2 workers:

| | 0.1.5 | 0.2.0 |
| --- | --- | --- |
| RSS total | 2.34 MB | 2.49 MB |
| Compartido con el sistema | 1.68 MB | 1.75 MB |
| Privado limpio (código y datos estáticos) | 504 KB | 580 KB |
| Privado sucio (heap y pilas) | 164 KB | 164 KB |

Bajo 900 descargas consecutivas del bundle el RSS no varió; tras 800 llamadas a la API
quedó en 3.0 MB, por debajo del objetivo de 50 MB.

## Lo que no se ha podido validar aquí

- **Interfaz en un navegador real.** Las pruebas usan un DOM sintético: verifican estructura
  y datos, no el resultado visual, ni la disposición responsive, ni el foco real.
- **Flujo de GitHub de extremo a extremo.** Crear la App, canjear el manifiesto, listar
  repositorios y recibir un webhook real requieren una cuenta de GitHub y un panel
  alcanzable desde internet. La lógica está cubierta por pruebas unitarias, pero la
  integración completa hay que probarla contra GitHub.
- **Búsqueda real en Docker Hub.** El proxy está probado en sus validaciones y en el
  troceado de nombres, pero las pruebas no llaman al registro para no depender de la red.
- **Construcción de imágenes desde repositorio en una máquina pequeña.** Conviene medir
  cuánto tarda y cuánta RAM consume `docker build` en tu servidor antes de usarlo en
  producción.
- **Servicio systemd en funcionamiento.** Verificado sintácticamente, no ejecutado.

## Reproducir la validación

```bash
cd web && npm ci && npm test && cd ..
cargo clippy --all-targets
cargo test
cargo build --release
./scripts/smoke-test.sh target/release/tmanager
```

`.github/workflows/ci.yml` ejecuta exactamente estas comprobaciones, y además falla si
`web/dist` no coincide con lo que produce `web/src`.
