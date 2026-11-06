# Plantillas

[English](templates.md) | **Español**

Una plantilla contiene instrucciones para que el modelo genere un documento a partir de la transcripción o del resultado de otra plantilla. Estas instrucciones se llaman prompt. Eliges qué plantillas ejecutar con `-t`. Las plantillas
de las que dependen se ejecutan primero.

## Plantillas integradas

| Nombre | Entrada | Escribe |
| --- | --- | --- |
| `summary` | transcripción | `summary.md`: panorama general, puntos clave, decisiones, tareas pendientes y preguntas abiertas |
| `chapters` | transcripción | `chapters.md` y `assets/chapters.json`: capítulos con marcas de tiempo que siguen las reglas de YouTube |
| `youtube` | transcripción | `youtube.md` y `assets/youtube.json`: título, descripción, capítulos, hashtags, etiquetas y texto de miniatura |
| `linkedin` | transcripción | `linkedin.md` y `assets/linkedin.json`: cuerpo de la publicación y hashtags |
| `instagram` | transcripción | `instagram.md` y `assets/instagram.json`: texto de la publicación y hashtags |

Por ejemplo, `-t youtube` también ejecuta `chapters`, porque `youtube` llena su
campo de capítulos con esa plantilla. Una plantilla que se ejecuta solo porque otra
la necesita escribe sus archivos en `assets/`, así que `chapters.md` aparece en la
raíz de la carpeta del trabajo solo cuando pides `-t chapters`. `summary` se ejecuta
solo con `-t summary`.

## De dónde vienen las plantillas

minuta carga plantillas desde tres lugares. Una plantilla con el mismo nombre en un
lugar posterior reemplaza a la anterior:

1. Integradas en minuta
2. Tus plantillas de usuario: `~/.minuta/templates/*.md`
3. Plantillas del proyecto: `./templates/*.md` en el directorio actual

Para personalizar una plantilla, cópiala y edita la copia. Escribe primero la copia
en el directorio actual y luego muévela: si rediriges directamente a
`~/.minuta/templates`, creas una plantilla vacía que reemplaza la que estás leyendo.

```sh
minuta templates show summary > summary.md
mkdir -p ~/.minuta/templates
mv summary.md ~/.minuta/templates/
```

> **Seguridad:** las plantillas de `./templates` se leen del directorio donde
> ejecutes minuta. Revisa las plantillas de un repositorio que clonaste antes de
> ejecutar minuta en él, igual que revisarías sus scripts.

## Descargar plantillas

Las plantillas del catálogo son archivos disponibles en la carpeta
[`templates`](https://github.com/JCIOriente/minuta/tree/main/templates) del
repositorio, listadas en `templates/index.toml` (el catálogo).
`minuta templates pull` descarga las que elijas en `./templates`:

```console
$ minuta templates pull --list
templates from https://raw.githubusercontent.com/JCIOriente/minuta/main/templates/
NAME       PLATFORM   STATE             DESCRIPTION
instagram  instagram  new               Caption with a hook in the first 125 characters
youtube    youtube    update available  Title, description, tags and thumbnail text
linkedin   linkedin   modified          Post with a hook and a closing question

$ minuta templates pull instagram youtube
minuta will download from:
  https://raw.githubusercontent.com/JCIOriente/minuta/main/templates/
  instagram.md  (new)
  youtube.md    (update available)

into .../templates

Continue? [y/N] y
instagram.md  written
youtube.md    updated
```

Los nombres de arriba son ejemplos. `--list` muestra lo que tiene el catálogo en
este momento, porque lo consulta cada vez, así que una plantilla nueva aparece sin
que salga una nueva versión de minuta. Un archivo descargado es una plantilla de
proyecto común: `minuta templates list` la muestra con la capa `project`, y
reemplaza a una plantilla de usuario o integrada del mismo nombre. Las plantillas
integradas siguen en el binario, así que el uso sin conexión no cambia.

| Opción | Qué hace |
| --- | --- |
| `NAME...` | Descarga estas plantillas |
| `--all` | Descarga todas las plantillas del catálogo |
| `--filter TEXT` | Descarga las plantillas cuyo nombre, plataforma o etiqueta contenga `TEXT` |
| `--list` | Muestra el catálogo con el estado de cada plantilla y no descarga nada |
| `--to DIR` | Pone los archivos en `DIR` en lugar de `./templates`. `--to ~/.minuta/templates` los deja disponibles en todas las carpetas |
| `--force` | Reemplaza las plantillas que editaste. Antes guarda el archivo anterior como `NAME.md.bak` |
| `--yes` | No pide confirmación |
| `--ref REF` | Rama, tag o commit desde el que se descarga (predeterminado: `main`) |

Si no eliges nada y estás en una terminal, `pull` abre una lista numerada para
elegir. Antes de descargar, muestra la URL de origen, cada archivo con su estado y
el destino, y pregunta `Continue? [y/N]`. Una barra de progreso en stderr cuenta los
archivos; no se muestra cuando la salida se envía a otro comando mediante una tubería o cuando la variable `NO_COLOR` está definida. Sin una terminal, nombra las plantillas (o usa `--all` o `--filter`) y
pasa `--yes`; de lo contrario, minuta termina sin descargar nada. La pregunta y los
mensajes van a stderr, y stdout lleva solo las rutas escritas.

Cada plantilla de la carpeta de destino está en uno de cuatro estados:

| Estado | Significado | Qué hace `pull` |
| --- | --- | --- |
| new | El archivo no está | Lo descarga |
| up to date | El archivo es igual al del repositorio | Nada |
| update available | El archivo está como minuta lo escribió y el repositorio tiene una versión más nueva | Lo actualiza |
| modified | Editaste el archivo, o minuta no lo escribió | Lo omite. Con `--force`, lo guarda como `NAME.md.bak` y luego lo reemplaza |

minuta recuerda lo que escribió en `.pulled.toml`, en la carpeta de destino: el hash
de cada archivo descargado. Así distingue tus ediciones de una versión más nueva
del repositorio. Si borras `.pulled.toml`, o no es válido, un archivo que difiere
del catálogo aparece como modified y no se sobrescribe nada. Para quitar una
plantilla, borra su archivo.

Antes de escribir nada, minuta comprueba que cada archivo coincida con el SHA-256
del catálogo y sea una plantilla válida, y no escribe ninguno si alguno falla. Las
descargas usan solo HTTPS, y se rechaza un destino, un `.pulled.toml` o una
plantilla que sea un enlace simbólico. El hash viene de la misma versión del
repositorio que el archivo, así que detecta descargas dañadas, no un cambio
malicioso en el repositorio. Para obtener contenido fijo, fija un tag o un commit
con `--ref v0.3.0`. Si una plantilla descargada lee otra plantilla que no tienes,
minuta muestra una `note:` con su nombre.

## Backends de LLM

| ID | Backend |
| --- | --- |
| `ollama:<model>` | Un modelo servido por tu Ollama local, como `ollama:gemma4:26b` |
| `claude` | Claude Code (`claude -p`) con su modelo predeterminado |
| `claude:<model>` | Claude Code con un modelo específico, como `claude:sonnet` |
| `codex` | Codex (`codex exec`) con su modelo predeterminado |
| `codex:<model>` | Codex con un modelo específico, uno que tu cuenta de Codex admita |
| `<endpoint>:<model>` | Un modelo en un [servidor compatible con OpenAI](#servidores-compatibles-con-openai) que nombraste en la configuración, como `lmstudio:google/gemma-4-26b-a4b` |

El backend se elige en este orden: la opción `--llm`, luego la clave `llm` de la
plantilla y luego `llm.default`, definida en el archivo de configuración o con
`MINUTA_LLM` (predeterminado: `ollama:gemma4:26b`). A diferencia de la opción,
`MINUTA_LLM` no anula la clave `llm` de una plantilla.

**Ollama** mantiene la transcripción en tu máquina, salvo que `ollama.url` apunte a
otra máquina.

**Claude.** minuta ejecuta `claude -p` con la cuenta con la que ya iniciaste sesión en Claude Code. minuta ejecuta Claude sin herramientas, sesiones ni servidores MCP, desde un directorio temporal
vacío. Claude envía la transcripción a Anthropic, y minuta muestra un aviso siempre
que una plantilla lo usa.

**Codex.** minuta ejecuta `codex exec` con la cuenta con la que ya iniciaste sesión en Codex (ejecuta
`codex login` una vez). minuta ejecuta Codex en un entorno restringido de solo lectura, sin archivos
de sesión y sin ninguno de los ajustes de tu `~/.codex/config.toml`, desde un
directorio temporal vacío, así que `codex` a secas usa el modelo predeterminado
integrado de Codex. Codex envía la transcripción a OpenAI, y minuta muestra un aviso
siempre que una plantilla lo usa.

Las transcripciones largas se dividen para que quepan en la ventana de contexto del
modelo y luego se combinan. En Ollama, la ventana es el menor valor entre
`ollama.num_ctx` y el límite propio del modelo. En un endpoint, es `context_tokens`.

Para ver los pasos de configuración, consulta
[la guía de instalación](install.es.md#3-elige-cómo-generar-documentos).

## Servidores compatibles con OpenAI

minuta puede usar cualquier servidor con una API compatible con OpenAI, como
[LM Studio](https://lmstudio.ai), [Colibri](https://github.com/JustVugg/colibri),
llama.cpp o vLLM, o un servicio en la nube. Dale un nombre al servidor una sola vez
en el archivo de configuración (`minuta config path` muestra dónde está):

```toml
[endpoints.lmstudio]
url = "http://127.0.0.1:1234/v1"
```

Luego usa `<endpoint>:<model>` en cualquier lugar donde vaya un backend:

```sh
minuta run meeting.mp4 -t summary --llm lmstudio:google/gemma-4-26b-a4b
```

El mismo ID funciona en la clave `llm` de una plantilla, en `llm.default` y en
`MINUTA_LLM`. Para comprobar el servidor y el modelo, defínelo como predeterminado y
ejecuta `minuta doctor`. Si el modelo es incorrecto, `doctor` lista lo que ofrece el
servidor.

| Clave en `[endpoints.<name>]` | Obligatoria | Predeterminado | Significado |
| --- | --- | --- | --- |
| `url` | sí | | URL base tal como la documenta el servidor, incluido `/v1` |
| `context_tokens` | no | `16384` | Cantidad máxima de tokens para el prompt y la respuesta de cada llamada, en conjunto. No debe superar el contexto que el servidor asigna al modelo |
| `timeout_secs` | no | `600` | Segundos de espera para una respuesta. Súbelo para modelos lentos |
| `api_key_env` | no | | El nombre de la variable de entorno que contiene la clave de API |

`<name>` empieza con una letra minúscula y usa letras minúsculas, dígitos, `-` y
`_`. Puede ser cualquier cosa excepto `ollama`, `claude` o `codex`. Declara todos
los endpoints que quieras.

**Las claves de API** nunca van en el archivo de configuración. Apunta `api_key_env` a
una variable, y minuta envía su valor como `Authorization: Bearer`:

```toml
[endpoints.openrouter]
url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"
```

**Privacidad.** Una URL en `localhost`, `127.0.0.0/8` o `::1` mantiene la
transcripción en tu máquina. Con cualquier otra URL, incluida una dirección de red
local, minuta muestra un aviso como
`summary sends its input to openrouter (openrouter.ai)`.

Los endpoints hacen una llamada a la vez, salvo que definas `llm.concurrency`.
minuta pide salida estructurada con `response_format` y, si el servidor la rechaza,
reenvía la solicitud sin ese campo. Consulta [la configuración de LM Studio y
Colibri](install.es.md#servidores-compatibles-con-openai).

## Escribir una plantilla

Crea una carpeta `templates` en el directorio donde ejecutas minuta y guarda el
ejemplo de abajo como `templates/weekly-sync.md`. El nombre del archivo es el nombre
de la plantilla, así que esta se llama `weekly-sync`. Los nombres empiezan con una
letra minúscula o un dígito y usan solo letras minúsculas, dígitos, `-` y `_`. Los
nombres `transcript`, `probe`, `join`, `audio`, `transcribe`, `outputs` y `job`
están reservados. El archivo es un prompt en Markdown con un encabezado YAML
opcional:

```markdown
---
description: Notes for our weekly sync
input: transcript
---
These are the notes of "{{job_name}}" ({{duration}}).

List what each person reported, then the blockers and who owns each one.
Use only what is said in the transcript.
```

```sh
minuta run sync.mp4 -t weekly-sync
```

minuta le indica al modelo en qué idioma escribir, así que el prompt puede estar en
cualquier idioma.

### Claves del encabezado

El encabezado es opcional, igual que cada clave, excepto que `output: fields`
necesita `fields`. Una clave desconocida produce un error.

| Clave | Valores | Predeterminado |
| --- | --- | --- |
| `description` | Texto libre, que muestra `templates list` | ninguno |
| `input` | `transcript`, o el nombre de otra plantilla cuya salida lee esta | `transcript` |
| `output` | `markdown`, o `fields` para salida estructurada (ver abajo) | `markdown` |
| `fields` | Con `output: fields`, los campos que se generan | ninguno |
| `limits` | Con `output: fields`, los límites de cada campo: `{ max_chars: N, max: N }` | ninguno |
| `format` | Con `output: fields`, cómo se presentan los campos en el archivo `.md`: `plain` o `markdown` | `plain` |
| `llm` | El backend de esta plantilla, como `ollama:qwen3:14b`, `claude` o `codex` | el predeterminado configurado |
| `lang` | El idioma en que escribe esta plantilla, como `en`. `--output-lang` lo anula | `llm.output_lang`, o si no, el idioma detectado en el audio |

La plantilla `chapters` es especial: usa `output: chapters`, que ninguna otra
plantilla puede usar. Si reemplazas `chapters`, conserva `output: chapters`. Su
único límite permitido es `limits: { chapters: { max: N, max_chars: N } }`, con
`max` de al menos 3.

### Variables

| Variable | Valor |
| --- | --- |
| `{{job_name}}` | El nombre del trabajo |
| `{{duration}}` | El tiempo final del último segmento de la transcripción, en formato `h:mm:ss` |
| `{{output_lang}}` | El nombre del idioma de salida, como `English` |
| `{{output_lang_code}}` | El código del idioma de salida, como `en` |
| `{{speech_lang}}` | El nombre del idioma detectado en el audio, como `Spanish` |

Cualquier otro `{{...}}` produce un error que lista estas variables.

### Salida estructurada (`fields`)

Una plantilla `fields` escribe `assets/<name>.json` para scripts y `<name>.md` para
leer. Los campos salen de un conjunto fijo:

- Texto: `title`, `body`, `thumbnail_text`, `notes` (para la persona que publica, no
  forma parte del texto publicado)
- Listas: `tags`, `hashtags`
- `chapters`: se llena con el resultado de la plantilla `chapters`, no lo escribe el
  modelo

`max_chars` limita la longitud de un campo en su formato final. `max` limita la
cantidad de elementos de una lista. Los modelos locales suelen ignorar las
instrucciones de longitud, así que cuando un campo supera un límite, minuta le pide
al modelo que lo repare, hasta `llm.repairs` veces (predeterminado: 2). Si el campo
sigue superando el límite, la plantilla falla.

```markdown
---
description: Short post for our team channel
input: transcript
output: fields
fields: [title, body, hashtags]
limits:
  title: { max_chars: 80 }
  body: { max_chars: 600 }
  hashtags: { max: 3 }
---
Write a short post about "{{job_name}}" from the transcript in the input.
```

## Contribuir con una plantilla

Una plantilla se agrega al repositorio, no al código de minuta, así que no necesita
una nueva versión:

1. Escribe `templates/NAME.md` en el [formato de arriba](#escribir-una-plantilla).
   Las guías de [`docs/template-briefs`](template-briefs) muestran cómo definir los criterios de calidad de una plantilla para una plataforma.
2. Agrega una entrada a `templates/index.toml` con su `name`, `description` y
   `sha256`, y opcionalmente `platform`, `tags` e `input`.
3. Obtén el `sha256` con `shasum -a 256 templates/NAME.md`. La prueba de consistencia
   `src/template/catalog/tests/repo.rs` imprime este comando cuando un hash es incorrecto.
4. Ejecuta `make ci`. La prueba falla si una entrada no tiene archivo, si un hash
   difiere, si un archivo no es una plantilla válida, si un nombre difiere del nombre
   de su archivo o si un archivo `templates/*.md` no tiene entrada.

Vuelve a actualizar el hash cada vez que edites el archivo.
