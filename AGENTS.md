# Instrucciones para agentes (Codex, Claude Code…)

Este repo es **Agent Gauge**, un widget de escritorio en Tauri 2 (Rust + TypeScript sin framework). Muestra el uso de Claude Code y Codex y los proyectos con sus agentes, por entorno (este PC, servidores SSH, nube de Claude).

- Qué hace y cómo se instala: `README.md`.
- Por qué está hecho así: `docs/superpowers/specs/2026-11-25-agent-gauge-v2-design.md` (y la v1 en el mismo directorio).
- Cómo se construyó, tarea a tarea: `docs/superpowers/plans/2026-11-25-agent-gauge-v2.md`.

## Estado

- Probado de verdad en Windows 11 con cuentas reales de Claude (Pro) y Codex (Plus): ventana, cristal, bandeja, instalador NSIS y datos.
- macOS y Linux compilan en CI, pero no se han probado a mano.
- Los formatos que se leen (registros de Claude Code y Codex, servicios de uso, sesiones en la nube, archivos de sesión de la app de escritorio) son internos y cambian entre versiones. Cada lector es tolerante y tiene tests con una muestra real anonimizada. Si algo deja de salir, empieza por ahí.

## Reglas del proyecto

- **Credenciales.**
  - El login de Codex (`~/.codex/auth.json`) solo se lee y solo se envía a chatgpt.com para pedir los límites (`codex_live.rs`). Nunca se renueva ni se reescribe.
  - En Windows y Linux, el widget puede renovar el token OAuth de Claude (`claude.rs`). Guarda los campos renovados de forma atómica y conserva los demás campos del archivo. En macOS, el acceso al Llavero es de solo lectura.
  - El token de Claude solo va a Anthropic: uso, renovación y lista de sesiones en la nube.
- **Conversaciones.** De los registros se leen metadatos (título, carpeta, tokens, horas). Nunca el texto.
- **SSH.** Solo con clave (`BatchMode`), con tiempo límite, y ejecutando el script de solo lectura `remote_codex.sh`.
- **No saturar los servicios.** Cada consulta remota tiene caché (60 s) y, ante un HTTP 429, espera más en cada intento (2 a 15 min). Un fallo nunca debe provocar un reintento en cada lectura.
- **Tipos compartidos.** Si cambias `src-tauri/src/model.rs`, actualiza también `src/types.ts`.
- **Textos.** La interfaz está en español, en frase normal (sin MAYÚSCULAS), y cada error dice qué hacer.
- **Diseño.** Los protagonistas son los bichitos (`src/critters.ts`, pixel art propio, no las mascotas oficiales) y sus arcos. Nada de tarjetas ni sombras decorativas. Toda animación se para con `prefers-reduced-motion`. Los tokens de color y tipografía están en `:root` de `src/styles.css`.
- **Windows.** El panel usa un radio de 8 px porque el acrylic rellena toda la ventana y Windows 11 la redondea a 8 px. Lo que sobresalga del panel se vería con un rectángulo borroso alrededor.
- **Antes de dar algo por terminado**, ejecuta `npm run build`, y en `src-tauri/` `cargo clippy --all-targets -- -D warnings` y `cargo test`.

## Ideas pendientes

- Atajo de teclado global para mostrar u ocultar el widget.
- Notificación del sistema al pasar del 90 % de la sesión.
- Recordar la posición de la ventana entre arranques.
- Sesiones de Codex en la nube (hoy no hay datos locales de ellas).
- Firmar la app (Windows y macOS) en el workflow de release.
