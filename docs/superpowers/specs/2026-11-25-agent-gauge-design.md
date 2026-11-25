# Agent Gauge — diseño

## Intención

Una persona que trabaja a diario con Claude Code y Codex quiere ver de un vistazo, sin abrir nada, cuánto margen le queda en cada cuenta y qué agentes tiene trabajando. Lo quiere como un widget de escritorio semitransparente, moderno y atractivo, que se instale fácil.

## Objetivos

- Mostrar el % usado de la ventana de 5 h y de la semanal de cada cuenta, con la hora de renovación.
- Avisar antes de llegar al límite, no cuando ya se ha llegado: de ahí la proyección de ritmo.
- Listar las sesiones CLI en marcha (carpeta, CPU, tiempo).
- Instalar con un instalador descargable o con un único script.

## Lo que no es

- No es un panel de costes ni de historial.
- No gestiona cuentas ni sesiones (no mata procesos ni hace login).
- No usa claves de API.

## Qué lo haría un fracaso aunque funcione

- Que tape trabajo: por eso es pequeño, se puede compactar a una píldora y ocultar.
- Que se vea como un panel SaaS genérico de tarjetas.
- Que toque o rote las credenciales del usuario.

## Enfoques considerados

| Enfoque | A favor | En contra |
|---|---|---|
| **Tauri 2 (elegido)** | Binario de unos 10 MB, desenfoque nativo (vibrancy y acrylic), multiplataforma, instaladores con `tauri-action` | Requiere Rust para compilar desde el código |
| Electron | Ecosistema conocido | Más de 150 MB y mucha RAM para un widget que está siempre abierto |
| Nativo (SwiftUI / Übersicht) | Lo más integrado en macOS | Solo funciona en un sistema y el usuario no lo ha especificado |

Elegido por Claude porque no se sabe el sistema del usuario y Tauri cubre los tres con cristal nativo y poco peso.

## Diseño visual

- **Material:** cristal ahumado azul tinta (`rgba(22,28,48,.38)` sobre el desenfoque nativo, `.8` en Linux, donde no hay desenfoque), con un brillo de 1 px en el borde superior y un grano sutil.
- **Protagonista:** dos medidores de arco de 240° con marcas de escala. Claude va en melocotón→coral y Codex en menta→celeste. Por encima del 85 % o con una proyección que pasa del 100 %, el arco vira a rojo rosado `#FF5C8A`.
- **Proyección:** una línea punteada desde el valor actual hasta el % previsto al renovar, terminada en un anillo.
- **Tipografía:** Unbounded para las cifras grandes y Manrope para el resto, con números tabulares.
- **Movimiento:** un único barrido de los arcos al arrancar y el latido del punto en las sesiones que usan CPU. Se respeta `prefers-reduced-motion`.
- **Textos:** en español, en frase normal, y cada error dice qué hacer.

## Datos

Ver la tabla "De dónde salen los datos" en `README.md`. El polling es de 20 s y la API de Claude se cachea 60 s.

## Decisiones de Claude, a revisar por el usuario

- Idioma de la interfaz: español.
- Posición inicial: arriba a la derecha, siempre encima.
- Se muestran como mucho 5 sesiones.
- El autoarranque viene desactivado y se activa desde la bandeja.
