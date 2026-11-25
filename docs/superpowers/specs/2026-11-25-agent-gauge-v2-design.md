# Agent Gauge v2: más cristal, chats en vivo y saturación — diseño

Continúa `2026-11-25-agent-gauge-design.md`. Este documento cambia tres cosas: el aspecto, de dónde salen los "chats en marcha" y cómo se leen los límites de Claude.

## Intención

El usuario trabaja casi siempre desde la **app de escritorio de Claude** (no desde el CLI `claude`). Desde ahí a veces da a Claude el control del CLI de Codex, y Codex lanza sus propios subagentes. Quiere ver de un vistazo:

1. Cuánto le queda en cada cuenta (Claude y Codex), con números reales.
2. Qué chats están trabajando ahora mismo en cada lado, con sus subagentes mientras corren.
3. Lo saturado que está el contexto de cada chat, para saber cuándo empezar uno nuevo.

Y que el widget se vea bien. La primera versión le parece fea: demasiado opaca, y en Windows las esquinas enseñan un "pico" de otro color.

## Qué cambia respecto a v1

| Tema | v1 | v2 |
|---|---|---|
| Aspecto | Cristal oscuro bastante opaco, radio 26 px | Cristal esmerilado más transparente, arcos más finos, radio que coincide con la ventana |
| "En marcha" | Procesos `claude`/`codex` del sistema | Chats con actividad reciente, leídos de sus registros, con subagentes y saturación |
| Límites de Claude | Solo lee el token. Si caduca, pide abrir `claude` | El widget renueva el token él mismo cuando caduca |

## Objetivos

- **Aspecto (elegido por el usuario: dirección "A, arcos más ligeros").** Cristal esmerilado con desenfoque nativo, más transparente que hoy. Se mantienen los dos arcos como protagonistas.
- **Esquinas limpias.** En Windows 11, el desenfoque (acrylic) rellena el rectángulo de la ventana y Windows lo redondea a unos 8 px. El panel debe usar el mismo radio para que no asome nada por fuera.
- **Chats en marcha.** Se listan los chats de Claude y de Codex con actividad reciente, y debajo de cada uno sus subagentes **solo mientras están corriendo**.
- **Saturación por chat.** Un medidor pequeño con el % de la ventana de contexto que ocupa el último turno del chat.
- **Límites de Claude siempre disponibles.** El widget usa el `refreshToken` para conseguir un token nuevo cuando el actual ha caducado.

## No objetivos

- No es un historial de chats: los que llevan un rato parados no salen.
- No se puede abrir, cerrar ni enviar nada a un chat desde el widget.
- No se sacan textos de las conversaciones: de cada chat solo se leen metadatos (título, carpeta, tokens, horas).

## Anti-objetivos (fracaso aunque funcione)

- Que el widget haga cerrar la sesión de Claude Code o de la app de escritorio al renovar el token.
- Que muestre el contenido de las conversaciones, aunque sea un trozo.
- Que vuelva a parecer un panel SaaS de tarjetas, o que tape trabajo.

## Restricciones

- Windows 11 es el sistema principal del usuario (con macOS y Linux igualmente soportados).
- El formato de los registros de Claude Code y de Codex no es público y cambia entre versiones. Todo se lee de forma tolerante (campos opcionales), y cada formato tiene un test con una línea real anonimizada.
- Sigue sin mandarse nada a ningún sitio salvo a Anthropic: la llamada de uso y, ahora, la de renovar el token.
- Los textos de la interfaz siguen en español, en frase normal.

## Enfoques considerados (detectar chats)

| Enfoque | A favor | En contra |
|---|---|---|
| **Leer los registros de los chats (elegido)** | Ve los chats de la app de escritorio, los subagentes y los tokens de contexto | Depende de formatos internos que pueden cambiar |
| Procesos del sistema (v1) | Formato estable | La app de escritorio ejecuta todos los chats en un único proceso: no ve los chats, ni los subagentes, ni la saturación |
| Mezcla de registros y procesos | Detectaría CLIs abiertos pero parados | El usuario quiere los chats que están trabajando, no los que siguen abiertos, así que no compensa la complejidad |

## Enfoques considerados (límites de Claude)

| Enfoque | A favor | En contra |
|---|---|---|
| **El widget renueva el token (elegido por el usuario)** | Siempre hay porcentajes oficiales | Rompe la regla de v1 "solo se lee". Puede chocar con otro programa que renueve a la vez |
| Solo leer (v1) | No toca nada | El usuario no usa el CLI: casi siempre aparecería "caducó" |
| Estimar con los tokens locales | No necesita login | No es el porcentaje oficial, y el límite de tokens del plan no es público |

## Diseño

### 1. Aspecto

- `--glass-native` pasa de `rgba(22,28,48,.38)` a unos `rgba(18,22,36,.22)`. Para que el texto se lea sobre fondos claros, la tinta lleva una sombra muy suave (`text-shadow` de 1 px con poca opacidad).
- El borde de luz superior y el grano se mantienen, más tenues.
- **Radio por sistema:** 8 px en Windows, porque coincide con el redondeo de Windows 11 a la ventana. En macOS y Linux sigue siendo grande (unos 22 px). El frontend ya conoce el sistema (`platformInfo.os`), así que basta con una variable CSS.
- En Windows se pide además el redondeo estándar con `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND`, para no depender del valor por defecto.
- La ventana sigue fijada al tema oscuro (`"theme": "Dark"`). Sin eso, el acrylic sale gris claro cuando Windows está en tema claro.
- Arcos con un trazo de unos 5 px, menos marcas de escala, y el número un poco más pequeño. La maqueta aprobada es `.superpowers/brainstorm/*/content/estilo-cristal.html`, tarjeta A.
- La sección "En marcha" pasa a llamarse **"Chats activos"**.

### 2. Chats activos

Un chat cuenta como **activo** si su registro se ha modificado en los últimos **10 minutos**. Está **trabajando** (punto que late) si se modificó en los últimos **20 segundos**.

**Claude** (`$CLAUDE_CONFIG_DIR` o `~/.claude`):

- Chats: `projects/<carpeta-codificada>/<sessionId>.jsonl`. Esto incluye los de la app de escritorio.
- Título: la última línea `{"type":"custom-title","customTitle":…}` del registro. Si no hay, el nombre de la carpeta de `cwd`.
- Carpeta: el campo `cwd` de cualquier línea del registro.
- Subagentes: `projects/<carpeta>/<sessionId>/subagents/agent-<id>.jsonl`, cada uno con su `agent-<id>.meta.json` (`agentType`, `description`, `model`, `finished`). Uno está **corriendo** si `finished` no es `true` y su `.jsonl` se ha modificado en los últimos 10 minutos.
- Saturación: en la última línea `assistant` con `message.usage`, se suma `input_tokens + cache_creation_input_tokens + cache_read_input_tokens` y se divide entre la ventana de contexto del modelo (`message.model`). La ventana sale de una tabla de modelos en el código; si el modelo no está en la tabla, se usa un valor por defecto y el medidor muestra "≈". **Contexto de 1M (decisión de Claude):** el registro no dice si un chat usa la variante de 1M tokens. Se supone la ventana estándar del modelo. Si algún turno del chat la supera, ese chat pasa a calcularse sobre 1M y se marca con "≈".

**Codex** (`$CODEX_HOME` o `~/.codex`):

- Chats: `sessions/AAAA/MM/DD/rollout-*.jsonl`. Las sesiones comprimidas (`.jsonl.zst`) son antiguas y se ignoran.
- La primera línea (`session_meta`) trae `id`, `cwd` y `originator`. Si `originator == "Claude Code"`, el chat se marca como "desde Claude".
- Subagentes: una sesión cuyo `session_meta.payload.source.subagent.thread_spawn` existe es un subagente. `parent_thread_id` indica su padre y `agent_nickname` su nombre. Se agrupa debajo del padre si el padre está en la lista. Si no lo está, el subagente sale suelto con su nombre.
- Título: `session_index.jsonl` (`id → thread_name`). Si no aparece, la carpeta.
- Saturación: del último evento `token_count`, `info.last_token_usage.input_tokens / info.model_context_window`.

**Presentación (decisión de Claude):** una sola lista mezclada y ordenada por actividad más reciente. Tiene un máximo de 5 chats en total y 3 subagentes por chat ("+N más" si hay más). Si hay chats activos de las dos herramientas, al menos uno de cada una entra en la lista, para que un Codex lanzado desde Claude no quede fuera. Cada fila muestra el punto del color de su herramienta, el título, la carpeta en gris y el medidor de saturación.

**Cuándo se pone rojo el medidor:** a partir del 80 % en Claude, porque Claude Code compacta solo cerca de ese punto. En Codex, a partir del 85 %.

Los procesos del sistema (`procs.rs`) dejan de usarse para la lista. El módulo se puede eliminar.

### 3. Renovar el token de Claude

- Antes de pedir el uso, si `expiresAt` ya ha pasado o pasa en menos de 5 minutos, el widget **vuelve a leer el archivo**, porque otro programa puede haberlo renovado ya. Solo si sigue caducado, renueva él.
- Para renovar, hace lo mismo que Claude Code 2.1.330 (comprobado en su binario y probado el 2026-11-25): un `POST` JSON a `https://platform.claude.com/v1/oauth/token` con `{grant_type: "refresh_token", refresh_token, client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e", scope: scopes.join(" ")}`. Responde `access_token`, `refresh_token`, `expires_in` y `refresh_token_expires_in`, entre otros campos.
- **El código de renovación rota** (comprobado): cada renovación devuelve uno nuevo. Por eso el widget **guarda siempre el nuevo de inmediato** y nunca renueva dos veces con el mismo código.
- Al guardar, reescribe solo `claudeAiOauth.accessToken`, `refreshToken`, `expiresAt` y `refreshTokenExpiresAt` (a partir de `refresh_token_expires_in`). El resto del JSON se conserva tal cual (`mcpOAuth`, `scopes`, `subscriptionType`, etc.). La escritura es atómica: primero un archivo temporal en la misma carpeta y luego se renombra.
- **macOS (decisión de Claude):** se queda en solo lectura. Reescribir el elemento del Llavero desde otro programa puede cambiar quién tiene acceso a él, y Claude Code empezaría a pedir permiso o a fallar. El usuario trabaja en Windows. En macOS se sigue mostrando "abre Claude Code".
- Se renueva como mucho una vez cada 5 minutos, para no entrar en bucle si falla.
- **Convivencia con la app de escritorio (prueba del 2026-11-25):** la app de escritorio no renueva este archivo, porque su login caducó ayer y la app seguía funcionando. Todo indica que usa un login propio y que no hay choque. Queda pendiente que el usuario confirme que sigue con la sesión iniciada tras la renovación. Si algún programa le pide volver a iniciar sesión, el plan B es renovar solo cuando no haya ningún `claude` CLI en marcha.
- La respuesta de uso confirmada trae `five_hour` / `seven_day` con `utilization` y `resets_at`, que es lo que el código ya lee. También trae `limits[]` con `kind`, `percent` y `severity`, que no hace falta usar.
- Si falla la renovación (por ejemplo, porque `refreshTokenExpiresAt` también ha pasado), se muestra: "Tu sesión de Claude caducó del todo. Abre Claude Code y vuelve a iniciar sesión."
- `AGENTS.md` y `README.md` se actualizan: la regla pasa a ser "el widget puede renovar el token, nunca lo envía a otro sitio que no sea Anthropic, y conserva el resto del archivo".

### 4. Tipos compartidos

`model.rs` y `src/types.ts` cambian juntos. `AgentProcess` se sustituye por:

```
Chat {
  id, provider, title, cwd,
  lastActivity (ISO), working (bool),
  fromClaude (bool, solo Codex),
  contextPercent (0–100 | null), contextApprox (bool),
  subagents: [{ id, name, kind, lastActivity, working }]
}
```

`Snapshot.processes` pasa a ser `Snapshot.chats`. El mock del navegador se actualiza con chats de ejemplo, incluido un subagente y un chat al 91 %.

### 5. Bichitos y vista por proyectos (añadido tras ver las maquetas)

El usuario vio el cristal nuevo y le pareció frío. Quiere algo más amable: los bichitos de Claude y de Codex animados, hover que despliegan detalles, y ver "cómo se mueve el uso" mientras algo trabaja. También quiere los chats agrupados **por proyecto**, porque suele tener un Claude mandando y varios Codex trabajando para él. Las maquetas aprobadas son `.superpowers/brainstorm/*/content/bichitos.html` (cuentas) y `proyectos.html` (proyectos).

**Cuentas (parte de arriba).** Una fila por cuenta: el bichito, un arco pequeño (unos 64 px), el % grande, el nombre y "se renueva en…". El bichito de Claude va a la izquierda y el de Codex a la derecha.
- En reposo, los bichitos respiran y parpadean.
- Si su herramienta tiene algún chat trabajando, el bichito "camina".
- Hover sobre la fila: despliega la semana, el ritmo previsto y los tokens de 5 h.

**Uso en vivo.** Cuando el % de una cuenta sube entre dos lecturas, el número y el arco suben con animación y sale un "+N %" flotando desde el bichito. Se muestra el % gastado (coincide con `/usage`), no lo que queda.

**Proyectos (parte de abajo).** Sustituye a la lista de chats. Un proyecto es el conjunto de chats activos con la misma carpeta de trabajo. La comparación de rutas no distingue mayúsculas en Windows e ignora la barra final. Cada proyecto tiene:
- **Jefe:** el chat de Claude más reciente de esa carpeta. Su saturación sale junto al nombre del proyecto. Mientras trabaja, el bichito de Claude da latigazos de dibujos animados cada 1,6 s.
- **Cuadrilla:** todo lo demás de esa carpeta.
  - Los chats de Codex y sus subagentes son bichitos azules.
  - Los subagentes del jefe son bichitos naranjas pequeños.
  - Otros chats de Claude en la misma carpeta son bichitos naranjas normales.
  - Los que trabajan dan un respingo y sudan con cada latigazo. Los parados se ven apagados.
- Si no hay chat de Claude en la carpeta, no hay jefe: solo la cuadrilla.
- Si nada trabaja, el jefe se ve apagado con el látigo bajado y el texto "esperando tu respuesta".
- **Hover sobre el proyecto:** despliega la lista con nombre, tipo, estado y saturación de cada uno.

**Límites (decisión de Claude):**
- Como mucho 4 proyectos, ordenados por actividad.
- Como mucho 8 en la cuadrilla, primero los que trabajan y después los más recientes, con "+N" si hay más.
- Se agrupa **antes** de recortar, para no perder la cuadrilla de un proyecto.

**Bichitos (decisión de Claude):** son dibujos propios en pixel art con los colores de cada herramienta, no la ilustración oficial de Anthropic ni de OpenAI, porque el repo publica instaladores. Con `prefers-reduced-motion` no se anima nada.

**Restricción del cristal:** los bichitos van **dentro** del panel. En Windows el desenfoque rellena toda la ventana, así que cualquier cosa que sobresaliera del panel dejaría ver un rectángulo borroso a su alrededor.

## Decisiones que deja al builder

- Los umbrales exactos (10 min / 20 s / 80 %) se pueden ajustar si en la práctica se ven mal.
- Cómo recorrer `~/.claude/projects` sin leer archivos enteros: se recomienda leer solo el final (los últimos ~64 KB) para el uso y la primera línea para los metadatos.
- La tabla de ventanas de contexto por modelo.

## Cómo se comprueba

- Tests con líneas reales anonimizadas: registro de Claude, `meta.json` de un subagente, `session_meta` de Codex con `thread_spawn` y `token_count` con `model_context_window`.
- Un test de la renovación del token contra un servidor falso, que verifique que se conservan las claves que no son del token.
- Revisión visual en Windows: sin pico en las esquinas, el fondo se ve a través, los textos se leen sobre un fondo claro y sobre uno oscuro.
- Con un chat de la app de escritorio trabajando y un Codex lanzado desde Claude, ambos aparecen con su saturación, y los subagentes desaparecen al terminar.
- `npm run build`, `cargo clippy --all-targets -- -D warnings` y `cargo test` pasan.

### 6. Entornos y agentes dormidos (añadido el 2026-11-25)

El usuario trabaja en varios sitios: este PC con Windows y su servidor Beelink, al que la app de Claude entra por SSH (`root@192.168.68.50`). Quiere filtrar los proyectos según dónde corren y ver también los agentes que ya existen aunque estén parados, "como dormidos".

**Entornos.**
- **Este PC:** chats locales de Claude y de Codex.
- **Un entorno por host SSH:** sale del `sshConfig` de los chats de la app de escritorio. Su nombre es el `hostname` que devuelve el propio servidor. Si no se puede obtener, se usa el alias de `~/.ssh/config` que apunta a ese host, y si tampoco hay, el host tal cual.
- Los chats de Claude por SSH (`~/.claude/projects/ssh-<cliSessionId>/`) pertenecen al entorno de su host. El host se busca por `cliSessionId` en los archivos de sesión de la app.
- **Nube:** queda fuera por ahora. En este PC no hay datos de agentes en la nube: el catálogo de la app de Codex solo tiene chats de ChatGPT, sin repositorio.

**Estados de un chat o subagente.**
- **Trabajando:** su registro se modificó hace ≤ 20 s.
- **Despierto:** hace ≤ 10 min.
- **Dormido:** hace ≤ 24 h.
- Lo de hace más de 24 h no se muestra.
- Los subagentes terminados (`finished: true`) también aparecen, dormidos, si son de las últimas 24 h.

**Agrupación.** Los proyectos se agrupan por (entorno, carpeta). La regla de unir carpetas padre e hija solo se aplica dentro del mismo entorno.

**Interfaz.**
- Pestañas sobre los proyectos: "Todos", "Este PC" y una por host SSH. La pestaña elegida se recuerda en `localStorage`.
- Los dormidos se dibujan con los ojos cerrados, más apagados y con una "z" pequeña. Un proyecto sin nadie despierto muestra al jefe dormido.
- Como mucho 5 proyectos por entorno, y una línea "+N proyectos más" si hay más.
