# Agrega y reemplaza tus propias plantillas

[English](custom-templates.md) | **Español**

**Objetivo:** escribir una plantilla propia, ejecutarla y después reemplazar una de
las plantillas integradas de minuta por tu versión.

Una plantilla es un archivo de texto con un prompt. minuta envía el prompt y tu
transcripción a un modelo de lenguaje y guarda la respuesta como un documento.
Eliges la plantilla con `-t`, por ejemplo `-t summary`.

## Antes de empezar

- minuta está instalada, `minuta doctor` termina correctamente y su línea `llm`
  termina en `(ok)`. Si no, consulta [Resuelve lo que reporta doctor](fix-doctor.es.md)
  y [Configura un LLM y un modelo de whisper](llm-and-whisper.es.md).
- Tienes una grabación. En esta página se usa `meeting.mp4`; usa el nombre de tu archivo.
  Los ejemplos suponen una grabación en inglés; el idioma de tus documentos
  depende de tu grabación y de tu configuración.
- Una terminal abierta en la carpeta donde está la grabación.

## Parte 1: escribe una plantilla nueva

1. Mira qué plantillas tiene ya minuta:

   ```sh
   minuta templates list
   ```

   Obtienes una tabla. Cada fila es una plantilla. La columna `LAYER` indica de
   dónde viene: `built-in` son las que vienen dentro de minuta.

   ```console
   NAME       LAYER     INPUT       OUTPUT    LLM        DESCRIPTION
   chapters   built-in  transcript  chapters  (default)  Timestamped chapters (YouTube rules)
   instagram  built-in  transcript  fields    (default)  Instagram caption about the recording
   linkedin   built-in  transcript  fields    (default)  LinkedIn post about the recording
   summary    built-in  transcript  markdown  (default)  Short summary of the recording
   youtube    built-in  transcript  fields    (default)  YouTube title, description, tags and thumbnail text
   ```

2. Crea una carpeta llamada `templates` junto a tu grabación. minuta lee las
   plantillas de una carpeta con ese nombre, dentro de la carpeta donde la ejecutas:

   ```sh
   mkdir -p templates
   ```

   Si la carpeta ya existe, el comando la deja como está.

3. Crea el archivo `templates/weekly-sync.md` con este contenido. El nombre del
   archivo es el nombre de la plantilla, así que esta se llama `weekly-sync`.

   ```markdown
   ---
   description: Notas de nuestra reunión semanal
   input: transcript
   ---
   Estas son las notas de "{{job_name}}" ({{duration}}).

   Enumera lo que reportó cada persona, luego los bloqueos y quién es responsable
   de cada uno. Usa solo lo que se dice en la transcripción.
   ```

   Lo que hay entre las dos líneas `---` es el encabezado. `description` es el
   texto que muestra `templates list`. `input: transcript` significa que la
   plantilla lee la transcripción. Todo lo que va después del encabezado es el
   prompt. minuta reemplaza `{{job_name}}` y `{{duration}}` por sus valores.

   El nombre de una plantilla empieza con una letra minúscula o un dígito y usa
   solo letras minúsculas, dígitos, `-` y `_`. Algunos nombres, como `transcript`
   y `outputs`, están reservados; la lista está en
   [Escribir una plantilla](../templates.es.md#escribir-una-plantilla).

4. Comprueba que minuta la ve:

   ```sh
   minuta templates list
   ```

   Hay una fila nueva, con la capa `project`:

   ```console
   weekly-sync  project   transcript  markdown  (default)  Notas de nuestra reunión semanal
   ```

5. Ejecútala:

   ```sh
   minuta run meeting.mp4 -t weekly-sync
   ```

   minuta muestra su progreso y, al final, la ruta de la carpeta del trabajo. El
   documento es `weekly-sync.md` en esa carpeta. Para `meeting.mp4` la carpeta es
   `minuta-out/meeting`; si minuta imprime otra ruta, usa esa:

   ```sh
   cat minuta-out/meeting/weekly-sync.md
   ```

   Las palabras de la respuesta dependen de tu modelo. La grabación del ejemplo
   está en inglés, así que la respuesta también; esto es un ejemplo:

   ```text
   ### Weekly Sync Report

   **Reports**
   * **Maria:** The website redesign is on track and will launch on Friday.
   * **Tom:** The payment bug is fixed.

   **Blockers**
   * **Tom:** Locked out of the new test server.
   ```

**Si no funcionó.** Ejecuta `minuta templates list`. Un archivo que minuta no puede
leer aparece al final, en una línea que empieza con `invalid`, con el motivo. Por
ejemplo, un error de escritura como `{{who}}` da:

```console
invalid  .../templates/oops.md  template oops: unknown variable 'who' (known: output_lang, output_lang_code, speech_lang, job_name, duration)
```

Corrige el archivo y ejecuta el comando otra vez. La lista completa de claves del
encabezado y de variables está en
[Escribir una plantilla](../templates.es.md#escribir-una-plantilla).

## Parte 2: reemplaza una plantilla integrada

Puedes reemplazar una plantilla integrada creando una con el mismo nombre. Lo más
fácil es copiar la integrada y cambiar la copia.

1. Copia la plantilla integrada `summary` en un archivo de la carpeta actual:

   ```sh
   minuta templates show summary > summary.md
   ```

   minuta muestra `built-in` en la pantalla, lo que significa que copiaste la
   original. Si en cambio muestra la ruta de un archivo, ya tienes tu propio
   `summary` y copiaste ese.

   No escribas la copia directamente dentro de la carpeta `templates`. La terminal
   vacía el archivo antes de que minuta lea la plantilla, y terminarías con un
   `summary` vacío que reemplaza al verdadero.

2. Abre `summary.md` en cualquier editor de texto y cambia el prompt. Por ejemplo,
   agrega al final una línea: `Empieza el resumen con el título "# Resumen de la reunión".`
   Deja el encabezado al principio.

3. Mueve el archivo al lugar donde quieres que se aplique.

   - **Solo para esta carpeta:** ponlo en `templates`.

     ```sh
     mkdir -p templates
     mv summary.md templates/
     ```

   - **En todas partes:** ponlo en tu carpeta de plantillas de usuario.

     ```sh
     mkdir -p ~/.minuta/templates
     mv summary.md ~/.minuta/templates/
     ```

4. Comprueba que se usa tu versión. La capa ya no es `built-in`:

   ```sh
   minuta templates list
   ```

   ```console
   summary    user      transcript  markdown  (default)  Short summary of the recording
   ```

   Dice `user` si usaste `~/.minuta/templates`, o `project` si usaste `templates`.

5. Ejecútala como siempre. El nombre `summary` ahora significa tu versión:

   ```sh
   minuta run meeting.mp4 -t summary
   ```

   Comprueba que el documento sigue tu cambio. Con la línea del ejemplo, el
   resumen empieza con tu título:

   ```sh
   head -n 1 minuta-out/meeting/summary.md
   ```

   ```console
   # Resumen de la reunión
   ```

   Puede que el modelo no siga esta línea al pie de la letra (puede cambiar las
   palabras o envolver el texto en un bloque de código). La comprobación que
   vale es la capa que `minuta templates list` mostró en el paso 4.

Para volver a la plantilla integrada, borra tu archivo.

**Cómo elige minuta.** Hay tres lugares y gana el último: primero las plantillas
integradas, luego `~/.minuta/templates` y por último `./templates` en la carpeta
actual.

**Si no funcionó.**

- La fila `summary` sigue diciendo `built-in`: el archivo no está en la carpeta
  correcta o no termina en `.md`. Ejecuta `minuta templates show summary` para ver
  qué archivo usa minuta; su ruta se imprime en la salida de errores.
- La fila `summary` dice `project`, pero pusiste tu archivo en
  `~/.minuta/templates`: un `templates/summary.md` en la carpeta actual tiene
  prioridad. Borra o renombra ese archivo.
- La fila `summary` no tiene descripción y el documento no es un resumen: el archivo
  está vacío, porque escribiste la copia directamente en la carpeta `templates`.
  Borra el archivo y empieza de nuevo desde el paso 1.
- El documento no cambió después de editar la plantilla: comprueba que guardaste
  el archivo y que `minuta templates list` muestra la capa que esperas. minuta
  detecta una plantilla modificada y la vuelve a ejecutar por su cuenta. Para
  ejecutarla de nuevo de todos modos, agrega `--force summary` después del nombre
  del archivo.

> **Cuidado.** minuta lee `./templates` de la carpeta donde la ejecutes. Si clonas
> un proyecto de otra persona, lee sus plantillas antes de ejecutar minuta allí.

## Parte 3: descarga plantillas ya hechas

El repositorio de minuta tiene un catálogo de plantillas ya hechas.

1. Lístalas. Necesitas conexión a internet:

   ```sh
   minuta templates pull --list
   ```

   ```console
   templates from https://raw.githubusercontent.com/JCIOriente/minuta/main/templates/
   NAME                PLATFORM   STATE  DESCRIPTION
   action-items        meeting    new    Flat action checklist sorted by owner with stated deadlines, blockers and timestamps
   meeting-minutes     meeting    new    Meeting minutes with decisions and actions first, followed by brief discussion
   ...
   ```

   El catálogo cambia con el tiempo, así que tu lista será distinta.

2. Descarga una en `./templates`:

   ```sh
   minuta templates pull meeting-minutes --yes
   ```

   ```console
   meeting-minutes.md  written
   .../templates/meeting-minutes.md
   ```

   Sin `--yes`, minuta muestra lo que va a descargar y pregunta
   `Continue? [y/N]`. Escribe `y` para continuar.

3. Úsala como cualquier otra plantilla: `minuta run meeting.mp4 -t meeting-minutes`.
   El documento es `meeting-minutes.md` en la carpeta del trabajo.

Un archivo descargado es una plantilla normal: puedes editarlo. Si lo editas y más
tarde ejecutas `pull` otra vez, minuta omite tu archivo en lugar de sobrescribirlo.
Agrega `--force` para reemplazarlo; antes se guarda tu versión como
`meeting-minutes.md.bak`.

## Siguientes pasos

- Todas las claves del encabezado, las variables y la salida estructurada con
  `fields`: [Plantillas](../templates.es.md).
- Elige qué modelo de lenguaje escribe los documentos:
  [Configura un LLM y un modelo de whisper](llm-and-whisper.es.md).
- Una ejecución completa con una plantilla ya hecha:
  [De una grabación al acta de una reunión](worked-example.es.md).
