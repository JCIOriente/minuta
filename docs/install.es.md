# Instalar minuta

[English](install.md) | **Español**

Elige tu sistema y sigue sus pasos de arriba abajo. Luego
[descarga un modelo](#2-descarga-un-modelo-de-whisper) y
[elige cómo generar documentos](#3-elige-cómo-generar-documentos).

| Necesitas | Para | ¿Obligatorio? |
| --- | --- | --- |
| Rust 1.88 o más reciente | Compilar minuta | Siempre |
| `ffmpeg` y `ffprobe` | Leer, unir y convertir archivos multimedia | Siempre |
| `whisper-cli` de whisper.cpp | Transcripción | Siempre |
| Un modelo de whisper | Transcripción | Siempre |
| Ollama, Claude Code, Codex o un servidor compatible con OpenAI | Generar documentos | Solo para `minuta run -t ...` |

## 1. Instala minuta y sus herramientas

### macOS

1. Instala [Homebrew](https://brew.sh) si no lo tienes.
2. Instala ffmpeg y whisper.cpp. En Apple Silicon, esta compilación de whisper usa
   la GPU a través de Metal:

   ```sh
   brew install ffmpeg whisper-cpp
   ```

3. Instala Rust. Sigue las instrucciones en pantalla:

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source "$HOME/.cargo/env"
   ```

4. Instala minuta:

   ```sh
   cargo install --locked minuta
   ```

### Linux

Estos comandos son para Debian y Ubuntu. En otra distribución, consulta
[otras distribuciones](#otras-distribuciones).

1. Instala ffmpeg y las herramientas de compilación:

   ```sh
   sudo apt update
   sudo apt install ffmpeg git cmake build-essential
   ```

2. Instala Rust. Sigue las instrucciones en pantalla:

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source "$HOME/.cargo/env"
   ```

3. Compila `whisper-cli` y agrégalo a tu `PATH`:

   ```sh
   git clone https://github.com/ggml-org/whisper.cpp.git
   cd whisper.cpp
   cmake -B build -DBUILD_SHARED_LIBS=OFF
   cmake --build build -j --config Release
   sudo install build/bin/whisper-cli /usr/local/bin/
   cd ..
   ```

   `-DBUILD_SHARED_LIBS=OFF` crea un solo binario autónomo, así que sigue
   funcionando fuera de la carpeta de compilación. Para usar una GPU, consulta las
   [opciones de compilación de whisper.cpp](https://github.com/ggml-org/whisper.cpp#readme),
   como CUDA o Vulkan.

4. Instala minuta:

   ```sh
   cargo install --locked minuta
   ```

#### Otras distribuciones

Instala ffmpeg y las herramientas de compilación con tu gestor de paquetes, y continúa desde el paso 2 de arriba.

| Sistema | Comando |
| --- | --- |
| Fedora | Habilita [RPM Fusion](https://rpmfusion.org/Configuration) y luego ejecuta `sudo dnf install ffmpeg git cmake gcc-c++ make` |
| Arch | `sudo pacman -S ffmpeg git cmake base-devel` |

[Homebrew](https://brew.sh) también funciona en Linux. Con él, los
[pasos de macOS](#macos) se aplican tal cual.

### Windows

minuta no se ejecuta de forma nativa en Windows. Se ejecuta dentro de WSL2, un
sistema Linux integrado en Windows.

1. Abre PowerShell como administrador, instala WSL2 con Ubuntu y reinicia si te lo
   pide:

   ```powershell
   wsl --install
   ```

2. Abre **Ubuntu** desde el menú Inicio y crea un usuario cuando te lo pida.
3. Dentro de Ubuntu, sigue los [pasos de Linux](#linux).

Ejecuta todos los comandos de minuta dentro de Ubuntu. Después de completar las secciones 2 a 4 de esta guía, puedes acceder a tus grabaciones en `/mnt`. En el ejemplo, reemplaza `<your-name>` por tu nombre de usuario de Windows:

```sh
cd /mnt/c/Users/<your-name>/Videos
minuta run meeting.mp4 -t summary
```

La carpeta del trabajo también se crea ahí, así que puedes abrirla desde Windows.
Leer desde `/mnt/c` es más lento que desde el sistema de archivos de Linux. Para procesar más rápido las grabaciones largas, cópialas primero a tu carpeta personal de Linux.

## 2. Descarga un modelo de whisper

minuta descarga los modelos, verifica su integridad mediante sumas de comprobación y los guarda en
`~/.minuta/models/`:

```sh
minuta models list         # muestra el catálogo y lo que está instalado
minuta models pull base    # descarga uno
```

| Modelo | Tamaño | Ideal para |
| --- | --- | --- |
| `tiny` | 74 MiB | Pruebas rápidas |
| `base` | 141 MiB | Una buena primera opción |
| `small` | 465 MiB | Mejor precisión a una velocidad moderada |
| `medium` | 1.4 GiB | Alta precisión |
| `large-v3` | 2.9 GiB | La mejor precisión, el más lento |

Luego déjalo como predeterminado, para no tener que pasar `--model` en cada
ejecución. Ejecuta `minuta config init` para crear el archivo de configuración,
abre el archivo en la ruta que muestra y configura lo siguiente:

```toml
[whisper]
model = "base"
```

También puedes usar un modelo de whisper.cpp que ya tengas. Define `model` con la
ruta del archivo en lugar de un nombre del catálogo.

Cada fragmento de TOML de esta guía va en ese mismo archivo. Si su sección ya
existe, edita esa sección en lugar de agregarla de nuevo.

### Opcional: detección de actividad de voz

Sin detección de actividad de voz (VAD), whisper a veces escribe frases que nadie
dijo durante silencios largos. Un modelo de VAD, de menos de 1 MiB, reduce esas
frases:

```sh
minuta models pull silero-v5.1.2
```

```toml
[whisper]
model = "base"
vad_model = "silero-v5.1.2"
```

## 3. Elige cómo generar documentos

Omite este paso si solo necesitas transcripciones. Necesitas una de estas opciones
para `minuta run -t ...`:

| Opción | Tu transcripción va a | Ideal si |
| --- | --- | --- |
| [Ollama](#ollama) | Tu máquina | Quieres todo local. Es la opción predeterminada |
| [Claude Code](#claude-code) | Anthropic | Ya usas Claude Code |
| [Codex](#codex) | OpenAI | Ya usas Codex |
| [Un servidor compatible con OpenAI](#servidores-compatibles-con-openai) | Tu máquina, o el servidor que elijas | Usas LM Studio, Colibri o un servicio en la nube |

### Ollama

[Ollama](https://ollama.com) ejecuta modelos de lenguaje en tu máquina.

| Sistema | Comando |
| --- | --- |
| macOS | `brew install ollama`, o descarga la app desde [ollama.com/download](https://ollama.com/download) |
| Linux y WSL2 | `curl -fsSL https://ollama.com/install.sh \| sh` |

Inicia el servidor y descarga el modelo predeterminado de minuta. La app de macOS
y el servicio de Linux inician el servidor por ti, así que quizá no necesites
`ollama serve`:

```sh
ollama serve              # déjalo en ejecución
ollama pull gemma4:26b    # en otra terminal
```

`gemma4:26b` necesita una máquina con bastante memoria. Para usar un modelo más
pequeño de [ollama.com/library](https://ollama.com/library), descárgalo y déjalo
como predeterminado en el archivo de configuración:

```sh
ollama pull <model>
```

```toml
[llm]
default = "ollama:<model>"
```

Si Ollama se ejecuta en otra máquina o puerto, define su URL. Las transcripciones
se envían entonces a esa máquina:

```toml
[ollama]
url = "http://192.168.1.20:11434"
```

### Claude Code

minuta usa la cuenta con la que ya iniciaste sesión en [Claude Code](https://claude.com/claude-code), así
que no hay ninguna clave de API que configurar. Instálalo con una de estas opciones:

```sh
curl -fsSL https://claude.ai/install.sh | bash     # instalador nativo
brew install --cask claude-code                    # Homebrew
npm install -g @anthropic-ai/claude-code           # npm
```

Ejecuta `claude` una vez para iniciar sesión y luego úsalo desde minuta:

```sh
minuta run meeting.mp4 -t summary --llm claude
```

Para usar Claude en todas las ejecuciones, define `default = "claude"` en `[llm]`.
La transcripción se envía a Anthropic, y minuta muestra un aviso cada vez.

### Codex

minuta usa la cuenta con la que ya iniciaste sesión en [Codex](https://github.com/openai/codex), así que
no hay ninguna clave de API que configurar. Instálalo con una de estas opciones:

```sh
npm install -g @openai/codex                       # npm
brew install --cask codex                          # Homebrew
```

Ejecuta `codex login` una vez y luego úsalo desde minuta:

```sh
minuta run meeting.mp4 -t summary --llm codex
minuta run meeting.mp4 -t summary --llm codex:<model>   # un modelo específico
```

Para usar Codex en todas las ejecuciones, define `default = "codex"` en `[llm]`.
La transcripción se envía a OpenAI, y minuta muestra un aviso cada vez.

### Servidores compatibles con OpenAI

Dale un nombre al servidor en el archivo de configuración y luego usa
`<endpoint>:<model>` como backend. `minuta config init` escribe un ejemplo
comentado. Consulta
[todas las claves de un endpoint](templates.es.md#servidores-compatibles-con-openai).

#### LM Studio

1. Instala [LM Studio](https://lmstudio.ai) y descarga un modelo.
2. Inicia el servidor y carga el modelo con un contexto de al menos
   `context_tokens` (16384 por defecto):

   ```sh
   lms server start
   lms load google/gemma-4-26b-a4b --context-length 16384
   ```

3. Agrega el servidor al archivo de configuración:

   ```toml
   [endpoints.lmstudio]
   url = "http://127.0.0.1:1234/v1"
   ```

4. Úsalo. `lms ls` muestra los nombres de los modelos:

   ```sh
   minuta run meeting.mp4 -t summary --llm lmstudio:google/gemma-4-26b-a4b
   ```

#### Colibri

1. Instala [Colibri](https://github.com/JustVugg/colibri) siguiendo su README e
   inícialo con `coli serve`.
2. Ejecuta `coli status` para obtener el ID del modelo, o búscalo en
   `http://127.0.0.1:8000/v1/models`. Puede ser distinto del nombre con el que
   iniciaste el modelo: Qwen3.8-Flash-Next es `qwen3.8-flash-next-colibri`.
3. Agrega el servidor al archivo de configuración. Colibri puede tardar mucho más
   que el límite predeterminado de 10 minutos, así que sube `timeout_secs`:

   ```toml
   [endpoints.colibri]
   url = "http://127.0.0.1:8000/v1"
   timeout_secs = 3600
   ```

4. Úsalo:

   ```sh
   minuta run meeting.mp4 -t summary --llm colibri:qwen3.8-flash-next-colibri
   ```

minuta se probó con Qwen3.8-Flash-Next en Colibri. Con esa configuración, una
transcripción de 5 minutos tardó unos 15 minutos por llamada, así que espera
ejecuciones largas. Su motor `qwen38` rechaza `response_format`, así que minuta
reenvía esas solicitudes sin ese campo.

#### Otro servidor

Define `url` con la URL base que indique la documentación del servidor, que suele
terminar en `/v1`. Si necesita una clave, guárdala en una variable de entorno y
pon el nombre de la variable en `api_key_env`:

```toml
[endpoints.openrouter]
url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"
```

Con una URL que no esté en tu máquina, la transcripción se envía a ese servidor, y
minuta muestra un aviso cada vez.

## 4. Comprueba tu instalación

```sh
minuta doctor
```

`doctor` muestra cada herramienta, el modelo y el estado del backend de LLM
predeterminado:

```console
$ minuta doctor
ffmpeg       ffmpeg version 8.0 ...
ffprobe      ffprobe version 8.0 ...
whisper-cli  whisper.cpp version: 1.8.0 ...
model        missing
llm          ollama:gemma4:26b (ok)
minuta: missing required tools: model
  model: run `minuta models pull base` or set whisper.model
```

Si falta una herramienta o el modelo, termina con código de salida 1 y muestra cómo
resolver cada caso. Un backend de LLM inaccesible se muestra, pero no cambia el
código de salida.

Si una herramienta está instalada en un lugar que no está en tu `PATH`, indícale a
minuta dónde está con `ffmpeg.bin`, `ffprobe.bin` o `whisper.bin`
([consulta Configuración](reference.es.md#configuración)).

Cuando `doctor` termine sin errores, ve al [inicio rápido](../README.es.md#inicio-rápido) y empieza
en el paso 2, porque ya tienes un modelo.
