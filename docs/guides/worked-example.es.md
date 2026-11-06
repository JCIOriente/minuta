# De una grabación al acta de una reunión

[English](worked-example.md) | **Español**

**Objetivo:** convertir la grabación de una reunión en un acta que puedas enviar a
tu equipo. Vas a transcribir la grabación, corregir una transcripción con errores,
generar el acta, revisarla y compartirla.

El ejemplo es una grabación corta en inglés llamada `weekly-sync.m4a`. Usa tu
propia grabación: en cada comando, cambia la carpeta y el nombre del archivo por
los tuyos. Si un nombre tiene espacios, escríbelo entre comillas, como en
`minuta transcribe "reunion semanal.m4a"`.

La salida que muestra esta página es un ejemplo. El texto que obtengas será
distinto, igual que las versiones, las rutas, los tiempos y los conteos, y en una
terminal interactiva el progreso aparece como barras en lugar de líneas simples.

## Antes de empezar

- minuta está instalada y `minuta doctor` no muestra `missing`. Si no, sigue
  [Resuelve lo que reporta doctor](fix-doctor.es.md).
- Tienes un modelo de lenguaje para los documentos, como Ollama con el modelo
  predeterminado. Si no, sigue [Configura un LLM y un modelo de whisper](llm-and-whisper.es.md).
- Tienes la grabación de una reunión en un archivo que ffmpeg pueda leer, como
  `.mp4`, `.m4a`, `.mp3` o `.wav`.
- Una terminal abierta en la carpeta donde está la grabación. Cambia
  `~/Meetings` por esa carpeta:

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

La carpeta del trabajo lleva el nombre de la grabación: aquí,
`minuta-out/weekly-sync/`. Contiene `transcript.txt` (texto sin marcas de tiempo)
y `captions.srt` (subtítulos con marcas de tiempo). Lee la transcripción:

```sh
cat minuta-out/weekly-sync/transcript.txt
```

```text
Welcome to the Weekly Sailing. Maria reported that the website redesign is on track and will launch on Friday.
Tom said the payment book is fixed, but he is locked on a new test server.
We decided to move the launch review to Thursday. Action items. Maria will send a final copy by Wednesday.
Tom will ask Heidi for the test server today. Thanks everyone.
```

**Si no funcionó.**

- `source file not found`: el archivo no está en esta carpeta o su nombre se
  escribe de otra forma. Ejecuta `ls` para ver el nombre exacto y usa comillas si
  tiene espacios.
- `holds a job made from different sources`: otra grabación ya usó el nombre
  `weekly-sync`. Elige un nombre nuevo con `--name`, como
  `minuta transcribe weekly-sync.m4a --name weekly-sync-2`, y usa en los comandos
  siguientes la carpeta que imprime minuta.

Hay más errores en [Solución de problemas](../reference.es.md#solución-de-problemas).

## 3. Corrige una transcripción con errores

La transcripción del ejemplo tiene errores: "Weekly Sailing" debería ser "Weekly
Sync" y "Heidi" debería ser "IT". Se hizo con `base`, el modelo que configura la
guía de instalación. `base` es pequeño y rápido. Un modelo más grande oye mejor.

Si tu transcripción ya es suficientemente buena, o ya usas `small` o un modelo
mayor, pasa al paso 4.

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
   transcripción y los archivos que salen de ella:

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

   ```sh
   cat minuta-out/weekly-sync/transcript.txt
   ```

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

   Mejor, pero "buck" y "locked on" siguen mal. Ningún modelo es perfecto, así que
   en el paso 7 revisarás el acta contra la grabación.

4. Deja `small` como modelo predeterminado para no volver a usar `--model`.
   Necesitas este paso para continuar con este ejemplo: el paso 5 no usa
   `--model`. Ejecuta `minuta config path` para ver dónde está el archivo de configuración y
   ábrelo. En `[whisper]`, cambia la línea `model`:

   ```toml
   [whisper]
   model = "small"
   ```

   Cambia la línea que ya existe. No agregues una segunda sección `[whisper]`:
   minuta se detiene con `duplicate key`. Si todavía no tienes archivo de
   configuración, ejecuta antes `minuta config init`.

**Si no funcionó.**

- `unknown model`: el nombre no está en `minuta models list`.
- `config error: ... invalid TOML at line ...`: el archivo de configuración tiene
  un error. Abre el archivo que imprime `minuta config path` y corrige la línea
  que indica el mensaje.
- En una grabación larga con silencios, whisper puede inventar texto.
  [La guía de modelos](llm-and-whisper.es.md) muestra la solución (paso 5 de la
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
lenguaje que escriba el acta. Si el modelo predeterminado no es `small`,
minuta transcribe de nuevo con el otro modelo y reemplaza tu transcripción
corregida; vuelve antes al paso 3.4. Las últimas líneas son:

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

## 6. Elige el idioma del acta

Si el idioma ya es el correcto, omite este paso.

Si no configuras nada, los documentos usan el idioma que se habla en la
grabación. Para obtenerlos en otro idioma, indícalo con `--output-lang`:

```sh
minuta run weekly-sync.m4a -t meeting-minutes --output-lang es
```

Solo se vuelve a escribir el acta, y el archivo nuevo reemplaza al anterior. Los
títulos y el texto salen en español:

```markdown
# weekly-sync

**Duración:** 0:00:25
**Asistentes:** Maria, Tom

## Resumen de la reunión
```

Si vuelves a ejecutar el comando sin `--output-lang`, el acta se escribe otra vez
en tu idioma habitual: el que define la plantilla o el archivo de configuración o,
si no hay ninguno, el idioma de la grabación. En este ejemplo, inglés.

Elige el idioma antes de editar el acta en el paso 7. Cada vez que la plantilla se
ejecuta de nuevo, reemplaza el archivo y, con él, tus cambios.

## 7. Revisa el acta antes de compartirla

Un modelo puede equivocarse, y la transcripción también. La plantilla permite que
el modelo corrija un error de audición evidente, como hizo con "buck", así que el
acta y la transcripción pueden no coincidir. Comprueba los datos contra la propia
grabación. Las marcas de tiempo del acta, como `[00:00:00]`, y las de
`captions.srt` te indican dónde escuchar.

Revisa esto, en orden:

1. **Nombres.** ¿Cada nombre coincide con lo que dijo la gente?
2. **Números y fechas.** "Wednesday" y "Thursday" del ejemplo coinciden con la
   grabación.
3. **Responsables.** ¿Cada tarea está junto a la persona correcta?
4. **Nada inventado.** Cada dato del acta debe estar respaldado por la grabación.
   Borra lo que no lo esté.

Para hacer un cambio, edita a mano `minuta-out/weekly-sync/meeting-minutes.md`; es
un archivo de texto simple. Antes de volver a ejecutar la plantilla, guarda una
copia del archivo editado, por ejemplo:

```sh
cp minuta-out/weekly-sync/meeting-minutes.md acta-weekly-sync.md
```

Para que el modelo escriba el acta de nuevo desde cero, ejecuta el comando del
paso 5 con `--force meeting-minutes`. Eso repite solo esta plantilla y reemplaza
tus cambios:

```sh
minuta run weekly-sync.m4a -t meeting-minutes --force meeting-minutes
```

## 8. Comparte el acta

La carpeta `minuta-out/weekly-sync/` contiene todo:

```console
$ ls -A minuta-out/weekly-sync
.minuta  assets  captions.srt  meeting-minutes.md  transcript.txt
```

| Archivo | Para qué sirve |
| --- | --- |
| `meeting-minutes.md` | El acta. Pégala en un correo o en una wiki, o envía el archivo |
| `transcript.txt` | El texto completo, sin marcas de tiempo, para quien quiera comprobar un detalle |
| `captions.srt` | Subtítulos con marcas de tiempo, si compartes la grabación como video |

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
- Para tu próxima reunión, ejecuta el mismo comando con su grabación. Cada reunión
  tiene su propia carpeta de trabajo. Pasa varios archivos en un solo comando solo
  cuando sean partes seguidas de la misma reunión: minuta los une en un solo
  trabajo. Consulta la [Referencia](../reference.es.md#comandos).
