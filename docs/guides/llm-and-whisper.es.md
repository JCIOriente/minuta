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
- Una terminal.

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

   Si el archivo ya existe, el comando no lo toca y puedes omitir este paso.
   `minuta config path` imprime dónde está el archivo.

4. Abre ese archivo en un editor de texto. Bajo la línea `[whisper]`, busca
   `# model = "small"`, borra el `# ` del principio y escribe el modelo que
   descargaste:

   ```toml
   [whisper]
   model = "small"
   ```

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
`codex` o `<nombre-del-servidor>:<modelo>`. Puedes pasarlo en una sola ejecución con
`--llm`, o guardarlo como predeterminado (consulta el paso 4 de la sección de Ollama).

### Ollama (en tu computadora)

1. Instala [Ollama](https://ollama.com). En macOS, `brew install ollama`; en Linux,
   `curl -fsSL https://ollama.com/install.sh | sh`.
2. Inícialo. La app de macOS y el servicio de Linux lo inician por ti; si no,
   ejecuta esto en una terminal y déjala abierta:

   ```sh
   ollama serve
   ```

3. En otra terminal, descarga el modelo predeterminado de minuta. Es grande (unos
   17 GB) y necesita una computadora con bastante memoria:

   ```sh
   ollama pull gemma4:26b
   ```

4. Para usar un modelo más pequeño, busca uno en
   [ollama.com/library](https://ollama.com/library), ejecuta `ollama pull <modelo>`
   y déjalo como predeterminado. En el archivo de configuración, busca `# [llm]` y
   la línea `# default = ...` que sigue. Borra el `# ` del principio de ambas y
   escribe tu modelo:

   ```toml
   [llm]
   default = "ollama:<modelo>"
   ```

   El prefijo `ollama:` se queda. Para `qwen3:14b` escribes `ollama:qwen3:14b`.

5. Ejecuta `minuta doctor` (consulta [Comprueba tu configuración](#comprueba-tu-configuración)).

Si Ollama se ejecuta en otra computadora, define `url` en `[ollama]` con su
dirección. Entonces tus transcripciones van a esa computadora. Si las respuestas se
cortan, sube `num_ctx` en `[ollama]`, siempre que el modelo admita una ventana mayor.

### Claude Code o Codex

minuta usa la sesión que ya tienes. No hay clave que configurar.

1. Instala y entra una sola vez: ejecuta `claude` para Claude Code, o
   `codex login` para Codex. Los pasos de instalación están en la
   [guía de instalación](../install.es.md#claude-code).
2. Úsalo en una ejecución:

   ```sh
   minuta run meeting.mp4 -t summary --llm claude
   ```

   Usa `--llm codex` para Codex. minuta muestra un aviso de que la transcripción se
   envía a la empresa.
3. Para dejarlo como predeterminado, define `default = "claude"` (o `"codex"`) en `[llm]`.

### Otro servidor (LM Studio, Colibri, un servicio en la nube)

Funciona cualquier servidor que hable la API de OpenAI. Le das un nombre una sola
vez, en el archivo de configuración, y luego usas ese nombre en el id.

1. Inicia el servidor y carga un modelo. Anota la dirección del servidor, que
   normalmente termina en `/v1`, y el nombre exacto del modelo.
2. En el archivo de configuración, agrega una sección con el nombre que quieras.
   Este ejemplo llama `lmstudio` al servidor:

   ```toml
   [endpoints.lmstudio]
   url = "http://127.0.0.1:1234/v1"
   ```

   El nombre usa letras minúsculas, dígitos, `-` y `_`. No puede ser `ollama`,
   `claude` ni `codex`.
3. Úsalo. El id es el nombre, dos puntos y el nombre del modelo tal como lo lista
   el servidor:

   ```sh
   minuta run meeting.mp4 -t summary --llm lmstudio:google/gemma-4-26b-a4b
   ```

4. Si el servidor pide una clave, no la escribas en el archivo. Guárdala en una
   variable de entorno y escribe el nombre de la variable:

   ```toml
   [endpoints.openrouter]
   url = "https://openrouter.ai/api/v1"
   api_key_env = "OPENROUTER_API_KEY"
   ```

Un servidor que no está en tu computadora recibe tu transcripción, y minuta muestra
un aviso cada vez. Los servidores lentos necesitan más tiempo: sube `timeout_secs`
en la sección (el valor predeterminado es 600 segundos). Todas las claves están en
[Servidores compatibles con OpenAI](../templates.es.md#servidores-compatibles-con-openai).

## Comprueba tu configuración

Para comprobar el LLM, déjalo como predeterminado o defínelo para un solo comando con
la variable de entorno `MINUTA_LLM`, y luego ejecuta `doctor`:

```sh
MINUTA_LLM=ollama:gemma4:26b minuta doctor
```

La última línea es el LLM:

```console
llm          ollama:gemma4:26b (ok)
```

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

Más soluciones: [Resuelve lo que reporta doctor](fix-doctor.es.md).

## Siguientes pasos

- Ejecuta todo con una grabación real:
  [De una grabación al acta de una reunión](worked-example.es.md).
- Todos los ajustes y variables de entorno:
  [Configuración](../reference.es.md#configuración).
