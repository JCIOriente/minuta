# Configura un LLM y un modelo de whisper

[English](llm-and-whisper.md) | **Español**

**Objetivo:** elegir el modelo de whisper que convierte la voz en texto y el modelo
de lenguaje (LLM) que escribe tus documentos, y comprobar que ambos funcionan.

minuta usa dos tipos de modelo:

- Un **modelo de whisper** escucha la grabación y escribe la transcripción. Se
  ejecuta en tu computadora. Siempre necesitas uno.
- Un **LLM** lee la transcripción y escribe documentos, como un resumen. Solo lo
  necesitas cuando usas `-t`. Puede ejecutarse en tu computadora o en un servicio.

## Antes de empezar

- minuta, `ffmpeg` y `whisper-cli` están instalados. Consulta la
  [guía de instalación](../install.es.md).
- Una grabación corta para hacer pruebas. Esta página la llama `meeting.mp4`;
  cámbiala por el nombre de tu archivo.
- Una terminal abierta en la carpeta donde está la grabación.

La salida que muestra esta página es un ejemplo. Tus versiones, rutas, modelos
instalados y tiempos serán distintos, y en una terminal interactiva una descarga
muestra una barra de progreso.

## Parte 1: elige un modelo de whisper

1. Mira los modelos que minuta puede descargar y cuáles ya tienes:

   ```sh
   minuta models list
   ```

   ```console
   models in .../.minuta/models
   tiny           available  74.1 MiB
   base           installed  141.1 MiB
   small          available  465.0 MiB
   medium         available  1.4 GiB
   large-v3       available  2.9 GiB
   silero-v5.1.2  available  0.8 MiB
   ```

   Un modelo más grande es más preciso y más lento. `base` es una buena primera
   opción. Usa `small` o uno mayor si los nombres y las palabras técnicas salen mal.
   `silero-v5.1.2` no es un modelo de whisper: es el modelo opcional de VAD del paso 5.

2. Descarga uno. minuta comprueba el archivo después de descargarlo:

   ```sh
   minuta models pull small
   ```

   ```console
   pulling small (465.0 MiB) into .../.minuta/models
   small downloaded and verified
   .../.minuta/models/ggml-small.bin
   ```

3. Hazlo el modelo predeterminado, para no tener que nombrarlo en cada ejecución.
   Crea el archivo de configuración, que trae todos los ajustes comentados:

   ```sh
   minuta config init
   ```

   ```console
   created .../.minuta/config.toml
   ```

   Si el archivo ya existe, el comando dice `already exists; nothing changed` y
   puedes seguir. `minuta config path` imprime dónde está el archivo.

4. Abre ese archivo en un editor de texto. Bajo la línea `[whisper]`, busca la
   línea `model`. Si empieza con `# `, borra el `# `. Escribe el modelo que
   descargaste:

   ```toml
   [whisper]
   model = "small"
   ```

   Cambia las líneas que ya existen. No agregues una segunda sección `[whisper]`:
   minuta se detiene con `duplicate key`.

   Guarda el archivo. Para probar un modelo una sola vez sin cambiar el archivo,
   agrega `--model small` al comando: `minuta run meeting.mp4 --model small`.

5. Opcional: reduce el texto inventado en los silencios. En grabaciones largas con
   partes en silencio, whisper a veces escribe frases que nadie dijo. Un modelo
   extra pequeño, llamado modelo de VAD, ayuda:

   ```sh
   minuta models pull silero-v5.1.2
   ```

   Luego, en la misma sección `[whisper]`, agrega la línea `vad_model`:

   ```toml
   [whisper]
   model = "small"
   vad_model = "silero-v5.1.2"
   ```

6. Compruébalo:

   ```sh
   minuta doctor
   ```

   La línea `model` muestra la ruta del archivo en lugar de `missing`:

   ```console
   model        .../.minuta/models/ggml-small.bin
   ```

**Si no funcionó.**

- `model missing`: el archivo no está descargado. `doctor` imprime el comando que
  lo resuelve, como ``run `minuta models pull small` ``.
- `unknown model`: usa uno de los nombres de `minuta models list`.
- `is a VAD model; set it as whisper.vad_model`: pusiste `silero-v5.1.2` en
  `model`. Muévelo a `vad_model`.
- `config error: ... invalid TOML at line ...`: el archivo de configuración tiene
  un error, como una segunda sección `[whisper]`. Corrige la línea que indica el
  mensaje.
- ¿Ya tienes un archivo de modelo de whisper.cpp? Pon en `model` su ruta completa,
  por ejemplo `model = "/Users/ana/models/ggml-medium.bin"`.

## Parte 2: elige un LLM

La tabla muestra adónde va tu transcripción. Elige una opción y sigue su sección.

| Opción | Tu transcripción va a | Elígela si |
| --- | --- | --- |
| Ollama | Tu computadora | Quieres todo privado. Es la opción predeterminada |
| Claude Code | Anthropic | Ya usas Claude Code |
| Codex | OpenAI | Ya usas Codex |
| Otro servidor | Ese servidor | Usas LM Studio, Colibri o un servicio en la nube |

Elijas lo que elijas, nombras el LLM con un id: `ollama:<modelo>`, `claude`,
`codex` o `<nombre-del-servidor>:<modelo>`. Las partes entre `<` y `>` son
marcadores: reemplázalas, signos incluidos. Puedes pasar el id en una sola
ejecución con `--llm`, o guardarlo como predeterminado en `[llm]` del archivo de
configuración.

### Ollama (en tu computadora)

1. Instala Ollama siguiendo [Ollama](../install.es.md#ollama) en la guía de
   instalación.
2. Inícialo. La app de macOS y el servicio de Linux lo inician por ti; si no,
   ejecuta esto en una terminal y déjala abierta:

   ```sh
   ollama serve
   ```

3. En otra terminal, descarga un modelo. Elige uno de estos y descarga solo ese:

   - **El modelo predeterminado de minuta, `gemma4:26b`.** Es una descarga grande
     (consulta su tamaño actual en [ollama.com/library](https://ollama.com/library))
     y necesita una computadora con bastante memoria. Después de
     descargarlo no hay nada que configurar:

     ```sh
     ollama pull gemma4:26b
     ```

   - **Un modelo más pequeño**, de [ollama.com/library](https://ollama.com/library).
     Antes, compara su tamaño con la memoria de tu computadora. Por ejemplo, para
     `qwen3:14b`:

     ```sh
     ollama pull qwen3:14b
     ```

     Luego déjalo como predeterminado. En el archivo de configuración, busca
     `# [llm]` y la línea `# default = ...` que sigue. Borra el `# ` del principio
     de ambas y escribe tu modelo después de `ollama:`:

     ```toml
     [llm]
     default = "ollama:qwen3:14b"
     ```

4. Ejecuta `minuta doctor`. La línea `llm` termina en `(ok)` (consulta
   [Comprueba tu configuración](#comprueba-tu-configuración)).

Si Ollama se ejecuta en otra computadora, define `url` en `[ollama]` con su
dirección. Entonces tus transcripciones van a esa computadora. Si las respuestas se
cortan, sube `num_ctx` en `[ollama]`, siempre que el modelo admita una ventana mayor.

### Claude Code o Codex

minuta usa la sesión que ya tienes. No hay clave que configurar.

1. Instala la herramienta e inicia sesión una vez, como se explica en
   [Claude Code](../install.es.md#claude-code) o [Codex](../install.es.md#codex)
   en la guía de instalación.
2. Úsala en una ejecución:

   ```sh
   minuta run meeting.mp4 -t summary --llm claude
   ```

   Usa `--llm codex` para Codex. minuta muestra un aviso de que la transcripción
   se envía a la empresa. Al terminar, el resumen está en la carpeta del trabajo
   que imprime minuta, aquí `minuta-out/meeting/summary.md`:

   ```sh
   cat minuta-out/meeting/summary.md
   ```

   Esta ejecución es la prueba real. `minuta doctor` solo comprueba que el
   programa arranca; no comprueba que hayas iniciado sesión. Si la ejecución
   falla con un error de inicio de sesión o de autenticación, vuelve a ejecutar
   `claude` (o `codex login`).
3. Para dejarla como predeterminada, define `default = "claude"` (o `"codex"`) en
   `[llm]`.

### Otro servidor (LM Studio, Colibri, un servicio en la nube)

Usa un servidor con una API compatible con OpenAI: debe responder en `/models` y
`/chat/completions`. LM Studio, Colibri, llama.cpp y vLLM lo hacen. Le das un
nombre al servidor una sola vez, en el archivo de configuración, y luego usas ese
nombre en el id. Los pasos para LM Studio y Colibri están en
[Servidores compatibles con OpenAI](../install.es.md#servidores-compatibles-con-openai).

1. Inicia el servidor y carga un modelo con un contexto de al menos 16384
   tokens. Anota la dirección del servidor, que normalmente termina en `/v1`, y
   el nombre exacto del modelo.
2. En el archivo de configuración, agrega una sección con el nombre que quieras.
   Este ejemplo llama `lmstudio` al servidor:

   ```toml
   [endpoints.lmstudio]
   url = "http://127.0.0.1:1234/v1"
   ```

   El nombre empieza con una letra minúscula (de la `a` a la `z`) y usa solo
   letras minúsculas, dígitos, `-` y `_`. No puede ser `ollama`, `claude` ni
   `codex`. Si el servidor le da al modelo un contexto menor de 16384 tokens,
   agrega `context_tokens` con ese número.
3. Si el servidor pide una clave, no la escribas en el archivo. Escribe en
   `api_key_env` el nombre de una variable de entorno. Este ejemplo es un
   servicio en la nube llamado `openrouter`:

   ```toml
   [endpoints.openrouter]
   url = "https://openrouter.ai/api/v1"
   api_key_env = "OPENROUTER_API_KEY"
   ```

   Luego define la variable en la terminal donde ejecutas minuta. Cambia el texto
   entre comillas por tu clave:

   ```sh
   export OPENROUTER_API_KEY='replace-with-your-key'
   ```

   La variable dura hasta que cierras esta terminal. Para conservarla, agrega la
   misma línea al archivo de inicio de tu shell, como `~/.zshrc` o `~/.bashrc`.
4. Comprueba el servidor y el modelo. El id es el nombre, dos puntos y el nombre
   del modelo tal como lo lista el servidor:

   ```sh
   MINUTA_LLM=lmstudio:google/gemma-4-26b-a4b minuta doctor
   ```

   La última línea termina en `(ok)`:

   ```console
   llm          lmstudio:google/gemma-4-26b-a4b (ok)
   ```

5. Úsalo:

   ```sh
   minuta run meeting.mp4 -t summary --llm lmstudio:google/gemma-4-26b-a4b
   ```

   El resumen está en la carpeta del trabajo que imprime minuta, aquí
   `minuta-out/meeting/summary.md`.

Un servidor que no está en tu computadora recibe tu transcripción, y minuta muestra
un aviso cada vez. Los servidores lentos necesitan más tiempo: sube `timeout_secs`
en la sección (el valor predeterminado es 600 segundos). Todas las claves están en
[Servidores compatibles con OpenAI](../templates.es.md#servidores-compatibles-con-openai).

## Comprueba tu configuración

```sh
minuta doctor
```

La última línea es tu LLM predeterminado y termina en `(ok)`:

```console
llm          ollama:gemma4:26b (ok)
```

Para comprobar otro LLM sin cambiar el archivo de configuración, defínelo para un
solo comando con la variable de entorno `MINUTA_LLM`. Cambia `claude` por el id
que quieras comprobar:

```sh
MINUTA_LLM=claude minuta doctor
```

Con Claude Code y Codex, `(ok)` significa que el programa arranca, no que hayas
iniciado sesión. Ejecuta un `minuta run ... -t summary` corto para probarlos de
principio a fin.

Para ver qué valor usa minuta y de dónde viene, ejecuta `minuta config show`. Un
`(file)` o un `(env)` al final de la línea te lo indica.

**Si no funcionó.** El texto entre paréntesis dice qué falla:

| `doctor` dice | Qué hacer |
| --- | --- |
| `not reachable: start Ollama ...` | Inicia Ollama con `ollama serve`. Si está en otra parte, define `ollama.url` |
| `model missing: ollama pull <model>` | Ejecuta el comando que imprime |
| `not reachable: install Claude Code: ...` o `install Codex: ...` | Instala esa herramienta e inicia sesión |
| `invalid: unknown endpoint "lmstudio"; add it to the config file...` | Agrega la sección `[endpoints.lmstudio]` |
| `model missing: not offered by the server; it offers: ...` | Usa uno de los nombres de la lista |
| `not reachable: start the server or fix endpoints.lmstudio.url...` | Inicia el servidor o corrige `url` |
| `not reachable: set OPENROUTER_API_KEY to the API key ...` | Ejecuta la línea `export` del paso 3 en la misma terminal |
| `... 401 ...; check the key in endpoints.openrouter.api_key_env` (o `403`) | El servidor rechazó la clave. Comprueba que la copiaste completa y que sigue siendo válida |

Más soluciones: [Resuelve lo que reporta doctor](fix-doctor.es.md).

## Siguientes pasos

- Ejecuta todo con una grabación real:
  [De una grabación al acta de una reunión](worked-example.es.md).
- Todos los ajustes y variables de entorno:
  [Configuración](../reference.es.md#configuración).
