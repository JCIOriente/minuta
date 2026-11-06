# De una grabación al acta de una reunión

[English](worked-example.md) | **Español**

**Objetivo:** convertir la grabación de una reunión en un acta que puedas enviar a
tu equipo. Vas a transcribir la grabación, corregir una transcripción con errores,
generar el acta, revisarla y compartirla.

El ejemplo es una grabación corta llamada `weekly-sync.m4a`. Usa tu propia
grabación y cambia el nombre en cada comando. Lo que veas en pantalla será distinto
de esta página en las palabras, porque los modelos escriben distinto cada vez.

## Antes de empezar

- minuta está instalada y `minuta doctor` no muestra `missing`. Si no, sigue
  [Resuelve lo que reporta doctor](fix-doctor.es.md).
- Tienes un modelo de lenguaje para los documentos, como Ollama con el modelo
  predeterminado. Si no, sigue [Configura un LLM y un modelo de whisper](llm-and-whisper.es.md).
- Tienes la grabación de una reunión en un archivo que ffmpeg pueda leer, como
  `.mp4`, `.m4a`, `.mp3` o `.wav`.
- Una terminal abierta en la carpeta donde está la grabación:

  ```sh
  cd ~/Meetings
  ```

## 1. Comprueba tu configuración

```sh
minuta doctor
```

Cada línea muestra una versión o una ruta, y la línea `llm` termina en `(ok)`:

```console
ffmpeg       ffmpeg version 9.0.2 Copyright (c) 2000-2026 the FFmpeg developers (/opt/homebrew/bin/ffmpeg)
ffprobe      ffprobe version 9.0.2 Copyright (c) 2007-2026 the FFmpeg developers (/opt/homebrew/bin/ffprobe)
whisper-cli  whisper.cpp version: 1.9.5 (/opt/homebrew/bin/whisper-cli)
model        .../.minuta/models/ggml-base.bin
llm          ollama:gemma4:26b (ok)
```

Si no es así, detente aquí y arréglalo con [la guía de doctor](fix-doctor.es.md).

## 2. Escribe la transcripción

```sh
minuta transcribe weekly-sync.m4a
```

minuta lista sus pasos y termina con la ruta de una carpeta nueva, la carpeta del
trabajo:

```console
note: VAD disabled (no VAD model configured)
[1/5] probe       0.0s
[2/5] join        single source  0.0s
[3/5] audio       0.0s
[4/5] transcribe  language=en  4 segments  0.5s
[5/5] outputs     0.0s
finished in 0.8s (5 run, 0 cached)
.../minuta-out/weekly-sync
```

La línea `note: VAD disabled` es información, no un error. Una reunión real tarda más;
el paso de transcripción es el lento.

La carpeta del trabajo es `minuta-out/weekly-sync/`. Contiene `transcript.txt` (el
texto con marcas de tiempo) y `captions.srt` (subtítulos). Lee la transcripción:

```sh
cat minuta-out/weekly-sync/transcript.txt
```

```text
Welcome to the Weekly Sailing. Maria reported that the website redesign is on track and will launch on Friday.
Tom said the payment book is fixed, but he is locked on a new test server.
We decided to move the launch review to Thursday. Action items. Maria will send a final copy by Wednesday.
Tom will ask Heidi for the test server today. Thanks everyone.
```

## 3. Corrige una transcripción con errores

La transcripción del ejemplo tiene errores: "Weekly Sailing" debería ser "Weekly
Sync" y "Heidi" debería ser "IT". El modelo predeterminado, `base`, es pequeño y
rápido. Un modelo más grande oye mejor.

1. Descarga el modelo `small` (465 MiB):

   ```sh
   minuta models pull small
   ```

   ```console
   pulling small (465.0 MiB) into .../.minuta/models
   small downloaded and verified
   .../.minuta/models/ggml-small.bin
   ```

2. Transcribe de nuevo con él. minuta nota que el modelo cambió y repite solo la
   transcripción:

   ```sh
   minuta transcribe weekly-sync.m4a --model small
   ```

   ```console
   note: VAD disabled (no VAD model configured)
   [1/5] probe       cached
   [2/5] join        cached
   [3/5] audio       cached
   [4/5] transcribe  language=en  8 segments  0.8s
   [5/5] outputs     0.0s
   finished in 1.0s (2 run, 3 cached)
   .../minuta-out/weekly-sync
   ```

3. Lee la transcripción otra vez:

   ```text
   Welcome to the Weekly Sync.
   Maria reported that the website redesign is on track and will launch on Friday.
   Tom said the payment buck is fixed, but he is locked on the new test server.
   We decided to move the launch review to Thursday.
   Action items.
   Maria will send the final copy by Wednesday.
   Tom will ask IT for the test server today.
   Thanks everyone.
   ```

   Mejor. Una palabra sigue mal ("buck"). Ningún modelo es perfecto, así que en el
   paso 6 revisarás el acta contra la transcripción.

4. Deja `small` como modelo predeterminado para no volver a usar `--model`. Abre el
   archivo de configuración (`minuta config path` imprime dónde está;
   `minuta config init` lo crea) y define el modelo en `[whisper]`:

   ```toml
   [whisper]
   model = "small"
   ```

**Si no funcionó.** `unknown model` significa que el nombre no está en
`minuta models list`. En una grabación larga con silencios, whisper puede inventar
texto; [la guía de modelos](llm-and-whisper.es.md) muestra la solución (paso 5 de la
Parte 1).

## 4. Consigue una plantilla de acta

minuta trae una plantilla integrada `summary`. Para un acta, con las decisiones y las
tareas primero, descarga la plantilla ya hecha `meeting-minutes`. Se guarda en una
carpeta `templates` dentro de la carpeta actual:

```sh
minuta templates pull meeting-minutes --yes
```

```console
meeting-minutes.md  written
.../templates/meeting-minutes.md
```

Comprueba que minuta la ve:

```sh
minuta templates list
```

La lista ahora tiene una fila con la capa `project`:

```console
meeting-minutes  project   transcript  markdown  (default)  Meeting minutes with decisions and actions first, followed by brief discussion
```

**Si no funcionó.** `pull` necesita conexión a internet. Sin ella, escribe tu propia
plantilla: [Agrega y reemplaza tus propias plantillas](custom-templates.es.md).

## 5. Genera el acta

```sh
minuta run weekly-sync.m4a -t meeting-minutes
```

La transcripción ya existe, así que minuta la reutiliza y solo le pide al modelo de
lenguaje que escriba el acta. Las últimas líneas son:

```console
finished in 2.6s (1 run, 5 cached)
.../minuta-out/weekly-sync
```

El resultado es `minuta-out/weekly-sync/meeting-minutes.md`:

```sh
cat minuta-out/weekly-sync/meeting-minutes.md
```

Esto es un ejemplo. Tus palabras serán distintas:

```markdown
# weekly-sync

**Duration:** 0:00:25
**Attendees:** Maria, Tom

## Overview
The meeting covered the status of the website redesign, a fix for the payment bug, and a scheduling change for the launch review.

## Decisions
* The launch review was moved to Thursday. [00:00:00]

## Action Items
* [ ] Maria: Send the final copy by Wednesday. [00:00:00]
* [ ] Tom: Ask IT for the test server today. [00:00:00]

## Discussion
* Maria reported that the website redesign is on track for a launch on Friday. [00:00:00]
* Tom stated that the payment bug is fixed but noted he is locked out of the new test server. [00:00:00]
```

**Si no funcionó.**

- `unknown template 'meeting-minutes'`: la plantilla no está en la carpeta actual.
  Ejecuta `minuta templates list`; si `meeting-minutes` no aparece, ve a la carpeta
  donde ejecutaste `pull`.
- `not reachable` o `model missing`: tu modelo de lenguaje no está listo. Consulta
  [la guía de doctor](fix-doctor.es.md#5-llm-no-está-en-ok).

## 6. Revisa el acta antes de compartirla

Un modelo puede equivocarse, así que lee el acta una vez contra la transcripción.
Revisa esto, en orden:

1. **Nombres.** ¿Cada nombre coincide con lo que dijo la gente?
2. **Números y fechas.** "Wednesday" y "Thursday" del ejemplo coinciden con la
   transcripción.
3. **Responsables.** ¿Cada tarea está junto a la persona correcta?
4. **Nada inventado.** Cada línea debe estar en la transcripción. Borra lo que no.

La plantilla le indica al modelo que no invente nada. Aun así conviene revisar.

Si quieres un cambio, edita `meeting-minutes.md` a mano; es un archivo de texto
simple. Para que el modelo la escriba de nuevo, ejecuta el comando del paso 5 con
`--force meeting-minutes` después del nombre del archivo. Eso repite solo esta
plantilla:

```sh
minuta run weekly-sync.m4a -t meeting-minutes --force meeting-minutes
```

## 7. Escribe el acta en otro idioma

Los documentos usan el idioma de la grabación. Para obtenerlos en otro idioma,
indícalo con `--output-lang`:

```sh
minuta run weekly-sync.m4a -t meeting-minutes --output-lang es
```

Solo se vuelve a escribir el acta. Los títulos y el texto salen en español:

```markdown
# weekly-sync

**Duración:** 0:00:25
**Asistentes:** Maria, Tom

## Resumen de la reunión
```

Si vuelves a ejecutar el comando sin `--output-lang es`, el acta se escribe otra vez
en inglés.

## 8. Comparte el acta

La carpeta `minuta-out/weekly-sync/` contiene todo:

```console
$ ls -A minuta-out/weekly-sync
.minuta  assets  captions.srt  meeting-minutes.md  transcript.txt
```

| Archivo | Para qué sirve |
| --- | --- |
| `meeting-minutes.md` | El acta. Pégala en un correo o en una wiki, o envía el archivo |
| `transcript.txt` | El texto completo, para quien quiera comprobar un detalle |
| `captions.srt` | Subtítulos, si compartes la grabación como video |

No envíes `.minuta` ni `assets`: son para minuta. Para compartir el acta desde la
terminal, usa `cat` o copia el archivo. En macOS, `pbcopy` la pone en el
portapapeles:

```sh
pbcopy < minuta-out/weekly-sync/meeting-minutes.md
```

La grabación y la transcripción se quedan solo en tu computadora, a menos que elijas
un LLM que no sea local. En [la guía de modelos](llm-and-whisper.es.md) verás adónde
va tu transcripción.

## Siguientes pasos

- Haz tuya la plantilla: [Agrega y reemplaza tus propias plantillas](custom-templates.es.md).
- Úsala en cada reunión: `minuta run` acepta varios archivos. Consulta la
  [Referencia](../reference.es.md#comandos).
