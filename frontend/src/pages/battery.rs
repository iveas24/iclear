use iclear_protocol::{BatteryInfo, HelperState};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::*;
use crate::state::{use_app, AppState, ToastKind};
use crate::{api, fmt};

const UNSUPPORTED_MSG: &str = "macOS 27 bloquea el control de carga por apps de terceros. Usa el límite de carga nativo (80–100 %) en Ajustes del Sistema › Batería.";

fn status_text(b: &BatteryInfo, inhibited: bool) -> (&'static str, String) {
    if b.is_charging {
        ("accent", "Cargando".into())
    } else if b.external_connected && b.fully_charged {
        ("accent", "Carga completa".into())
    } else if b.external_connected && inhibited {
        ("warn", "En pausa por límite de carga".into())
    } else if b.external_connected {
        ("neutral", "Conectado, sin cargar".into())
    } else {
        ("neutral", "Usando batería".into())
    }
}

fn open_battery_settings(app: AppState) {
    spawn_local(async move {
        if let Err(e) = api::open_battery_settings().await {
            app.toast(ToastKind::Error, "No se pudo abrir Ajustes del Sistema", Some(e));
        }
    });
}

#[component]
pub fn BatteryPage() -> impl IntoView {
    let app = use_app();
    let battery = Memo::new(move |_| app.metrics.with(|m| m.as_ref().and_then(|m| m.battery.clone())));
    let has_metrics = move || app.metrics.with(|m| m.is_some());

    view! {
        <Show
            when=move || battery.with(|b| b.is_some()) || !has_metrics()
            fallback=|| {
                view! {
                    <EmptyState
                        icon="battery"
                        title="Este Mac no tiene batería"
                        text="Las opciones de carga solo están disponibles en portátiles."
                    />
                }
            }
        >
            <BatteryDetails battery=battery />
            <ChargeControls />
        </Show>
    }
}

#[component]
fn BatteryDetails(battery: Memo<Option<BatteryInfo>>) -> impl IntoView {
    let app = use_app();
    let get = move |f: fn(&BatteryInfo) -> String| {
        Signal::derive(move || battery.with(|b| b.as_ref().map(f).unwrap_or_else(|| "—".into())))
    };
    let pct = Signal::derive(move || battery.with(|b| b.as_ref().map(|b| b.percent as f64).unwrap_or(0.0)));
    let inhibited = move || app.helper_state().is_some_and(|s| s.charging_inhibited);
    let status = move || {
        battery.with(|b| b.as_ref().map(|b| status_text(b, inhibited())))
            .unwrap_or(("neutral", "—".into()))
    };

    let cap_row = move |label: &'static str, f: fn(&BatteryInfo) -> u32| {
        let value = move || battery.with(|b| b.as_ref().map(f).unwrap_or(0));
        let design = move || battery.with(|b| b.as_ref().map(|b| b.design_capacity_mah.max(1)).unwrap_or(1));
        view! {
            <div class="cap-row">
                <div class="cap-row-head">
                    <span>{label}</span>
                    <span class="num">{move || fmt::thousands(value() as u64)} <span class="unit">" mAh"</span></span>
                </div>
                <Meter value=Signal::derive(move || value() as f64 / design() as f64 * 100.0) />
            </div>
        }
    };

    view! {
        <div class="grid battery-hero">
            <section class="card hero-card">
                <Ring value=pct size=196 stroke=10.0 tone=Signal::derive(move || {
                    if pct.get() <= 15.0 { "warn".to_string() } else { "accent".to_string() }
                })>
                    <span class="ring-big num">{move || format!("{}", pct.get() as u32)}<span class="ring-unit">"%"</span></span>
                    <span class="ring-caption">"carga"</span>
                </Ring>
                <div class="hero-info">
                    <Badge tone=Signal::derive(move || status().0.to_string()) dot=true>
                        {move || status().1}
                    </Badge>
                    <p class="hero-line">
                        {move || {
                            battery.with(|b| {
                                let b = b.as_ref()?;
                                let t = b.time_remaining_min?;
                                Some(if b.is_charging {
                                    format!("Carga completa en {}", fmt::minutes(t))
                                } else if !b.external_connected {
                                    format!("{} restantes", fmt::minutes(t))
                                } else {
                                    return None;
                                })
                            })
                            .unwrap_or_else(|| "Calculando tiempo…".into())
                        }}
                    </p>
                    <div class="info-list">
                        <InfoRow
                            label="Adaptador"
                            value=get(|b| match (b.external_connected, b.adapter_watts) {
                                (false, _) => "Desconectado".into(),
                                (true, Some(w)) => format!("{w} W"),
                                (true, None) => "Conectado".into(),
                            })
                        />
                        <InfoRow
                            label="Modelo"
                            value=get(|b| b.adapter_name.clone().unwrap_or_else(|| "—".into()))
                        />
                        <InfoRow
                            label="Corriente"
                            value=get(|b| format!("{} mA", b.amperage_ma))
                        />
                    </div>
                </div>
            </section>
            <div class="grid grid-2 tight">
                <StatTile
                    label="Salud"
                    icon="shield"
                    value=get(|b| fmt::dec(b.health_percent as f64, 0))
                    unit="%"
                    sub=Signal::derive(move || {
                        battery.with(|b| match b.as_ref().map(|b| b.health_percent) {
                            Some(h) if h >= 80.0 => "Normal".into(),
                            Some(_) => "Se recomienda revisión".into(),
                            None => String::new(),
                        })
                    })
                    tone=Signal::derive(move || {
                        battery.with(|b| match b.as_ref().map(|b| b.health_percent) {
                            Some(h) if h < 80.0 => "warn".to_string(),
                            _ => String::new(),
                        })
                    })
                />
                <StatTile label="Ciclos" icon="refresh" value=get(|b| fmt::thousands(b.cycle_count as u64)) sub="Diseñada para 1.000" />
                <StatTile label="Temperatura" icon="thermo" value=get(|b| fmt::dec(b.temperature_c as f64, 1)) unit="°C" />
                <StatTile label="Voltaje" icon="bolt" value=get(|b| fmt::dec(b.voltage_v as f64, 2)) unit="V" />
                <StatTile
                    label="Potencia"
                    icon="plug"
                    value=get(|b| fmt::dec(b.power_w.abs() as f64, 1))
                    unit="W"
                    sub=get(|b| if b.power_w >= 0.0 { "Entrando".into() } else { "Saliendo".into() })
                />
                <StatTile
                    label="Tiempo restante"
                    icon="clock"
                    value=get(|b| b.time_remaining_min.map(fmt::minutes).unwrap_or_else(|| "—".into()))
                    sub=get(|b| if b.is_charging { "Hasta carga completa".into() } else { "Hasta agotarse".into() })
                />
            </div>
        </div>

        <section class="card">
            <header class="card-head">
                <span class="card-icon"><Icon name="battery" size=16 /></span>
                <div class="card-head-text">
                    <h3 class="card-title">"Capacidad"</h3>
                    <p class="card-sub">"Relativa a la capacidad de diseño"</p>
                </div>
            </header>
            <div class="grid grid-3 tight">
                {cap_row("Actual", |b| b.current_capacity_mah)}
                {cap_row("Máxima", |b| b.max_capacity_mah)}
                {cap_row("Diseño", |b| b.design_capacity_mah)}
            </div>
        </section>
    }
}

#[component]
fn ChargeControls() -> impl IntoView {
    let app = use_app();
    let hs = Memo::new(move |_| app.helper_state());
    let limit = Memo::new(move |_| hs.with(|s| s.as_ref().and_then(|s| s.charge_limit)));
    let limit_on = Signal::derive(move || limit.get().is_some());
    let limit_val = RwSignal::new(80.0_f64);
    Effect::new(move |_| {
        if let Some(p) = limit.get() {
            limit_val.set(p as f64);
        }
    });
    let supports_limit = Signal::derive(move || hs.with(|s| s.as_ref().is_some_and(|s| s.supports_charge_limit)));
    let supports_discharge =
        Signal::derive(move || hs.with(|s| s.as_ref().is_some_and(|s| s.supports_force_discharge)));
    let discharge_on = Signal::derive(move || hs.with(|s| s.as_ref().is_some_and(|s| s.force_discharge)));
    let pending = RwSignal::new(false);

    let apply = move |fut: std::pin::Pin<Box<dyn std::future::Future<Output = Result<HelperState, String>>>>,
                      ok_msg: String| {
        pending.set(true);
        spawn_local(async move {
            match fut.await {
                Ok(s) => {
                    app.apply_helper_state(s);
                    app.toast(ToastKind::Success, ok_msg, None);
                }
                Err(e) => app.toast(ToastKind::Error, "No se pudo aplicar el cambio", Some(e)),
            }
            pending.set(false);
        });
    };

    let set_limit = move |p: Option<u8>| {
        let msg = match p {
            Some(p) => format!("Límite de carga: {p} %"),
            None => "Límite de carga desactivado".into(),
        };
        apply(Box::pin(api::set_charge_limit(p)), msg);
    };
    let set_discharge = move |on: bool| {
        let msg = if on { "Descarga forzada activada" } else { "Descarga forzada desactivada" };
        apply(Box::pin(api::set_force_discharge(on)), msg.into());
    };

    view! {
        <section class="card">
            <header class="card-head">
                <span class="card-icon"><Icon name="sliders" size=16 /></span>
                <div class="card-head-text">
                    <h3 class="card-title">"Control de carga"</h3>
                    <p class="card-sub">"Mantener la carga bajo 100 % reduce el desgaste de la batería"</p>
                </div>
            </header>
            <HelperRequired feature="El control de carga">
                <Show when=move || !supports_limit.get() || !supports_discharge.get()>
                    <UnsupportedNote
                        message=UNSUPPORTED_MSG
                        action_label="Abrir ajustes de batería"
                        on_action=Callback::new(move |_| open_battery_settings(app))
                    />
                </Show>
                <div class="control-list">
                    <div class="control-row" class:disabled=move || !supports_limit.get()>
                        <div class="control-text">
                            <p class="control-title">"Límite de carga"</p>
                            <p class="control-sub">
                                "Detiene la carga al llegar al porcentaje elegido, aunque el adaptador siga conectado."
                            </p>
                        </div>
                        <Toggle
                            checked=limit_on
                            label="Límite de carga"
                            disabled=Signal::derive(move || !supports_limit.get() || pending.get())
                            on_toggle=move |on: bool| {
                                set_limit(on.then(|| limit_val.get_untracked() as u8))
                            }
                        />
                    </div>
                    <div class="control-slider" class:disabled=move || !supports_limit.get() || !limit_on.get()>
                        <Slider
                            value=limit_val
                            min=20.0
                            max=100.0
                            step=5.0
                            label="Porcentaje máximo de carga"
                            disabled=Signal::derive(move || !supports_limit.get() || !limit_on.get())
                            on_input=move |v: f64| limit_val.set(v)
                            on_change=move |v: f64| set_limit(Some(v as u8))
                        />
                        <span class="slider-value num">{move || format!("{} %", limit_val.get() as u32)}</span>
                    </div>
                    <div class="control-row" class:disabled=move || !supports_discharge.get()>
                        <div class="control-text">
                            <p class="control-title">"Descarga forzada"</p>
                            <p class="control-sub">
                                "Usa la batería aunque el adaptador esté conectado. Útil para bajar hasta el límite."
                            </p>
                        </div>
                        <Toggle
                            checked=discharge_on
                            label="Descarga forzada"
                            disabled=Signal::derive(move || !supports_discharge.get() || pending.get())
                            on_toggle=move |on: bool| set_discharge(on)
                        />
                    </div>
                </div>
            </HelperRequired>
        </section>
    }
}
