# Estabilidad

[English](stability.md) | **Español**

Desde la versión 1.0.0, minuta sigue el [versionado semántico](https://semver.org/lang/es/):
un cambio que rompa algo marcado como Estable aquí abajo solo llega en una versión
mayor (2.0.0). Las versiones anteriores a 1.0.0 no hacen esa promesa; el
[registro de cambios](../CHANGELOG.md) (en inglés) muestra qué se rompió entonces.

Todo lo que puedes usar de minuta está en una de tres clases:

| Clase | Qué significa |
| --- | --- |
| Estable | Cambia solo en una versión mayor. Pueden añadirse cosas nuevas en una versión menor |
| Provisional | Puede cambiar en una versión menor. El cambio aparece en el registro de cambios, en `Changed` |
| No cubierto | Puede cambiar en cualquier versión, incluso un parche, sin aviso |

Lo que no aparece aquí es No cubierto.

## Línea de comandos

| Elemento | Clase |
| --- | --- |
| Los subcomandos y sus nombres: `run`, `transcribe`, `doctor`, `models list\|pull`, `templates list\|show\|pull`, `config path\|show\|init` | Estable |
| Las opciones y su significado, como están en la [referencia](reference.es.md#opciones-de-run-y-transcribe) y en `minuta help` | Estable |
| Los códigos de salida `0`, `1`, `2` y `130` y [su significado](reference.es.md#código-de-salida) | Estable |
| Los ids de `--llm`: `ollama:<modelo>`, `claude`, `claude:<modelo>`, `codex`, `codex:<modelo>`, `<endpoint>:<modelo>` | Estable |
| Los nombres de modelos de whisper del catálogo (`tiny`, `base`, `small`, `medium`, `large-v3`, `silero-v5.1.2`) | Provisional |
| Las variables `MINUTA_*` de la [tabla de configuración](reference.es.md#configuración) y `MINUTA_CONFIG` | Estable |
| `MINUTA_TEMPLATES_URL` (no está en la documentación; existe para pruebas) | No cubierto |

Añadir un subcomando, una opción o un backend no rompe nada. En `run` y `transcribe`, que solo la ruta de la
carpeta del trabajo salga por stdout, y todo lo demás por stderr, es Estable.

## Configuración

| Elemento | Clase |
| --- | --- |
| La ruta del archivo de configuración y las claves de la [tabla de configuración](reference.es.md#configuración), con su significado | Estable |
| Las claves de `[endpoints.<nombre>]`: `url`, `context_tokens`, `timeout_secs`, `api_key_env` | Estable |
| Los valores predeterminados de la configuración | Provisional |
| El orden en que se resuelven los ajustes (opción, variable de entorno, archivo, valor predeterminado; las claves `llm` y `lang` de una plantilla van justo después de la opción, ver la [referencia](reference.es.md#configuración)) | Estable |

Una clave nueva puede aparecer en una versión menor. Las claves desconocidas son un
error, así que un archivo con una clave nueva no carga en un minuta más antiguo.

## Plantillas

| Elemento | Clase |
| --- | --- |
| Las claves del encabezado (`description`, `input`, `output`, `fields`, `limits`, `format`, `llm`, `lang`), sus valores y los predeterminados del encabezado | Estable |
| Las variables (`{{job_name}}`, `{{duration}}`, `{{output_lang}}`, `{{output_lang_code}}`, `{{speech_lang}}`) | Estable |
| Los tipos de salida `markdown` y `fields`, y los campos `title`, `body`, `thumbnail_text`, `notes`, `tags`, `hashtags`, `chapters` | Estable |
| El tipo de salida `chapters`, reservado para la plantilla integrada `chapters` | Estable |
| Los tres lugares de donde se cargan las plantillas y cuál gana | Estable |
| El catálogo `templates/index.toml`, su `version = 1` y sus claves | Estable |
| `.pulled.toml` | Provisional |
| El texto de las plantillas integradas y del catálogo, y los nombres y límites de las del catálogo | No cubierto |

Pueden llegar claves de encabezado, variables y campos nuevos en una versión menor.
El texto de una plantilla cambia cada vez que mejora su resultado; vuelve a
descargarla para recibir el cambio. Un minuta más antiguo rechaza una plantilla con
una clave de encabezado que no conoce.

## Archivos en disco

| Elemento | Clase |
| --- | --- |
| Los nombres en la raíz de la [carpeta del trabajo](reference.es.md#la-carpeta-del-trabajo): `transcript.txt`, `captions.srt`, `<plantilla>.md` | Estable |
| Los archivos de `assets/`: `transcript.json`, `<plantilla>.json`, `audio.wav`, `joined.<ext>` | Estable |
| El contenido de `transcript.json`: `language` y `segments`, cada uno con `start`, `end` y `text` | Estable |
| El contenido de `assets/<plantilla>.json`: `version`, `template`, `output_lang` y `fields` (`chapters.json` tiene `version`, `output_lang` y `chapters`) | Estable |
| El formato de los archivos legibles `transcript.txt` y `captions.srt` | Provisional |
| `.minuta/` y todo lo que contiene | No cubierto |

Los programas que lean los archivos JSON deben ignorar las claves que no conozcan:
una versión menor puede añadir claves. Cambiar o quitar una clave sube la
`version` del archivo y solo ocurre en una versión mayor. Una carpeta de trabajo
creada por una versión anterior no se migra: la siguiente ejecución empieza de cero.

## Lo que minuta imprime

| Elemento | Clase |
| --- | --- |
| Líneas de progreso, advertencias y avisos | No cubierto |
| El texto de los errores | No cubierto |
| La salida de `doctor`, `config show`, `models list` y `templates list` | Provisional |

Los scripts deben apoyarse en los códigos de salida y en la ruta de la carpeta del
trabajo, no en el texto.

## El crate de Rust

minuta es un programa. El crate tiene un destino de biblioteca, pero solo para que
el binario y sus pruebas compartan código; su API de Rust (`cli`, `error` y un
módulo oculto `testing`) es No cubierta y no sigue el versionado semántico. No
dependas de ella como biblioteca.

## Plataformas y herramientas

| Elemento | Clase |
| --- | --- |
| macOS (Apple silicon e Intel) y Linux (x64 y arm64) | Estable |
| Windows solo mediante WSL2 | Provisional |
| Node.js 22 o más reciente para el paquete de npm | Provisional |
| Versión mínima de Rust (hoy 1.88), para compilar desde el código fuente | Provisional |
| ffmpeg, ffprobe, whisper.cpp, Ollama, Claude Code y Codex | No cubierto |

Dejar de dar soporte a una plataforma Estable requiere una versión mayor. Una nueva
versión mínima de Node.js o de Rust puede llegar en una versión menor, con una nota
en el registro de cambios. minuta no comprueba una versión mínima de las
herramientas externas, así que una versión nueva de alguna puede cambiar un
resultado o romper una ejecución; `minuta doctor` muestra las versiones en uso.

## Obsolescencia

Cuando algo Estable va a cambiar o desaparecer, una versión menor lo marca antes
como obsoleto: el registro de cambios lo dice en `Deprecated` y la documentación
indica qué usar en su lugar. Sigue funcionando al menos una versión menor más. La
eliminación ocurre en la siguiente versión mayor.

## Dónde se anuncian los cambios incompatibles

Cada cambio incompatible aparece en [`CHANGELOG.md`](../CHANGELOG.md), en
`Changed (breaking)`, con los pasos para pasar a la nueva versión. El mismo texto
es el cuerpo de la versión en GitHub.
