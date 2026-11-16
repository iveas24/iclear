# Cambios

## 0.1.0 — 2026-11-15

### Agregado
- App de escritorio Tauri 2 con UI Leptos: Panel, Batería, Ventiladores, Almacenamiento, Limpieza, Procesos, Optimizar y Ajustes.
- Métricas en vivo cada 1,5 s y título en la barra de menú (batería · temperatura de CPU).
- Cliente SMC propio (lectura de ventiladores, temperaturas y llaves de carga).
- Limpieza con borrado permanente: vista previa, token de un solo uso, guardas de rutas y registro en `~/Library/Logs/iclear/deletions.log`.
- Archivos grandes y duplicados en tu carpeta de usuario.
- Helper root (`com.iclear.helper`) para ventiladores, límite de carga, descarga forzada y acciones de sistema.
- `iclear-cli` para diagnóstico.
- Diseño oscuro en tonos plomo con acento verde, inspirado en metalforge.xyz.

- **Memoria**: diagnóstico de por qué el Mac usa tanta RAM. Mide `phys_footprint`, agrupa cada proceso bajo la app responsable (API de libSystem que usa el Monitor de Actividad), registra qué app está al frente, detecta fugas por crecimiento y genera recomendaciones con cuánto liberan.
- **Apps sin uso**: veredicto por app (Spotlight, registro propio de foco y datos modificados), restos en `~/Library`, desinstalación completa y limpieza de restos huérfanos. Usa una guarda de borrado propia y más estricta.
- `iclear-cli memory`, `iclear-cli apps` y `iclear-cli apps preview` (simulación, no borra).

- Video promocional para historias de Instagram (`promo/`): 26 s en 9:16, con animación en canvas, música original sintetizada y voz en off en español. Ver [promo/README.md](promo/README.md).

- Instalador `.pkg` (`scripts/build-pkg.sh`): la app trae el helper dentro, firma ad-hoc e instala en /Applications.
- Voz del video promocional: estilo tráiler masculino, grave y envolvente.

- **Nube**: gestión de todas las nubes sincronizadas (iCloud Drive y cada cuenta de `~/Library/CloudStorage`). Tamaño, porcentaje en este Mac y porcentaje subido por nube y por archivo/carpeta, pendientes de subir y conflictos (vía `SF_DATALESS` y `NSURLUbiquitousItem*`). Escaneo en segundo plano con caché: más de un millón de archivos en ~20 s.
- Explorador de nubes: descargar a este Mac y liberar espacio local (solo archivos ya subidos).
- Transferencias con progreso de bajada, copia y subida, velocidad y cancelación; porcentaje en la barra de menú y aviso al terminar.
- **Backups en la nube**: espejo incremental o versiones con fecha (conserva las últimas N), manual o cada hora/día/semana, exclusiones, espera la confirmación de subida del proveedor y restauración a una carpeta nueva. Registro en `~/Library/Logs/iclear/backups.log`.
- `iclear-cli cloud`, `cloud stats ID`, `cloud ls ID [RUTA]` y `cloud backups`.

### Corregido
- La instalación del helper fallaba si el proyecto estaba en `~/Desktop` (TCC); el binario ahora se copia a una carpeta temporal antes de instalarlo.
- Los ventiladores detenidos por el sistema (`F{i}Md = 3`) aparecían como "Manual".

### Privacidad
- Los datos simulados, los tests y los docs ya no contienen datos del Mac de desarrollo; usan valores ficticios.

### Conocido
- En macOS 27 el límite de carga por SMC no está disponible; la app enlaza al límite nativo de macOS.
