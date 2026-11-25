# Instrucciones para agentes (Codex, Claude Code…)

Este repo es **Agent Gauge**, un widget de escritorio en Tauri 2 (Rust + TypeScript sin framework) que muestra el uso de Claude Code y Codex, y las sesiones en marcha. Lee `README.md` para el qué y `docs/superpowers/specs/2026-11-25-agent-gauge-design.md` para el porqué de las decisiones.

## Estado actual

Se inicializó en un contenedor Linux sin pantalla. Allí:

- `cargo build`, `cargo clippy --all-targets` (0 avisos) y `cargo test` (5 tests) pasan.
- `npm run build` pasa y la interfaz se revisó con capturas en el navegador usando datos de ejemplo.
- **Nunca se ha abierto la ventana real ni se ha leído una cuenta real.** Esa es tu primera tarea.

## Tu tarea: dejarlo funcionando en esta máquina

1. Requisitos: Node 20+ y Rust estable. En Linux, además: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`.
2. Ejecuta `npm install` y luego `npm run app:dev`. Comprueba lo siguiente:
   - La ventana sale sin bordes, transparente, siempre encima y en la esquina superior derecha. En macOS no debe aparecer en el Dock.
   - En macOS y Windows se ve el escritorio difuminado detrás (vibrancy o acrylic). Si no, revisa `apply_glass` en `src-tauri/src/lib.rs`.
   - La ventana se puede arrastrar desde la franja superior.
   - La ventana se adapta a la altura del contenido (`fitWindow` en `src/api.ts`).
   - El icono de la bandeja permite mostrar u ocultar la ventana, activar "Abrir al iniciar sesión" y salir.
3. **Datos reales.** Compara lo que muestra el widget con `/usage` en Claude Code y `/status` en Codex. Los porcentajes y las horas de renovación deben coincidir.
   - Claude: `src-tauri/src/claude.rs`. El endpoint `https://api.anthropic.com/api/oauth/usage` no es público. Si la forma de la respuesta no coincide con `ApiUsage`, ajusta el struct. Hazlo tolerante: todo `Option`.
   - Codex: `src-tauri/src/codex.rs`. Comprueba el formato real de `rate_limits` en `~/.codex/sessions/…`. Si tu versión usa otros nombres de campos, añádelos y escribe un test con una línea real anonimizada.
4. **Procesos.** Abre un `claude` y un `codex` en dos terminales y comprueba que aparecen una sola vez cada uno, con su carpeta. Las reglas están en `classify` dentro de `src-tauri/src/procs.rs`. Las apps de escritorio de Claude y Codex se excluyen a propósito.
5. Ejecuta `./scripts/install.sh` (o `scripts/install.ps1` en Windows) y comprueba que la app instalada arranca.

## Reglas del proyecto

- El login de Codex (`~/.codex/auth.json`) solo se lee y solo se envía a chatgpt.com para pedir los límites (`src-tauri/src/codex_live.rs`). Nunca se renueva ni se reescribe.
- En Windows y Linux, el widget puede renovar el token OAuth de Claude. Guarda los campos renovados de forma atómica y conserva los demás campos del archivo. En macOS, el acceso al Llavero es de solo lectura.
- Envía las credenciales únicamente a los endpoints de uso y renovación de Anthropic. Los registros de los chats se procesan localmente.
- Si cambias `src-tauri/src/model.rs`, actualiza también `src/types.ts`.
- Los textos de la interfaz están en español, en frase normal (sin MAYÚSCULAS), y dicen qué hacer cuando algo falla.
- Diseño: un solo elemento llamativo, los arcos. No añadas tarjetas, sombras ni animaciones decorativas. La paleta y la tipografía están en las variables de `:root` en `src/styles.css`.
- Antes de dar algo por terminado, ejecuta `npm run build`, `cargo clippy --all-targets -- -D warnings` y `cargo test` en `src-tauri/`.

## Ideas pendientes (no hechas)

- Notificación del sistema al pasar del 90 % de la sesión.
- Opacidad ajustable desde el menú de la bandeja.
- Recordar la posición de la ventana entre arranques (hoy siempre se coloca arriba a la derecha).
- Firmar y notarizar la app de macOS en el workflow de release.
