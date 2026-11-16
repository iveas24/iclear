# Reglas de limpieza

> El borrado de iclear es **permanente**: no pasa por la Papelera. Por eso todo borrado requiere vista previa + confirmación y queda registrado en `~/Library/Logs/iclear/deletions.log`.

## Categorías (`backend/core/src/storage/categories.rs`)

Se borran los **hijos** de cada carpeta raíz, nunca la raíz misma.

| ID | Nombre | Raíces (relativas a `~`) | Riesgo | Preseleccionada |
|---|---|---|---|---|
| `user_caches` | Cachés de apps | `Library/Caches` | Seguro | Sí |
| `user_logs` | Logs | `Library/Logs`, `Library/Logs/DiagnosticReports` | Seguro | Sí |
| `xcode_derived` | Xcode DerivedData | `Library/Developer/Xcode/DerivedData` | Seguro | Sí |
| `xcode_device_support` | Soporte de dispositivos | `Library/Developer/Xcode/{iOS,watchOS,tvOS} DeviceSupport` | Moderado | No |
| `xcode_archives` | Archivos de Xcode | `Library/Developer/Xcode/Archives` | Revisar | No |
| `simulator_caches` | Cachés de simuladores | `Library/Developer/CoreSimulator/Caches` | Seguro | Sí |
| `dev_caches` | Cachés de desarrollo | `.npm/_cacache`, `Library/pnpm/store`, `.bun/install/cache`, `.cache`, `.cargo/registry/cache`, `.gradle/caches` | Moderado | No |
| `mail_downloads` | Adjuntos de Mail | `Library/Containers/com.apple.mail/Data/Library/Mail Downloads` | Seguro | Sí |
| `trash` | Papelera | `.Trash` | Seguro | Sí |
| `old_downloads` | Descargas antiguas | `Downloads` (solo > 90 días sin modificar) | Revisar | No |

Las **cachés del sistema** (`/Library/Caches`) se limpian desde *Optimizar* a través del helper.

## Guardas (`backend/core/src/storage/delete.rs` → `Guard::check`)

Una ruta solo se borra si cumple **todo**:

1. Es absoluta y no contiene `..`.
2. Está estrictamente dentro de `$HOME` (y `$HOME` es una ruta válida, no `/`).
3. No está dentro de la lista protegida: `Library/Keychains`, `Library/Mobile Documents` (iCloud), `Library/Mail`, `Library/Messages`, `Library/Photos`, `Library/Application Support/MobileSync` (respaldos de iPhone), `Library/Application Support/AddressBook`, `Library/Calendars`, `Library/CloudStorage`, `Library/Group Containers`, `Library/Logs/iclear`, `Pictures/Photos Library.photoslibrary`, `.ssh`, `.gnupg`.
4. No es una raíz de categoría.
5. Su carpeta padre no pasa por un symlink (`canonicalize(parent) == parent`).
6. Si es **carpeta**: debe estar dentro de una raíz de categoría.
   Si es **archivo**: puede estar en cualquier parte de `$HOME` (archivos grandes, duplicados), salvo dotfiles en la raíz de `$HOME`.
7. Los symlinks se borran como enlace; nunca se sigue su destino.

## Desinstalar apps y restos (`Guard::check_app_removal`)

Las vistas previas de `preview_uninstall` y `preview_orphans` quedan marcadas como **app_removal**. Al ejecutarse se validan **solo** con esta guarda; las demás vistas previas usan solo `Guard::check`, así que ninguna de las dos reglas sirve para la otra. Se acepta una ruta únicamente si es uno de estos dos casos:

1. **Paquete de app**: `X.app` directamente en `/Applications`, en una carpeta de fabricante dentro de ella o en `~/Applications`. Debe ser una carpeta con `Info.plist` válido, sin symlinks en la ruta, y no puede ser Safari ni iclear.
2. **Datos de app**: un hijo **directo** de `~/Library/{Application Support, Caches, Containers, Preferences, Saved Application State, HTTPStorages, WebKit, Application Scripts, Logs, LaunchAgents}`. Tiene que estar fuera de la lista protegida, la carpeta contenedora misma nunca se borra y no puede haber symlinks en la ruta.

El caso se decide por **ubicación** y no por extensión, porque `com.foo.app` es un nombre común para carpetas de datos.

Cómo se asignan los restos y cuándo cuenta algo como huérfano: [INTELLIGENCE.md](INTELLIGENCE.md).

## Token de confirmación

- `preview_cleanup` / `preview_paths` registran la lista exacta mostrada y devuelven un token aleatorio (128 bits de `/dev/urandom`).
- El token dura **15 minutos** y es de **un solo uso**.
- `execute_cleanup` ignora cualquier ruta que no estuviera en esa vista previa.

## Escaneos de archivos grandes y duplicados

- Omiten `~/Library`, `~/.Trash` y la fototeca de Fotos.
- Duplicados: mismo tamaño → mismo hash de los primeros 64 KB → mismo blake3 completo. Los hard links (mismo inodo) no cuentan como duplicados.

## Tests

`cargo test -p iclear-core` cubre: reglas de la guarda, escape por symlink, flujo de token (ruta fuera de la vista previa, token de un uso, log), detección de duplicados, guarda de desinstalación, restos ambiguos (no se asignan) y huérfanos conservadores.
