# Seguridad del helper root (`iclear-helper`)

El helper es la única parte de iclear que corre como **root**. Está diseñado para que, aunque otra app de tu usuario le hable, no pueda hacer nada fuera de una lista cerrada de acciones.

## Instalación

- Se instala desde **Ajustes › Helper › Instalar** (o `install_helper`). macOS pide tu contraseña de administrador mediante `osascript … with administrator privileges`.
- `backend/helper/scripts/install.sh`:
  - copia el binario a `/Library/PrivilegedHelperTools/com.iclear.helper` (`root:wheel`, `755`);
  - escribe `/Library/Application Support/iclear/helper-auth.json` con el **uid permitido** (el tuyo);
  - crea `/Library/LaunchDaemons/com.iclear.helper.plist` (`RunAtLoad`, `KeepAlive`) y lo carga con `launchctl bootstrap system`.
- Logs: `/Library/Logs/iclear-helper.log`.
- Desinstalar: **Ajustes › Helper › Desinstalar** (`uninstall.sh`). Antes de borrar, la app quita el límite de carga y la descarga forzada.

## Superficie de ataque

| Control | Detalle |
|---|---|
| Socket | `/var/run/iclear-helper.sock`, permisos `0600`, dueño = uid permitido. |
| Autenticación | `getpeereid()` en cada conexión: solo el uid de `helper-auth.json` o root. |
| Protocolo | JSON por línea, máximo 4 KB por línea y 64 solicitudes por conexión, timeout de lectura 30 s. |
| Comandos | `enum HelperRequest` cerrado (`backend/protocol`). No hay ejecución de texto libre ni rutas arbitrarias. |
| Ventiladores | Índice validado contra `FNum`; RPM acotadas a `F{i}Mn..F{i}Mx`. |
| Límite de carga | Solo 20–100 %. Histéresis de 3 % para no alternar carga constantemente. |
| Optimización | Comandos fijos con ruta absoluta: `/usr/sbin/purge`, `dscacheutil`, `killall -HUP mDNSResponder`, `tmutil thinlocalsnapshots`, `mdutil -E /`. |
| Cachés del sistema | Solo hijos directos de `/Library/Caches`, sin seguir symlinks. |

## Valores seguros por defecto

- Al iniciar (incluido tras reiniciar el Mac) **todos los ventiladores vuelven a automático**.
- Si algún ventilador está en manual y la CPU llega a **95 °C**, el helper vuelve todo a automático.
- La descarga forzada se apaga sola al llegar al límite configurado (o al 20 % si no hay límite).
- El límite de carga y la descarga forzada se guardan en `helper-state.json` y se reaplican al iniciar.

## Limitaciones conocidas

- Sin firma de Apple el helper no usa `SMAppService`; se instala con el script de arriba.
- En macOS 27 el SMC rechaza escribir las llaves de carga (ver [SMC_KEYS.md](SMC_KEYS.md)); el helper lo detecta y lo informa en `HelperState`.
