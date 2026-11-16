# Desarrollo

## Requisitos

- macOS en Apple Silicon (probado con macOS 27).
- Rust estable vía rustup + target WASM:
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y -t wasm32-unknown-unknown
  ```
- Herramientas:
  ```bash
  cargo install --locked trunk tauri-cli
  ```

## Correr la app en desarrollo

```bash
cargo build -p iclear-helper
```
```bash
cd backend/app && cargo tauri dev
```

`cargo tauri dev` levanta `trunk serve --port 1420` en `frontend/` y abre la ventana. El binario `iclear-helper` debe estar compilado en `target/debug/` para que el botón **Instalar helper** lo encuentre (o define `ICLEAR_HELPER_PATH`).

## Solo la UI (con datos simulados)

```bash
cd frontend && trunk serve --port 1420
```

Abre http://localhost:1420 en el navegador: sin `window.__TAURI__`, `frontend/src/api.rs` usa datos de prueba (`frontend/src/mock.rs`).

## Instalador (.pkg)

```bash
scripts/build-pkg.sh
```

Genera `dist/iclear-<versión>.pkg` (está en `.gitignore`). El script:
1. compila `iclear-helper` y la app en release (`cargo tauri build --bundles app`);
2. copia el helper a `iclear.app/Contents/MacOS/`, donde `helper_client::bundled_helper_path()` lo busca;
3. firma la app (ad-hoc por defecto);
4. arma el `.pkg` con `pkgbuild` y `productbuild`: instala en `/Applications`, solo para arm64, con macOS 13 o superior.

**Firma:** sin una Developer ID de Apple, la firma es ad-hoc y el `.pkg` no está firmado. Al abrirlo, macOS avisa que es de un desarrollador no identificado; se abre con clic derecho › Abrir, o en Ajustes › Privacidad y seguridad › "Abrir igualmente". Para distribuirlo firmado y notarizado:

```bash
APP_SIGN_ID="Developer ID Application: …" PKG_SIGN_ID="Developer ID Installer: …" scripts/build-pkg.sh
```

Después hay que notarizarlo con `xcrun notarytool submit dist/iclear-*.pkg --wait` y `xcrun stapler staple`.

## Solo el .app

```bash
cd backend/app && cargo tauri build --bundles app
```

## Tests

```bash
cargo test
```

Corre los tests del backend (`default-members` excluye el frontend, que se compila con trunk).

## Diagnóstico por terminal

```bash
cargo run -p iclear-cli -- <comando>
```

| Comando | Qué hace |
|---|---|
| `battery` | Batería en JSON |
| `metrics` | Un snapshot completo |
| `fans` / `temps` | Ventiladores / sensores |
| `smc dump [PREFIJO]` | Lista llaves SMC con tipo y valor |
| `smc read KEY` | Una llave |
| `scan [RUTA]` | Uso por carpeta |
| `large [RUTA] [MB]` | Archivos grandes |
| `categories` | Categorías de limpieza (no borra nada) |
| `memory [--json]` | Diagnóstico de memoria: desglose, apps y recomendaciones |
| `apps [--json]` | Apps instaladas con veredicto de uso y restos huérfanos |
| `apps preview RUTA.app` / `apps preview BUNDLE_ID` | Qué borraría desinstalar esa app o esos restos (**no borra nada**) |
| `agents` | LaunchAgents / Daemons |
| `helper` | Estado del helper |
| `helper send '<json>'` | Envía una solicitud al helper, p. ej. `{"cmd":"set_fan_rpm","fan":0,"rpm":3000}` o `{"cmd":"set_fan_auto","fan":0}` |

## Permisos de macOS

- **Acceso total al disco** (Ajustes › Privacidad y seguridad): necesario para medir/limpiar `~/Library/Mail`, Papelera de otros volúmenes, contenedores, etc. Sin él, esos elementos aparecen con 0 B o se omiten.
- **Helper root**: ver [HELPER_SECURITY.md](HELPER_SECURITY.md).
