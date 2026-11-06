# Referencia

[English](reference.md) | **Español**

## Comandos

| Comando | Qué hace |
| --- | --- |
| `minuta run FILES... [-t TEMPLATE]...` | Transcribe, genera documentos a partir de plantillas e imprime la ruta de la carpeta del trabajo |
| `minuta transcribe FILES...` | Igual que `run`, sin plantillas |
| `minuta doctor` | Informa sobre las herramientas, las versiones, el modelo y el backend de LLM; termina con código 1 si falta una herramienta o el modelo |
| `minuta models list` | Muestra cada modelo del catálogo y si está instalado |
| `minuta models pull NAME` | Descarga un modelo y verifica su suma de comprobación |
| `minuta templates list` | Muestra cada plantilla con su capa, entrada, salida y backend, y luego los archivos no válidos |
| `minuta templates show NAME` | Imprime el archivo que define una plantilla (su ruta va a stderr) |
| `minuta templates pull [NAME]...` | Descarga plantillas del catálogo en `./templates`, después de preguntar ([detalles](templates.es.md#descargar-plantillas)) |
| `minuta config path` | Imprime la ruta del archivo de configuración |
| `minuta config show` | Imprime cada ajuste, su valor y de dónde viene |
| `minuta config init [--force]` | Escribe un archivo comentado con todos los ajustes en la ruta del archivo de configuración; un archivo existente no se toca salvo que uses `--force` |

Ejecuta `minuta help <command>` para ver el uso completo de cualquier comando.

### Opciones de `run` y `transcribe`

| Opción | Descripción |
| --- | --- |
| `FILES...` | Una o más grabaciones, unidas en el orden indicado |
| `-n, --name NAME` | Nombre del trabajo. Por defecto es el nombre del primer archivo sin su extensión multimedia |
| `--out DIR` | Dónde se crean las carpetas de los trabajos. Predeterminado: `./minuta-out` |
| `--lang auto\|CODE` | Idioma del audio: `auto` (detectar) o un código de idioma de whisper como `en` o `es` |
| `--model PATH\|NAME` | Un archivo de modelo de whisper, o un nombre del catálogo como `base` |
| `--force [STEP\|TEMPLATE]` | Repite los pasos en lugar de reutilizar la caché (consulta [Caché](#caché-y---force)) |
| `-q, --quiet` | Oculta los mensajes de progreso. La ruta de la carpeta del trabajo sigue yendo a stdout, y los avisos, las advertencias y los errores siguen yendo a stderr |

`run` también acepta:

| Opción | Descripción |
| --- | --- |
| `-t, --template NAME` | Genera un documento con esta plantilla. Repítela para usar más plantillas |
| `--llm ID` | Backend para todas las plantillas de esta ejecución: `ollama:<model>`, `claude`, `claude:<model>`, `codex`, `codex:<model>` o `<endpoint>:<model>` |
| `--output-lang CODE` | Idioma en que se escriben los documentos. Predeterminado: el idioma detectado en el audio |

Solo la ruta de la carpeta del trabajo va a stdout. El progreso, las advertencias y
los errores van a stderr.

### Código de salida

| Código | Significado |
| --- | --- |
| `0` | Éxito |
| `1` | Falta una herramienta, un error de una herramienta, un error de LLM o de generación, o un error de configuración, de trabajo o de E/S |
| `2` | Error de uso: argumentos incorrectos, una plantilla desconocida o no válida, un idioma, paso o ID de backend desconocido, o un nombre de modelo desconocido para `models pull` |
| `130` | `run` o `transcribe` se interrumpió (Ctrl-C, `SIGTERM` o `SIGHUP`). La herramienta en ejecución también se detiene |

## Modelos de whisper

| Nombre | Tamaño de descarga | Notas |
| --- | --- | --- |
| `tiny` | 74 MiB | El más rápido, el menos preciso |
| `base` | 141 MiB | Una buena opción predeterminada |
| `small` | 465 MiB | |
| `medium` | 1.4 GiB | |
| `large-v3` | 2.9 GiB | El más preciso, el más lento |
| `silero-v5.1.2` | 0.8 MiB | Modelo de detección de actividad de voz (VAD), ver abajo |

Los modelos se guardan en `~/.minuta/models/`. Los modelos de whisper quedan ahí
como archivos `ggml-*.bin` simples, con la misma organización que usa whisper.cpp,
así que whisper.cpp puede leerlos directamente. Los modelos de VAD van en
`~/.minuta/models/vad/`. Cambia la carpeta con `MINUTA_MODELS_DIR` o
`whisper.models_dir`; `minuta models list` muestra la carpeta que lee.

En una terminal, `models pull` muestra una barra de progreso con el porcentaje, la
velocidad y el tiempo restante. Pasa `--quiet` para ocultarla.

Los modelos que una versión anterior guardó en `~/.cache/minuta/models/` ya no se
usan, así que `models pull` los descarga de nuevo. Puedes borrar esa carpeta.

`--model`, `whisper.model` y `whisper.vad_model` aceptan un nombre del catálogo o la ruta a cualquier
archivo de modelo `ggml` de whisper.cpp. `minuta models pull` acepta solo nombres
del catálogo.

**La detección de actividad de voz** reduce las líneas que whisper inventa durante
los silencios. Está desactivada hasta que
[configures un modelo de VAD](install.es.md#opcional-detección-de-actividad-de-voz).

## La carpeta del trabajo

Cada ejecución trabaja en una carpeta del trabajo, `<out>/<name>/`. Por defecto es
`./minuta-out/<first file's name without its extension>/`.

Los documentos quedan en la raíz de la carpeta del trabajo, los archivos que otros programas pueden reutilizar en `assets/` y los registros internos de minuta en una carpeta oculta `.minuta/`.

| Archivo | Contenido |
| --- | --- |
| `transcript.txt` | Transcripción legible con marcas de tiempo |
| `captions.srt` | Subtítulos |
| `<template>.md` | El documento generado con cada plantilla que pediste |
| `assets/transcript.json` | Segmentos con marcas de tiempo y el idioma detectado, como `{"version": 1, "language": ..., "segments": [{"start", "end", "text"}]}` |
| `assets/<template>.json` | El resultado estructurado de una plantilla `fields` o `chapters` |
| `assets/audio.wav` | Audio mono de 16 kHz que se le pasa a whisper |
| `assets/joined.<ext>` | La grabación unida. Solo con varias fuentes; con una, se usa el archivo original tal cual |
| `assets/<template>.md` | El Markdown de una plantilla que se ejecutó solo porque otra la necesitaba (por ejemplo, `chapters` con `youtube`); pasa a la raíz si pides esa plantilla |
| `.minuta/job.json`, `.minuta/probe.json` | Qué usó y encontró cada paso; minuta los lee para omitir lo que ya terminó |
| `.minuta/llm-cache/` | Un archivo por cada llamada al LLM |

Por ejemplo, después de `minuta run standup.mp4 -t linkedin -t youtube -t instagram`:

```console
$ ls minuta-out/standup
assets  captions.srt  instagram.md  linkedin.md  transcript.txt  youtube.md
$ ls minuta-out/standup/assets
audio.wav  chapters.json  chapters.md  instagram.json  linkedin.json  transcript.json  youtube.json
$ ls -A minuta-out/standup/.minuta
job.json  llm-cache  probe.json
```

`transcript.json` lleva `"version": 1`, como los demás archivos JSON de `assets/`. Un
archivo sin `version`, escrito por 0.9.0, se lee como versión 1 y no invalida la caché
de su trabajo. Un archivo con otra versión se rechaza con un mensaje que indica la
versión.

Una carpeta del trabajo pertenece a las grabaciones que la crearon. Si procesas otras grabaciones con un nombre de trabajo que ya está en uso, minuta se detiene en lugar de
sobrescribir. Elige otro nombre con `--name`, o reemplaza el trabajo con `--force`.

Las carpetas del trabajo creadas por una versión anterior no se migran: la siguiente
ejecución en una de ellas empieza de cero y lo rehace todo una vez. Los archivos
antiguos se quedan donde están, y puedes borrarlos.

### Caché y `--force`

minuta recuerda qué usó cada paso: los archivos de origen, las versiones de las
herramientas, el modelo, el idioma, y el texto y el backend de cada plantilla.
Cuando vuelves a ejecutar un comando, omite cada paso cuyas entradas no cambiaron.
Las respuestas del LLM también quedan en caché, así que una ejecución después de un
fallo o una interrupción reutiliza todas las respuestas que ya llegaron.

Para repetir los pasos de todos modos:

| Comando | Repite |
| --- | --- |
| `--force` | Todo |
| `--force transcribe` | Ese paso, todos los pasos posteriores y todas las plantillas de la ejecución. Pasos, en orden: `probe`, `join`, `audio`, `transcribe`, `outputs` |
| `--force summary` | Esa plantilla y las plantillas que la leen, sin usar respuestas en caché |

`--force` acepta un valor opcional, así que ponlo después de los archivos:

```sh
minuta run meeting.mp4 -t summary --force summary
```

## Configuración

Los ajustes de la tabla de abajo pueden venir del archivo de configuración o de una
variable de entorno, y algunos también tienen una opción de línea de comandos. Una
opción tiene prioridad sobre una variable de entorno, esta sobre el archivo de
configuración, y este sobre el valor predeterminado. Las claves `llm` y `lang` de
una plantilla quedan entre la opción y las demás capas (consulta
[Backends de LLM](templates.es.md#backends-de-llm)). Ejecuta `minuta config show`
para ver cada valor y de dónde viene.

El archivo de configuración es `~/.minuta/config.toml`. Define `MINUTA_CONFIG` para
usar otro archivo. Una clave desconocida produce un error. `minuta config init`
escribe un archivo comentado con todos los ajustes, para que veas qué se puede
definir.

Un ejemplo:

```toml
out_dir = "minuta-out"

[whisper]
model = "small"
lang = "auto"
vad_model = "silero-v5.1.2"

[llm]
default = "ollama:gemma4:26b"
output_lang = "en"

[ollama]
url = "http://127.0.0.1:11434"
num_ctx = 16384
```

`~` no se expande en `out_dir`. Usa una ruta relativa o una completa.

| Clave | Variable de entorno | Opción | Predeterminado |
| --- | --- | --- | --- |
| `out_dir` | `MINUTA_OUT_DIR` | `--out` | `./minuta-out` |
| `whisper.model` | `MINUTA_WHISPER_MODEL` | `--model` | sin definir (obligatorio) |
| `whisper.lang` | `MINUTA_WHISPER_LANG` | `--lang` | `auto` |
| `whisper.vad_model` | `MINUTA_WHISPER_VAD_MODEL` | | sin definir (VAD desactivado) |
| `whisper.models_dir` | `MINUTA_MODELS_DIR` | | `~/.minuta/models` |
| `whisper.bin` | `MINUTA_WHISPER_BIN` | | `whisper-cli` |
| `ffmpeg.bin` | `MINUTA_FFMPEG_BIN` | | `ffmpeg` |
| `ffprobe.bin` | `MINUTA_FFPROBE_BIN` | | `ffprobe` |
| `llm.default` | `MINUTA_LLM` | `--llm` | `ollama:gemma4:26b` |
| `llm.output_lang` | `MINUTA_OUTPUT_LANG` | `--output-lang` | el idioma detectado en el audio |
| `llm.concurrency` | `MINUTA_LLM_CONCURRENCY` | | lo elige el backend |
| `llm.retries` | `MINUTA_LLM_RETRIES` | | `3` (intentos en total) |
| `llm.backoff_ms` | `MINUTA_LLM_BACKOFF_MS` | | `2000` (se duplica tras cada fallo) |
| `llm.repairs` | `MINUTA_LLM_REPAIRS` | | `2` (`0` desactiva las reparaciones) |
| `ollama.url` | `MINUTA_OLLAMA_URL` | | `http://127.0.0.1:11434` |
| `ollama.num_ctx` | `MINUTA_OLLAMA_NUM_CTX` | | `16384` |
| `claude.bin` | `MINUTA_CLAUDE_BIN` | | `claude` |
| `codex.bin` | `MINUTA_CODEX_BIN` | | `codex` |

Las claves `*.bin` aceptan un nombre de comando que se busca en tu `PATH`, o una
ruta completa.

Las tablas `[endpoints.<name>]` se leen solo del archivo. Consulta
[Servidores compatibles con OpenAI](templates.es.md#servidores-compatibles-con-openai)
para ver sus claves.

### Actualizar desde 0.2.0 o anterior

El archivo de configuración y las plantillas de usuario se movieron a `~/.minuta/`,
junto a los modelos. Los archivos de la carpeta antigua se ignoran, así que muévelos
una vez:

```sh
mkdir -p ~/.minuta
mv ~/.config/minuta/config.toml ~/.minuta/config.toml      # si tienes uno
mv ~/.config/minuta/templates ~/.minuta/templates          # si tienes plantillas de usuario
```

## Solución de problemas

**`missing required tools: model`.** No hay ningún modelo de whisper configurado.
Ejecuta `minuta models pull base` y define `whisper.model = "base"` en la
configuración, o pasa `--model base`.

**Falta `ffmpeg`, `ffprobe` o `whisper-cli`.** Instálalos (consulta la
[guía de instalación](install.es.md)), o apunta `ffmpeg.bin`, `ffprobe.bin` o
`whisper.bin` al binario.

**Ollama aparece como `not reachable` o `model missing`.** Inícialo con
`ollama serve` y descarga el modelo, por ejemplo con `ollama pull gemma4:26b`. Si
Ollama se ejecuta en otra máquina o puerto, define `ollama.url`.

**Una respuesta de Ollama llega incompleta.** El prompt no cabe en la ventana de
contexto. Aumenta `ollama.num_ctx` si el modelo admite una ventana de contexto más grande.

**`unknown endpoint`.** La parte antes de `:` en el ID del backend no es una tabla
del archivo de configuración. Agrega `[endpoints.<name>]` con su `url`, o revisa la
ortografía.

**Un endpoint dice que el modelo es `not offered by the server`.** El ID debe
coincidir exactamente con el ID del modelo en el servidor. El mensaje lista lo que
ofrece el servidor.

**Un endpoint responde `exceeds the available context size`, o una respuesta llega incompleta.**
`context_tokens` es mayor que el contexto que el servidor le dio al modelo. Carga el
modelo con un contexto mayor, o baja `context_tokens`.

**Una solicitud a un endpoint agota el tiempo de espera.** Los modelos lentos
necesitan más que los 10 minutos predeterminados. Sube `timeout_secs`.

**La transcripción repite frases durante los silencios.** Activa la
[detección de actividad de voz](install.es.md#opcional-detección-de-actividad-de-voz).

**`choose another name with --name or replace it with --force`.** Un trabajo con ese
nombre se creó a partir de otras grabaciones. Consulta
[La carpeta del trabajo](#la-carpeta-del-trabajo).

**Una plantilla no aparece o no se carga.** `minuta templates list` muestra al final
los archivos de plantilla no válidos con el motivo.
