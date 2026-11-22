# Starling 0.9.0 — edición en español (Android)

Esta carpeta se basa en la **versión 0.9.0 del código fuente** de
`starling-client/starling`, del 20 de noviembre de 2026.

## Qué se cambió

- `crates/horizon-client/assets/horizon/lang/es_es.json`: se completaron las **547 traducciones que faltaban** en el paquete propio de idioma del juego. Hay ahora 1.595/1.595 claves de referencia, conservando las variables `%s`, `%d`, etc.
- `launcher/lang/es.json`: se tradujeron más mensajes del lanzador de escritorio y de diagnósticos de compilación. Su archivo contiene las 1.354 claves de referencia.
- `crates/horizon-client-render/src/settings.rs`: se usa `es_es` por defecto **solo al crear ajustes nuevos en Android**. Si ya existe `options.txt`, se respeta el idioma que el jugador haya elegido.
- `packaging/android/res/values/*.xml` y clases Java: textos del diagnóstico Android, importar mundos, permisos de mods, notificaciones, ventana de reinicio y aviso sobre servidor de compilación traducidos.
- `.github/workflows/starling-es-arm64.yml`: flujo de compilación de una APK ARM64 de prueba.

**Se han conservado intencionalmente** los nombres propios y términos de protocolo (Starling, Minecraft, Microsoft, Vulkan, `mod`, ABI, rutas, comandos, versiones, hashes y códigos técnicos). Las traducciones de contenido añadido por mods y mensajes que envían servidores externos no dependen del idioma del ejecutable. La interfaz no se ha probado físicamente en Android.

## Cómo obtener la APK mediante GitHub Actions

1. Crea un **fork** de https://github.com/starling-client/starling en tu cuenta de GitHub.
2. Copia sobre el fork los archivos modificados de este ZIP (o aplica el parche entregado) **en el commit de la etiqueta `v0.9.0`**. Sube los cambios a una rama llamada `espanol`.
3. En tu fork, abre **Actions**. Si GitHub te pregunta, habilita los workflows de tu fork.
4. Abre **Starling Español - APK Android ARM64** y selecciona **Run workflow** en la rama `espanol`. Los cambios en esa rama también pueden iniciar el workflow automáticamente.
5. Al terminar la compilación, abre la ejecución, descarga el artefacto `Starling-0.9.0-Espanol-ARM64` y extrae `starling-preview-0.9.0-arm64-v8a.apk`.
6. Instala esa APK en un teléfono Android ARM64. Es una edición de prueba con identificador `io.github.juanluci.starling.preview`; se instala **al lado** de la versión oficial, para evitar errores de firma. Usa su propio directorio de mundos y configuraciones.

La compilación se realiza en servidores de GitHub con Java 25, Rust 1.99.1, Android NDK 28.2.13676358, Android SDK 36 y build-tools 35.0.0 según los requisitos del repositorio. Este ZIP **contiene código fuente, no una APK previamente compilada**.

## Compilar localmente (si tienes las herramientas)

En Linux con el Android SDK / NDK requerido, Java 25, Rust 1.99.1 con `rust-src`, objetivo `aarch64-linux-android` y `cargo-ndk` 3.5.4:

```bash
python3 tools/android_native_sdk.py prepare
RUSTFLAGS='-C prefer-dynamic' python3 packaging/android/build_apk.py --abi arm64-v8a --profile quick --preview --zip
```

APK resultante: `target/android/starling-preview-0.9.0-arm64-v8a.apk`.

## Validación realizada

```bash
python3 tools/lang_coverage.py --lang es_es es --summary
python3 -m pytest tests/test_lang_coverage.py tests/test_sync_lang.py -q
```

Estas comprobaciones validan el formato, las claves de idioma y los marcadores. No reemplazan una compilación real ni pruebas visuales en Android.
