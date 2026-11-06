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
termina sin mensaje de error:

```console
ffmpeg       ffmpeg version 9.0.2 Copyright (c) 2000-2026 the FFmpeg developers (/opt/homebrew/bin/ffmpeg)
ffprobe      ffprobe version 9.0.2 Copyright (c) 2007-2026 the FFmpeg developers (/opt/homebrew/bin/ffprobe)
whisper-cli  whisper.cpp version: 1.9.5 (/opt/homebrew/bin/whisper-cli)
model        .../.minuta/models/ggml-small.bin
llm          ollama:gemma4:26b (ok)
```

Tus versiones y rutas serán distintas. Si algo falta, la línea dice `missing` y
`doctor` agrega al final una lista que explica cómo arreglar cada cosa:

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

Arregla los problemas uno a la vez, de arriba abajo. Después de cada arreglo,
ejecuta `minuta doctor` otra vez. Busca tu problema a continuación.

## 2. Falta `ffmpeg` o `ffprobe`

Los dos vienen en el mismo paquete.

1. Instálalo. En macOS y Linux, con Homebrew:

   ```sh
   brew install ffmpeg
   ```

   En otro sistema, consulta la [guía de instalación](../install.es.md).

2. Ejecuta `minuta doctor` otra vez. Las líneas `ffmpeg` y `ffprobe` ahora muestran
   una versión.

**Si no funcionó.**

- `brew: command not found`: instala primero [Homebrew](https://brew.sh), como
  explica la guía de instalación, y abre una terminal nueva.
- La herramienta está instalada pero `doctor` sigue diciendo que falta: está en una
  carpeta donde minuta no busca. Encuentra su ruta completa con `which ffmpeg` y
  díselo a minuta en el archivo de configuración (`minuta config path` muestra
  dónde está). Para ffprobe haz lo mismo con `[ffprobe]`:

  ```toml
  [ffmpeg]
  bin = "/ruta/completa/a/ffmpeg"
  ```

  Si la ruta está mal, el error dice `configured path ... does not exist or is not
  executable`, y así puedes corregirla.
- ``found at ... but `-version` failed``: el programa está ahí pero no se ejecuta.
  Vuelve a instalarlo.

## 3. Falta `whisper-cli`

1. Instala whisper.cpp, que incluye `whisper-cli`:

   ```sh
   brew install whisper.cpp
   ```

2. Ejecuta `minuta doctor` otra vez. La línea `whisper-cli` muestra una versión.

**Si no funcionó.** Igual que en la sección 2: si `whisper-cli` está instalado en un
lugar donde minuta no busca, define `bin` en `[whisper]` con su ruta completa.

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

2. Dile a minuta que lo use. En el archivo de configuración, define `model` en
   `[whisper]`. Si aún no tienes archivo de configuración, ejecuta
   `minuta config init` para crearlo:

   ```toml
   [whisper]
   model = "base"
   ```

3. Ejecuta `minuta doctor` otra vez. La línea `model` muestra una ruta.

**Si no funcionó.** Lee la segunda línea del error:

| Mensaje | Causa y solución |
| --- | --- |
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
| `claude` | `not reachable: install Claude Code: ...` | Instala [Claude Code](https://claude.com/claude-code) y ejecuta `claude` una vez para iniciar sesión |
| `codex` | `not reachable: install Codex: ...` | Instala Codex (`npm install -g @openai/codex`) y ejecuta `codex login` |
| un nombre que elegiste, como `lmstudio:` | `invalid: unknown endpoint ...` | Agrega el servidor al archivo de configuración. El mensaje muestra las líneas que debes agregar |
| igual | `not reachable: start the server or fix endpoints.<name>.url ...` | Inicia el servidor o corrige su `url` |
| igual | `model missing: not offered by the server; it offers: ...` | Usa uno de los nombres de modelo de esa lista |
| cualquier otro | `invalid: invalid LLM backend ...` | El id no está bien escrito. El mensaje lista las formas válidas |

Si no usas `-t`, puedes ignorar esta línea. Los pasos completos para configurar cada
LLM están en [Configura un LLM y un modelo de whisper](llm-and-whisper.es.md).

Para comprobar otro LLM sin cambiar el archivo de configuración, defínelo para un
solo comando:

```sh
MINUTA_LLM=claude minuta doctor
```

## 6. Comprueba que terminaste

```sh
minuta doctor
echo $?
```

`doctor` no imprime `missing` y el número que imprime `echo` es `0`. La línea `llm`
termina en `(ok)` si quieres generar documentos.

Si el problema sigue, ejecuta `minuta config show`. Lista cada ajuste, su valor y de
dónde viene, como `(file)`, `(env)` o `(default)`. Un valor distinto del que esperas
suele explicarlo.

## Siguientes pasos

- Prueba con una grabación real: [De una grabación al acta de una reunión](worked-example.es.md).
- Más errores y soluciones: [Solución de problemas](../reference.es.md#solución-de-problemas).
