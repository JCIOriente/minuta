# Resuelve lo que reporta `minuta doctor`

[English](fix-doctor.md) | **Español**

**Objetivo:** lograr que `minuta doctor` no reporte problemas, arreglando una
herramienta faltante a la vez.

`minuta doctor` revisa los programas y archivos que minuta necesita e imprime una
línea por cada uno. Es el primer comando que conviene ejecutar cuando algo no
funciona.

## Antes de empezar

- minuta está instalada: `minuta --version` imprime un número de versión. Si dice
  `command not found`, vuelve a la [guía de instalación](../install.es.md).
- Una terminal.

## 1. Ejecuta doctor y lee el resultado

```sh
minuta doctor
```

Cuando todo está bien, cada línea muestra una versión o una ruta, y el comando
termina sin mensaje de error. Esto es un ejemplo; tus versiones y rutas serán
distintas:

```console
ffmpeg       ffmpeg version 9.0.2 Copyright (c) 2000-2026 the FFmpeg developers (/opt/homebrew/bin/ffmpeg)
ffprobe      ffprobe version 9.0.2 Copyright (c) 2007-2026 the FFmpeg developers (/opt/homebrew/bin/ffprobe)
whisper-cli  whisper.cpp version: 1.9.5 (/opt/homebrew/bin/whisper-cli)
model        .../.minuta/models/ggml-small.bin
llm          ollama:gemma4:26b (ok)
```

Si algo falta, la línea dice `missing` y `doctor` agrega al final una lista que
explica cómo arreglar cada cosa:

```console
minuta: missing required tools: model
  model: run `minuta models pull base` or set whisper.model
```

Qué es cada línea:

| Línea | Qué es | Para qué se necesita |
| --- | --- | --- |
| `ffmpeg` | Lee y convierte grabaciones | Siempre |
| `ffprobe` | Lee la duración y el formato de una grabación (viene con ffmpeg) | Siempre |
| `whisper-cli` | Escribe la transcripción (parte de whisper.cpp) | Siempre |
| `model` | El archivo del modelo de whisper | Siempre |
| `llm` | El modelo de lenguaje que escribe los documentos | Solo cuando usas `-t` |

Si falta una herramienta o el modelo, `doctor` termina con el estado 1. Un problema
en la línea `llm` se muestra pero no cambia el estado de salida, así que revisa esa
línea por tu cuenta.

Si `doctor` imprime `config error: ...` en lugar de la tabla, el archivo de
configuración tiene un error. Ejecuta `minuta config path`, abre el archivo que
imprime y corrige la línea que indica el mensaje. Una causa frecuente es una
sección escrita dos veces, como `[whisper]`: deja solo una y junta sus líneas.

Arregla los problemas uno a la vez, de arriba abajo. Después de cada arreglo,
ejecuta `minuta doctor` otra vez. Busca tu problema a continuación.

## 2. Falta `ffmpeg` o `ffprobe`

Los dos vienen en el mismo paquete.

1. Instálalo con el comando que `doctor` muestra junto al nombre. En macOS y
   Linux, con Homebrew, es:

   ```sh
   brew install ffmpeg
   ```

   En Windows, o sin Homebrew, sigue la
   [guía de instalación](../install.es.md#1-instala-minuta-y-sus-herramientas).

2. Ejecuta `minuta doctor` otra vez. Las líneas `ffmpeg` y `ffprobe` ahora muestran
   una versión.

**Si no funcionó.**

- `brew: command not found`: instala primero [Homebrew](https://brew.sh), como
  explica la guía de instalación, y abre una terminal nueva.
- La herramienta está instalada pero `doctor` sigue diciendo que falta:
  1. Abre una terminal nueva y ejecuta `minuta doctor` otra vez. Una terminal
     abierta antes de la instalación puede no ver el programa nuevo.
  2. Ejecuta `minuta config show` y mira la línea `ffmpeg.bin`. Si termina en
     `(file)` o `(env)`, minuta usa ese valor en lugar de buscar el programa.
     Corrígelo o quítalo.
  3. Si no, el programa está en una carpeta donde tu terminal no busca. Encuentra
     su ruta completa. Homebrew instala los programas en `/opt/homebrew/bin` en
     los Mac con Apple silicon, en `/usr/local/bin` en los Mac con Intel y en
     `/home/linuxbrew/.linuxbrew/bin` en Linux. Luego escribe esa ruta en el
     archivo de configuración (`minuta config path` muestra dónde está;
     `minuta config init` lo crea). Para ffprobe haz lo mismo con `[ffprobe]`:

     ```toml
     [ffmpeg]
     bin = "/opt/homebrew/bin/ffmpeg"
     ```

  Si la ruta está mal, el error dice `configured path ... does not exist or is not
  executable`, y así puedes corregirla.
- ``found at ... but `-version` failed``: el programa está ahí pero no se ejecuta.
  Vuelve a instalarlo.

## 3. Falta `whisper-cli`

1. Instala whisper.cpp, que incluye `whisper-cli`, con el comando que muestra
   `doctor`. Con Homebrew es:

   ```sh
   brew install whisper.cpp
   ```

   Sin Homebrew, sigue la
   [guía de instalación](../install.es.md#1-instala-minuta-y-sus-herramientas).

2. Ejecuta `minuta doctor` otra vez. La línea `whisper-cli` muestra una versión.

**Si no funcionó.** Haz las mismas comprobaciones de la sección 2, con la línea
`whisper.bin` y la sección `[whisper]`.

## 4. Falta `model`

El modelo de whisper es un archivo que se descarga una sola vez.

1. Descárgalo:

   ```sh
   minuta models pull base
   ```

   ```console
   pulling base (141.1 MiB) into .../.minuta/models
   base downloaded and verified
   .../.minuta/models/ggml-base.bin
   ```

   En una terminal interactiva, la descarga muestra una barra de progreso.

2. Dile a minuta que lo use. Descargarlo no basta: el archivo de configuración
   tiene que nombrar el modelo. Si aún no tienes archivo de configuración,
   ejecuta `minuta config init` para crearlo. Luego abre el archivo que imprime
   `minuta config path` y, en `[whisper]`, define la línea `model` (borra el `# `
   del principio):

   ```toml
   [whisper]
   model = "base"
   ```

   Cambia la línea que ya existe. No agregues una segunda sección `[whisper]`.

3. Ejecuta `minuta doctor` otra vez. La línea `model` muestra una ruta.

**Si no funcionó.** Lee la segunda línea del error:

| Mensaje | Causa y solución |
| --- | --- |
| ``run `minuta models pull base` or set whisper.model`` | No hay ningún modelo definido. Haz el paso 2 |
| ``run `minuta models pull small` `` | La configuración nombra un modelo que no has descargado. Ejecuta ese comando |
| `configured path /some/file.bin is not a file` | `model` es una ruta y ahí no hay nada. Corrige la ruta o usa un nombre de `minuta models list` |
| `... is a VAD model; set it as whisper.vad_model` | El modelo de detección de voz está en `model`. Muévelo a `vad_model` |
| ``vad model: run `minuta models pull silero-v5.1.2` `` | `vad_model` está definido pero no descargado. Ejecuta ese comando o quita la línea |

Para ver dónde se guardan los modelos, ejecuta `minuta models list`. La primera
línea muestra la carpeta.

## 5. `llm` no está en `ok`

La línea `llm` muestra el id del modelo de lenguaje predeterminado y, entre
paréntesis, su estado. El estado depende de qué modelo sea. Busca tu id en la
primera columna.

| El id empieza con | Estado en `doctor` | Qué hacer |
| --- | --- | --- |
| `ollama:` | `not reachable: start Ollama ...` | Inicia Ollama: `ollama serve`. Si Ollama está en otra computadora, define `url` en `[ollama]` |
| `ollama:` | `model missing: ollama pull <model>` | Ejecuta el comando que imprime |
| `claude` | `not reachable: install Claude Code: ...` | Instala [Claude Code](../install.es.md#claude-code) y ejecuta `claude` una vez para iniciar sesión |
| `codex` | `not reachable: install Codex: ...` | Instala [Codex](../install.es.md#codex) y ejecuta `codex login` |
| un nombre que elegiste, como `lmstudio:` | `invalid: unknown endpoint ...` | Agrega el servidor al archivo de configuración. El mensaje muestra las líneas que debes agregar |
| igual | `not reachable: start the server or fix endpoints.<name>.url ...` | Inicia el servidor o corrige su `url` |
| igual | `not reachable: set <VARIABLE> to the API key ...` | Define esa variable de entorno con tu clave, en la misma terminal |
| igual | `model missing: not offered by the server; it offers: ...` | Usa uno de los nombres de modelo de esa lista |
| cualquier otro | `invalid: invalid LLM backend ...` | El id no está bien escrito. El mensaje lista las formas válidas |

Con `claude` y `codex`, `(ok)` solo significa que el programa arranca. `doctor` no
comprueba que hayas iniciado sesión. Para probarlos de principio a fin, genera un
resumen corto, como `minuta run meeting.mp4 -t summary --llm claude` con tu propia
grabación. Si falla con un error de inicio de sesión, vuelve a ejecutar `claude`
(o `codex login`).

Si no usas `-t`, puedes ignorar esta línea. Los pasos completos para configurar cada
LLM están en [Configura un LLM y un modelo de whisper](llm-and-whisper.es.md).

Para comprobar otro LLM sin cambiar el archivo de configuración, defínelo para un
solo comando. Cambia `claude` por el id que quieras comprobar:

```sh
MINUTA_LLM=claude minuta doctor
```

## 6. Comprueba que terminaste

```sh
minuta doctor
echo $?
```

`doctor` no imprime `missing` y el número que imprime `echo` es `0`. Si quieres
generar documentos, la línea `llm` también termina en `(ok)`.

Si el problema sigue, ejecuta `minuta config show`. Lista cada ajuste, su valor y de
dónde viene, como `(file)`, `(env)` o `(default)`. Un valor distinto del que esperas
suele explicarlo.

## Siguientes pasos

- Prueba con una grabación real: [De una grabación al acta de una reunión](worked-example.es.md).
- Más errores y soluciones: [Solución de problemas](../reference.es.md#solución-de-problemas).
