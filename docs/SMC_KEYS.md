# Llaves SMC usadas por iclear

Verificadas el 2026-11-15 en un **Mac con Apple Silicon y macOS 27** con `iclear-cli smc read <KEY>` / `iclear-cli smc dump <PREFIJO>`. El SMC expone unas **3400 llaves**.

## Codificación (importante)

En Apple Silicon los valores vienen en **little-endian**, no en big-endian como en Intel:

| Tipo | Ejemplo real | Decodificación |
|---|---|---|
| `flt ` | `F0Tg = 00 f0 a5 45` | `f32` LE → 5310.0 |
| `ui32` | `CHA1 = 28 0d 00 00` | `u32` LE → 3368 (coincide con `PMUConfigured` de ioreg) |
| `ui16` | `BC1V = 7c 0f` | `u16` LE → 3964 mV |
| `ui8 ` | `FNum = 02` | 2 |
| `#KEY` | — | Excepción: sigue siendo big-endian |

Implementado en `backend/core/src/smc.rs` (`decode`/`encode`, constante `LITTLE_ENDIAN_INTS`).

## Ventiladores

| Llave | Tipo | Uso | Valor típico |
|---|---|---|---|
| `FNum` | ui8 | Cantidad de ventiladores | 2 |
| `F{i}Ac` | flt | RPM actuales | ~1200–5700 |
| `F{i}Mn` / `F{i}Mx` | flt | Mínimo / máximo | depende del modelo |
| `F{i}Tg` | flt | RPM objetivo (escritura en modo manual) | — |
| `F{i}Md` | ui8 | 1 = manual · 0 = auto · **3 = auto con ventilador detenido por el sistema** | 3 en reposo |
| `Ftst` | ui8 | Desbloqueo de modo prueba (M1–M3; existe en M4) | 0 |

**Probado con el helper (2026-11-15):** `F0Md=1` se acepta directamente, sin `Ftst`. Con `F0Tg=4000` el ventilador 0 pasó de 0 a ~4000 rpm en ~6 s; al volver a `F0Md=0` el sistema retomó el control (`F0Md` vuelve a 3 y el ventilador se detiene).

Secuencia del helper para modo manual: escribe `F{i}Md=1`; si falla y existe `Ftst`, escribe `Ftst=1` y reintenta hasta 5 s; luego `F{i}Tg=rpm` (acotado a `Mn..Mx`). Para volver: `F{i}Md=0` y, si no quedan ventiladores manuales, `Ftst=0`.

## Temperaturas

Se descubren al iniciar: todas las llaves `T???` de tipo `flt ` o `sp78` con valor entre 5 y 130 °C.

| Prefijo | Grupo |
|---|---|
| `Tp` | CPU P-core |
| `Te` | CPU E-core |
| `Tf` | CPU |
| `Tg` | GPU |
| `Tm` | Memoria |
| `TB` | Batería |
| `TH` | SSD |
| `TW` | Wi-Fi |
| `Ts` | Chasis / superficie |
| `TP`, `TV`, `TC` | Energía / reguladores |
| `Ta`, `TA` | Ambiente |

## Carga de batería — ⚠️ bloqueado en macOS 27

| Llave | Firmware | Uso | En este Mac |
|---|---|---|---|
| `CHTE` | macOS 15–26, M3/M4 | ui32, 1 = no cargar | **no existe** (result 132) |
| `CH0B` + `CH0C` | M1/M2 antiguos | 0x02 = no cargar | **no existen** |
| `CHIE` | Nuevos | 0x08 = desconectar adaptador (descarga forzada) | existe y **acepta escritura** (el helper lo detecta como `discharge=Chie`); efecto aún no probado |
| `CH0J` | Antiguos | 0x01 = desconectar adaptador | no legible |

Desde macOS 27 Apple restringió las llaves de control de carga del SMC a un entitlement privado, y además existe el **límite de carga nativo** (Ajustes del Sistema › Batería, 80–100 %, desde macOS 26.4). Por eso el helper **prueba cada llave al iniciar** (escribe de vuelta su valor actual) y reporta `supports_charge_limit` / `supports_force_discharge`. Si no hay soporte, la UI ofrece abrir los ajustes nativos de batería.

Referencias: [batt (charlie0129)](https://github.com/charlie0129/batt), [OpenDente](https://github.com/killerk3emstar/OpenDente), [ChargeWatching](https://github.com/TY-teo/ChargeWatching), [Apple: Límite de carga en Mac](https://support.apple.com/en-us/102338).
