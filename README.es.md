<p align="center">
  <img src="https://raw.githubusercontent.com/JCIOriente/minuta/main/assets/logo.svg" alt="Logo de minuta: tres barras de forma de onda que se convierten en tres líneas de texto" width="96" height="96">
</p>

<h1 align="center">minuta</h1>

<p align="center">Convierte grabaciones de reuniones, clases y webinars en transcripciones y documentos escritos.</p>

<p align="center">
  <a href="https://crates.io/crates/minuta"><img src="https://img.shields.io/crates/v/minuta" alt="crates.io version"></a>
  <a href="https://www.npmjs.com/package/@jcioriente/minuta"><img src="https://img.shields.io/npm/v/@jcioriente/minuta" alt="npm version"></a>
  <a href="https://github.com/JCIOriente/minuta/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/JCIOriente/minuta/ci.yml?branch=main" alt="CI status"></a>
  <a href="https://github.com/JCIOriente/minuta/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="License: Apache-2.0"></a>
</p>

[English](https://github.com/JCIOriente/minuta/blob/main/README.md) | **Español**

Convierte grabaciones de reuniones, clases y webinars en transcripciones y
documentos escritos, como un resumen, capítulos con marcas de tiempo, textos para
YouTube o una publicación de LinkedIn.

```console
$ minuta run standup.mp4 -t summary -t youtube
.../minuta-out/standup

$ ls minuta-out/standup
assets  captions.srt  summary.md  transcript.txt  youtube.md
```

- **Local por defecto.** La transcripción siempre se ejecuta en tu máquina con
  [whisper.cpp](https://github.com/ggml-org/whisper.cpp). Los documentos los
  escribe un modelo local de [Ollama](https://ollama.com), salvo que elijas
  [Claude Code](https://claude.com/claude-code),
  [Codex](https://github.com/openai/codex) u otro servidor.
- **Las plantillas son archivos Markdown simples.** Escribe las tuyas o
  encadénalas para que una plantilla lea la salida de otra.
- **Reanudable.** Cada paso queda en caché. Si interrumpes una ejecución o cambias
  una plantilla, la siguiente ejecución rehace solo lo que cambió.

## Instalación

minuta funciona en macOS, Linux y Windows (a través de WSL2).

**[Sigue la guía de instalación](https://github.com/JCIOriente/minuta/blob/main/docs/install.es.md)**
para tu sistema. Explica cómo instalar Rust, ffmpeg y whisper.cpp, y cómo configurar el servicio que generará los documentos (backend de LLM).

Si ya tienes Node.js 22 o más reciente, instálalo con npm. No necesita Rust y
funciona en macOS (Apple Silicon e Intel) y Linux (x64 y arm64); en Windows,
ejecútalo dentro de WSL2:

```sh
npm i -g @jcioriente/minuta
```

npm instala solo minuta. ffmpeg, whisper.cpp y un backend de LLM se siguen
instalando aparte, como explica la guía de instalación. Para probar minuta sin
instalarlo, ejecuta `npx @jcioriente/minuta doctor`.

Si ya tienes Rust 1.88 o más reciente:

```sh
cargo install --locked minuta
```

Luego revisa tu instalación. `doctor` te dice qué falta y cómo resolverlo:

```sh
minuta doctor
```

## Inicio rápido

Primero instala minuta y sus herramientas. Reemplaza `meeting.mp4` por la ruta de
tu grabación, y pon entre comillas cualquier ruta con espacios.

1. **Descarga un modelo de whisper y déjalo como predeterminado.** `base` es una
   buena primera opción. Omite este paso si ya lo hiciste en la guía de instalación; si no, sigue
   [Descarga un modelo de whisper](https://github.com/JCIOriente/minuta/blob/main/docs/install.es.md#2-descarga-un-modelo-de-whisper).

2. **Transcribe una grabación.**

   ```sh
   minuta transcribe meeting.mp4
   ```

   Obtienes `transcript.txt` y `captions.srt` en `minuta-out/meeting/`.

3. **Genera un documento.** Esto usa tu backend predeterminado, Ollama con
   `gemma4:26b` si no lo cambiaste. Para usar otro backend, agrega su ID, por
   ejemplo `--llm claude`.


   ```sh
   minuta run meeting.mp4 -t summary
   ```

   La transcripción ya está en caché, así que solo se genera el resumen. El
   resultado es `minuta-out/meeting/summary.md`.

## Más ejemplos

```sh
# Une dos partes de una clase, en orden, en un trabajo llamado "class-4"
minuta run part1.mp4 part2.mp4 --name class-4 -t summary

# Título, descripción, etiquetas y capítulos para YouTube, escritos con Claude
minuta run webinar.mov -t youtube --llm claude

# Una reunión en español, con las notas escritas en inglés
minuta run reunion.m4a --lang es --output-lang en -t summary

# La ruta de la carpeta del trabajo va a stdout, así que puedes usarla en scripts
cat "$(minuta run talk.mp4 -t linkedin -q)/linkedin.md"
```

Las plantillas integradas son `summary`, `chapters`, `youtube`, `linkedin` e
`instagram`. Las
[plantillas del catálogo](https://github.com/JCIOriente/minuta/blob/main/docs/templates.es.md#plantillas-del-catálogo)
se descargan con `minuta templates pull`.

## Documentación

| Guía | Qué cubre |
| --- | --- |
| [Instalación](https://github.com/JCIOriente/minuta/blob/main/docs/install.es.md) | Configuración en macOS, Linux y Windows, modelos y backends de LLM |
| [Guías prácticas](https://github.com/JCIOriente/minuta/blob/main/docs/guides/README.es.md) | Paso a paso: tus propias plantillas, un LLM y un modelo de whisper, resolver lo que reporta `minuta doctor` y de una grabación al acta de una reunión |
| [Plantillas](https://github.com/JCIOriente/minuta/blob/main/docs/templates.es.md) | Plantillas integradas, cómo descargar más, cómo escribir las tuyas y cómo elegir un backend |
| [Referencia](https://github.com/JCIOriente/minuta/blob/main/docs/reference.es.md) | Comandos, la carpeta del trabajo, caché, configuración y solución de problemas |
| [Estabilidad](https://github.com/JCIOriente/minuta/blob/main/docs/stability.es.md) | Qué promete la 1.0 que no se rompe, qué puede cambiar y cómo funcionan las obsolescencias |

Ejecuta `minuta help <command>` para ver el uso completo de cualquier comando.

## Desarrollo

```sh
git clone https://github.com/JCIOriente/minuta.git
cd minuta
make ci        # rustfmt, clippy y pruebas, las mismas comprobaciones que ejecuta CI
```

El diseño está en
[`docs/specs`](https://github.com/JCIOriente/minuta/tree/main/docs/specs), y
las decisiones detrás de él están en
[`docs/adr`](https://github.com/JCIOriente/minuta/tree/main/docs/adr).

## Licencia

minuta se distribuye bajo la
[Apache License, Version 2.0](https://github.com/JCIOriente/minuta/blob/main/LICENSE).
