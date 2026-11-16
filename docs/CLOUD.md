# Nube: nubes sincronizadas, transferencias y backups

La sección **Nube** (Disco › Nube) reúne todas las nubes sincronizadas con el Mac, muestra cuánto está en este Mac y cuánto está subido, permite descargar o liberar espacio por carpeta y programa backups hacia cualquiera de ellas.

Código: `backend/core/src/cloud/` (`mod.rs`, `ubiquity.rs`, `transfers.rs`, `backup.rs`) y `frontend/src/pages/cloud.rs`.

## Qué nubes detecta

| Origen | Ruta | Id |
|---|---|---|
| iCloud Drive | `~/Library/Mobile Documents/com~apple~CloudDocs` | `icloud` |
| Proveedores de File Provider (Google Drive, OneDrive, Dropbox, Box, Proton Drive, pCloud, MacDroid…) | `~/Library/CloudStorage/<Proveedor>-<cuenta>` | nombre de la carpeta |

Cada cuenta es una nube distinta (dos cuentas de Google Drive aparecen por separado). Si la raíz es de solo lectura (Google Drive), los backups van a la primera carpeta con escritura: `Mi unidad` / `My Drive`.

## Cómo se mide el estado

Una sola ruta de código sirve para todos los proveedores:

- **Solo en la nube**: flag `SF_DATALESS` de `stat` (sin red, muy barato).
- **Subido / subiendo / descargando / conflicto**: claves `NSURLUbiquitousItem*` de Foundation (vía `objc2-foundation`). Las responden iCloud y todos los dominios de File Provider.
- **% en este Mac** = bytes no-placeholder / bytes totales.
- **% subido** = (bytes totales − bytes pendientes de subir) / bytes totales.

### Rendimiento

Una nube real puede tener más de un millón de archivos. Orden de magnitud medido en un Mac con Apple Silicon:

| Archivos en la nube | Escaneo |
|---|---|
| ~1 millón | ~20 s |
| ~400 mil | ~10 s |
| ~300 mil | ~6 s |

Para lograrlo:

1. **Escaneo en segundo plano** por nube (`Scans`), con cifras que crecen en vivo (la UI las refresca con cada tick de métricas) y límite de 20 min.
2. **El estado de subida se consulta solo para archivos modificados en los últimos 14 días** (`STATUS_WINDOW_DAYS`). Lo pendiente de subir siempre es un cambio reciente; los archivos anteriores se cuentan como subidos. Preguntarle al proveedor por cada archivo de una nube de un millón de archivos tomaría horas.
3. **Las carpetas que están solo en la nube no se listan** (eso obligaría al proveedor a traerlas de la red): se cuentan como `unlisted_folders`.
4. El escaneo guarda el total de cada carpeta, así que explorar es instantáneo. Sin caché, cada listado tiene un presupuesto de 2,5 s y marca los tamaños como parciales (`partial`, se muestran con `+`).

## Acciones sobre archivos

| Acción | Cómo | Seguridad |
|---|---|---|
| **Descargar** (archivo o carpeta) | `NSFileManager.startDownloadingUbiquitousItem` por archivo; si el proveedor no lo implementa, se lee 1 byte para que lo descargue. Avance = bytes ya materializados. | Solo rutas dentro de una nube (`cloud::resolve`). |
| **Liberar espacio** | `NSFileManager.evictUbiquitousItem` por archivo. | Solo archivos ya confirmados como subidos; los que tienen cambios pendientes se omiten y se informan. No borra nada de la nube. |

## Transferencias

Descargas, backups y restauraciones son `Transfer` con progreso en vivo (`list_transfers`):

| Tipo | Barras |
|---|---|
| Descarga | Bajada |
| Backup | Copia local → Subida (confirmada por el proveedor) |
| Restaurar | Bajada (archivos solo en la nube) → Copia |

También muestran bytes, archivos y velocidad. La barra de menú muestra `↑42%` / `↓42%` mientras hay una transferencia activa, y la UI avisa con un toast cuando termina. Se conservan las últimas 30 terminadas.

## Backups

Configuración en `~/Library/Application Support/iclear/cloud-backups.json`; registro en `~/Library/Logs/iclear/backups.log`.

- **Espejo**: una sola carpeta que se actualiza; solo copia lo nuevo o modificado (tamaño + fecha). No borra en el destino lo que se borró en el origen.
- **Versiones**: una copia completa con fecha (`2026-11-15 16.30.05`) en cada ejecución; se escribe como `… (en curso)` y se renombra al terminar. Conserva las últimas N (1–100); las anteriores se eliminan.
- **Frecuencia**: manual, cada hora, diaria o semanal. El programador revisa cada minuto mientras iclear está abierto; la primera ejecución programada es ~1 min después de crear el backup.
- **Exclusiones**: nombres de archivo o carpeta (`.git`, `node_modules`…). `.DS_Store` siempre se omite. Los enlaces simbólicos no se siguen.
- **Destino**: `<carpeta con escritura de la nube>/<carpeta, por defecto «iclear Backups»>/<nombre>`. Dos backups nunca comparten destino.
- Tras copiar, espera hasta 6 h a que el proveedor confirme la subida (barra **Subida**). Si se agota, el backup termina con «La nube sigue subiendo el resto».
- **Restaurar** copia una versión a una carpeta nueva en `~/Downloads` (`<nombre> (restaurado <fecha>)`); nunca reemplaza archivos.

### Reglas de seguridad

- El origen nunca se modifica.
- Se rechaza como origen `/`, la carpeta de usuario completa, cualquier carpeta que contenga las nubes o el destino, y cualquier carpeta dentro del destino.
- Las copias se escriben en un archivo temporal y se renombran al final: un archivo a medias nunca queda con su nombre final.
- El único borrado es de versiones antiguas del mismo backup: carpetas hijas directas de su destino cuyo nombre es exactamente una marca de tiempo de iclear. Cada una queda en `backups.log`.
- Eliminar un backup en iclear no borra sus copias en la nube.

## Diagnóstico

```bash
cargo run -p iclear-cli -- cloud
```
```bash
cargo run -p iclear-cli -- cloud stats icloud
```
```bash
cargo run -p iclear-cli -- cloud ls icloud
```
