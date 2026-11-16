use iclear_protocol::{FanInfo, FanMode, TempSensor};
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::PageIntro;
use crate::components::*;
use crate::state::{use_app, ToastKind};
use crate::{api, fmt};

const GROUP_ORDER: &[&str] = &[
    "CPU", "GPU", "Memoria", "Batería", "SSD", "Wi-Fi", "Chasis", "Energía", "Ambiente",
];

#[component]
pub fn FansPage() -> impl IntoView {
    let app = use_app();
    let fan_ids = Memo::new(move |_| {
        app.metrics.with(|m| {
            m.as_ref()
                .map(|m| m.fans.iter().map(|f| f.index).collect::<Vec<u8>>())
                .unwrap_or_default()
        })
    });
    let fan_control_blocked = move || {
        app.helper_ready() && !app.helper_state().is_some_and(|s| s.supports_fan_control)
    };

    view! {
        <PageIntro text="Velocidad en vivo de cada ventilador. El modo manual fija las revoluciones hasta que vuelvas a automático o reinicies el equipo.">
            <Badge tone="warn".to_string() dot=true>"Experimental"</Badge>
        </PageIntro>
        <Show when=move || !app.helper_ready()>
            <HelperCard feature="El control manual de ventiladores" />
        </Show>
        <Show when=fan_control_blocked>
            <UnsupportedNote message="macOS 27 bloquea el control de ventiladores por apps de terceros. Los ventiladores seguirán en modo automático, gestionados por el sistema." />
        </Show>
        <Show
            when=move || !fan_ids.with(|f| f.is_empty())
            fallback=|| {
                view! {
                    <EmptyState
                        icon="fan"
                        title="Sin ventiladores"
                        text="Este equipo no informa ventiladores (por ejemplo, un MacBook Air)."
                    />
                }
            }
        >
            <div class="grid grid-2">
                <For each=move || fan_ids.get() key=|i| *i let:index>
                    <FanCard index=index />
                </For>
            </div>
        </Show>
        <TemperatureSection />
    }
}

#[component]
fn FanCard(index: u8) -> impl IntoView {
    let app = use_app();
    let fan = Memo::new(move |_| {
        app.metrics
            .with(|m| m.as_ref().and_then(|m| m.fans.iter().find(|f| f.index == index).cloned()))
    });
    let initial: Option<FanInfo> = fan.get_untracked();
    let mode = RwSignal::new(match initial.as_ref().map(|f| f.mode) {
        Some(FanMode::Manual) => "manual".to_string(),
        _ => "auto".to_string(),
    });
    let target = RwSignal::new(
        initial
            .as_ref()
            .map(|f| f.target_rpm.unwrap_or(f.actual_rpm).round() as f64)
            .unwrap_or(0.0),
    );
    let pending = RwSignal::new(false);

    let min = initial.as_ref().map(|f| f.min_rpm as f64).unwrap_or(0.0);
    let max = initial.as_ref().map(|f| f.max_rpm as f64).unwrap_or(1.0).max(min + 1.0);

    let pct = Signal::derive(move || {
        fan.with(|f| f.as_ref().map(|f| (f.actual_rpm as f64 - min) / (max - min) * 100.0).unwrap_or(0.0))
    });
    let controls_disabled = Signal::derive(move || {
        !app.helper_ready() || !app.helper_state().is_some_and(|s| s.supports_fan_control) || pending.get()
    });

    // In auto mode the slider mirrors the live speed; in manual it holds the target.
    let shown = Signal::derive(move || {
        if mode.get() == "manual" {
            target.get()
        } else {
            fan.with(|f| f.as_ref().map(|f| f.actual_rpm as f64).unwrap_or(min))
        }
    });

    let send_rpm = move |rpm: f64| {
        pending.set(true);
        spawn_local(async move {
            match api::set_fan_rpm(index, rpm as f32).await {
                Ok(s) => app.apply_helper_state(s),
                Err(e) => app.toast(ToastKind::Error, "No se pudo fijar la velocidad", Some(e)),
            }
            pending.set(false);
        });
    };
    let set_mode = move |m: String| {
        if m == mode.get_untracked() {
            return;
        }
        mode.set(m.clone());
        if m == "manual" {
            let cur = fan.with_untracked(|f| f.as_ref().map(|f| f.actual_rpm as f64).unwrap_or(min));
            let rpm = (cur / 50.0).round() * 50.0;
            target.set(rpm.clamp(min, max));
            send_rpm(target.get_untracked());
        } else {
            pending.set(true);
            spawn_local(async move {
                match api::set_fan_auto(index).await {
                    Ok(s) => app.apply_helper_state(s),
                    Err(e) => {
                        mode.set("manual".into());
                        app.toast(ToastKind::Error, "No se pudo volver a automático", Some(e))
                    }
                }
                pending.set(false);
            });
        }
    };

    view! {
        <section class="card fan-card">
            <header class="card-head">
                <span class="card-icon"><Icon name="fan" size=16 class="spin-slow" /></span>
                <div class="card-head-text">
                    <h3 class="card-title">{format!("Ventilador {}", index + 1)}</h3>
                    <p class="card-sub">
                        {format!("Rango {} – {} rpm", fmt::rpm(min as f32), fmt::rpm(max as f32))}
                    </p>
                </div>
                <Segmented
                    options=vec![("auto", "Auto"), ("manual", "Manual")]
                    selected=mode
                    disabled=controls_disabled
                    on_select=set_mode
                />
            </header>
            <div class="fan-body">
                <Ring value=pct size=150 stroke=8.0>
                    <span class="ring-value-text num">
                        {move || fan.with(|f| f.as_ref().map(|f| fmt::rpm(f.actual_rpm)).unwrap_or_default())}
                    </span>
                    <span class="ring-caption">"rpm"</span>
                </Ring>
                <div class="fan-side">
                    <div class="info-list">
                        <InfoRow
                            label="Modo"
                            value=Signal::derive(move || {
                                if mode.get() == "manual" { "Manual".to_string() } else { "Automático".to_string() }
                            })
                        />
                        <InfoRow
                            label="Objetivo"
                            value=Signal::derive(move || {
                                fan.with(|f| {
                                    f.as_ref()
                                        .and_then(|f| f.target_rpm)
                                        .map(|t| format!("{} rpm", fmt::rpm(t)))
                                        .unwrap_or_else(|| "—".into())
                                })
                            })
                        />
                        <InfoRow label="Mínimo" value=format!("{} rpm", fmt::rpm(min as f32)) />
                        <InfoRow label="Máximo" value=format!("{} rpm", fmt::rpm(max as f32)) />
                    </div>
                </div>
            </div>
            <div class="control-slider" class:disabled=move || mode.get() != "manual" || controls_disabled.get()>
                <Slider
                    value=shown
                    min=min
                    max=max
                    step=50.0
                    label="Revoluciones objetivo"
                    disabled=Signal::derive(move || mode.get() != "manual" || controls_disabled.get())
                    on_input=move |v: f64| target.set(v)
                    on_change=move |v: f64| send_rpm(v)
                />
                <span class="slider-value num">{move || format!("{} rpm", fmt::rpm(shown.get() as f32))}</span>
            </div>
        </section>
    }
}

fn split_label(t: &TempSensor) -> (String, String) {
    match t.label.split_once(" · ") {
        Some((name, key)) => (name.to_string(), key.to_string()),
        None => (t.label.clone(), t.key.clone()),
    }
}

#[component]
fn TemperatureSection() -> impl IntoView {
    let app = use_app();
    let s = move |f: fn(&iclear_protocol::TempSummary) -> Option<f32>| {
        Signal::derive(move || {
            app.metrics.with(|m| {
                m.as_ref()
                    .and_then(|m| f(&m.temp_summary))
                    .map(|c| fmt::dec(c as f64, 1))
                    .unwrap_or_else(|| "—".into())
            })
        })
    };
    let groups = Memo::new(move |_| {
        app.metrics.with(|m| {
            let mut groups: Vec<(String, Vec<TempSensor>)> = Vec::new();
            for t in m.as_ref().map(|m| m.temps.as_slice()).unwrap_or_default() {
                match groups.iter_mut().find(|g| g.0 == t.group) {
                    Some(g) => g.1.push(t.clone()),
                    None => groups.push((t.group.clone(), vec![t.clone()])),
                }
            }
            groups.sort_by_key(|g| GROUP_ORDER.iter().position(|o| *o == g.0).unwrap_or(usize::MAX));
            groups
        })
    });

    view! {
        <h2 class="section-title">"Temperaturas"</h2>
        <div class="grid grid-5 tight">
            <StatTile label="CPU promedio" icon="cpu" value=s(|t| t.cpu_avg) unit="°C" />
            <StatTile label="CPU máxima" icon="thermo" value=s(|t| t.cpu_max) unit="°C" />
            <StatTile label="GPU" icon="layers" value=s(|t| t.gpu_avg) unit="°C" />
            <StatTile label="Batería" icon="battery" value=s(|t| t.battery) unit="°C" />
            <StatTile label="SSD" icon="disk" value=s(|t| t.ssd) unit="°C" />
        </div>
        <Show
            when=move || !groups.with(|g| g.is_empty())
            fallback=|| view! { <EmptyState icon="thermo" title="Sin sensores de temperatura" /> }
        >
            <div class="temp-groups">
                {move || {
                    groups
                        .get()
                        .into_iter()
                        .map(|(name, sensors)| {
                            let count = sensors.len();
                            view! {
                                <section class="card temp-card">
                                    <header class="temp-head">
                                        <h3 class="card-title">{name}</h3>
                                        <span class="muted num">{count}</span>
                                    </header>
                                    <ul class="temp-list">
                                        {sensors
                                            .into_iter()
                                            .map(|t| {
                                                let (label, key) = split_label(&t);
                                                let pct = ((t.celsius as f64 - 20.0) / 80.0 * 100.0).clamp(0.0, 100.0);
                                                let tone = if t.celsius >= 90.0 {
                                                    "danger"
                                                } else if t.celsius >= 75.0 {
                                                    "warn"
                                                } else {
                                                    "accent"
                                                };
                                                view! {
                                                    <li class="temp-row">
                                                        <span class="temp-label truncate">
                                                            {label}
                                                            <span class="temp-key">{key}</span>
                                                        </span>
                                                        <Meter value=pct tone=tone.to_string() />
                                                        <span class="temp-value num">{fmt::temp(t.celsius)}</span>
                                                    </li>
                                                }
                                            })
                                            .collect_view()}
                                    </ul>
                                </section>
                            }
                        })
                        .collect_view()
                }}
            </div>
        </Show>
    }
}
