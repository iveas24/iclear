use leptos::prelude::*;

use super::root_disk;
use crate::components::*;
use crate::fmt;
use crate::state::{use_app, Page};

#[component]
fn RingTile(
    #[prop(into)] label: String,
    #[prop(into)] icon: String,
    #[prop(into)] value: Signal<f64>,
    #[prop(into)] center: Signal<String>,
    #[prop(into)] sub: Signal<String>,
    #[prop(into)] tone: Signal<String>,
    #[prop(optional)] page: Option<Page>,
) -> impl IntoView {
    let app = use_app();
    view! {
        <section
            class="card ring-tile"
            class:clickable=page.is_some()
            on:click=move |_| {
                if let Some(p) = page {
                    app.page.set(p)
                }
            }
        >
            <div class="ring-tile-head">
                <span class="stat-label">
                    <Icon name=icon size=14 />
                    <span>{label}</span>
                </span>
            </div>
            <Ring value=value size=118 stroke=7.0 tone=tone>
                <span class="ring-value-text num">{move || center.get()}</span>
            </Ring>
            <p class="ring-tile-sub">{move || sub.get()}</p>
        </section>
    }
}

#[component]
pub fn PanelPage() -> impl IntoView {
    let app = use_app();
    let m = app.metrics;

    let cpu = Signal::derive(move || m.with(|m| m.as_ref().map(|m| m.system.cpu_total as f64).unwrap_or(0.0)));
    let mem_pct = Signal::derive(move || {
        m.with(|m| {
            m.as_ref()
                .filter(|m| m.system.mem_total > 0)
                .map(|m| m.system.mem_used as f64 / m.system.mem_total as f64 * 100.0)
                .unwrap_or(0.0)
        })
    });
    let disk = Memo::new(move |_| m.with(|m| m.as_ref().and_then(|m| root_disk(&m.system).cloned())));
    let disk_pct = Signal::derive(move || {
        disk.with(|d| {
            d.as_ref()
                .filter(|d| d.total_bytes > 0)
                .map(|d| (d.total_bytes - d.available_bytes) as f64 / d.total_bytes as f64 * 100.0)
                .unwrap_or(0.0)
        })
    });
    let battery = Memo::new(move |_| m.with(|m| m.as_ref().and_then(|m| m.battery.clone())));
    let bat_pct = Signal::derive(move || battery.with(|b| b.as_ref().map(|b| b.percent as f64).unwrap_or(0.0)));

    let cores = move || {
        m.with(|m| m.as_ref().map(|m| m.system.cpu_per_core.clone()).unwrap_or_default())
    };

    view! {
        <div class="grid grid-4">
            <RingTile
                label="CPU"
                icon="cpu"
                value=cpu
                center=Signal::derive(move || fmt::pct(cpu.get() as f32))
                sub=Signal::derive(move || {
                    m.with(|m| {
                        m.as_ref()
                            .map(|m| {
                                format!(
                                    "Carga {} · {}",
                                    fmt::dec(m.system.load_avg[0], 2),
                                    fmt::plural(m.system.process_count, "proceso", "procesos"),
                                )
                            })
                            .unwrap_or_default()
                    })
                })
                tone=Signal::derive(move || load_tone(cpu.get()))
                page=Page::Procesos
            />
            <RingTile
                label="Memoria"
                icon="memory"
                value=mem_pct
                center=Signal::derive(move || fmt::pct(mem_pct.get() as f32))
                sub=Signal::derive(move || {
                    m.with(|m| {
                        m.as_ref()
                            .map(|m| format!("{} de {}", fmt::mem(m.system.mem_used), fmt::mem(m.system.mem_total)))
                            .unwrap_or_default()
                    })
                })
                tone=Signal::derive(move || load_tone(mem_pct.get()))
                page=Page::Memoria
            />
            <RingTile
                label="Disco"
                icon="disk"
                value=disk_pct
                center=Signal::derive(move || fmt::pct(disk_pct.get() as f32))
                sub=Signal::derive(move || {
                    disk.with(|d| {
                        d.as_ref()
                            .map(|d| format!("{} libres de {}", fmt::bytes(d.available_bytes), fmt::bytes(d.total_bytes)))
                            .unwrap_or_default()
                    })
                })
                tone=Signal::derive(move || load_tone(disk_pct.get()))
                page=Page::Almacenamiento
            />
            <RingTile
                label="Batería"
                icon="battery"
                value=bat_pct
                center=Signal::derive(move || {
                    battery.with(|b| b.as_ref().map(|b| format!("{}%", b.percent)).unwrap_or_else(|| "—".into()))
                })
                sub=Signal::derive(move || {
                    battery.with(|b| match b {
                        None => "Sin batería".to_string(),
                        Some(b) if b.is_charging => format!("Cargando · {} W", fmt::dec(b.power_w.abs() as f64, 1)),
                        Some(b) if b.external_connected => "Conectado · sin cargar".to_string(),
                        Some(b) => format!("Descargando · {} W", fmt::dec(b.power_w.abs() as f64, 1)),
                    })
                })
                tone=Signal::derive(move || {
                    if bat_pct.get() <= 15.0 { "warn".to_string() } else { "accent".to_string() }
                })
                page=Page::Bateria
            />
        </div>

        <div class="grid grid-3">
            <section class="card span-2">
                <header class="card-head">
                    <span class="card-icon"><Icon name="cpu" size=16 /></span>
                    <div class="card-head-text">
                        <h3 class="card-title">"Uso de CPU"</h3>
                        <p class="card-sub">"Últimos 90 segundos"</p>
                    </div>
                    <span class="card-figure num">{move || fmt::pct(cpu.get() as f32)}</span>
                </header>
                <Sparkline data=app.cpu_hist min=0.0 max=100.0 height=96 />
                <div class="cores" aria-label="Uso por núcleo">
                    {move || {
                        cores()
                            .into_iter()
                            .map(|v| {
                                view! {
                                    <span class="core" title=fmt::pct(v)>
                                        <span class="core-fill" style=format!("height:{:.1}%", v.clamp(2.0, 100.0)) />
                                    </span>
                                }
                            })
                            .collect_view()
                    }}
                </div>
            </section>
            <section class="card">
                <header class="card-head">
                    <span class="card-icon"><Icon name="thermo" size=16 /></span>
                    <div class="card-head-text">
                        <h3 class="card-title">"Temperatura CPU"</h3>
                        <p class="card-sub">"Promedio de sensores"</p>
                    </div>
                    <span class="card-figure num">
                        {move || m.with(|m| fmt::temp_opt(m.as_ref().and_then(|m| m.temp_summary.cpu_avg)))}
                    </span>
                </header>
                <Sparkline data=app.temp_hist min=30.0 max=90.0 height=96 tone="warm" />
                <div class="mini-stats">
                    <div>
                        <span class="eyebrow">"Máx"</span>
                        <span class="num">{move || m.with(|m| fmt::temp_opt(m.as_ref().and_then(|m| m.temp_summary.cpu_max)))}</span>
                    </div>
                    <div>
                        <span class="eyebrow">"GPU"</span>
                        <span class="num">{move || m.with(|m| fmt::temp_opt(m.as_ref().and_then(|m| m.temp_summary.gpu_avg)))}</span>
                    </div>
                    <div>
                        <span class="eyebrow">"SSD"</span>
                        <span class="num">{move || m.with(|m| fmt::temp_opt(m.as_ref().and_then(|m| m.temp_summary.ssd)))}</span>
                    </div>
                </div>
            </section>
        </div>

        <div class="grid grid-3">
            <FansCard />
            <NetworkCard />
            <MachineCard />
        </div>
    }
}

#[component]
fn FansCard() -> impl IntoView {
    let app = use_app();
    let fans = move || app.metrics.with(|m| m.as_ref().map(|m| m.fans.clone()).unwrap_or_default());
    view! {
        <section class="card clickable" on:click=move |_| app.page.set(Page::Ventiladores)>
            <header class="card-head">
                <span class="card-icon"><Icon name="fan" size=16 /></span>
                <div class="card-head-text">
                    <h3 class="card-title">"Ventiladores"</h3>
                    <p class="card-sub">"Revoluciones por minuto"</p>
                </div>
            </header>
            <div class="stack">
                {move || {
                    let list = fans();
                    if list.is_empty() {
                        return view! { <p class="muted">"Este equipo no informa ventiladores."</p> }.into_any();
                    }
                    list.into_iter()
                        .map(|f| {
                            let span = (f.max_rpm - f.min_rpm).max(1.0);
                            let pct = ((f.actual_rpm - f.min_rpm) / span * 100.0).clamp(0.0, 100.0) as f64;
                            view! {
                                <div class="fan-row">
                                    <div class="fan-row-head">
                                        <span>{format!("Ventilador {}", f.index + 1)}</span>
                                        <span class="num">{fmt::rpm(f.actual_rpm)} <span class="unit">" rpm"</span></span>
                                    </div>
                                    <Meter value=pct />
                                </div>
                            }
                        })
                        .collect_view()
                        .into_any()
                }}
            </div>
        </section>
    }
}

#[component]
fn NetworkCard() -> impl IntoView {
    let app = use_app();
    let m = app.metrics;
    view! {
        <section class="card">
            <header class="card-head">
                <span class="card-icon"><Icon name="network" size=16 /></span>
                <div class="card-head-text">
                    <h3 class="card-title">"Red"</h3>
                    <p class="card-sub">"Tráfico de todas las interfaces"</p>
                </div>
            </header>
            <div class="net-grid">
                <div>
                    <span class="eyebrow">"↓ Recibido"</span>
                    <span class="net-value num">
                        {move || m.with(|m| m.as_ref().map(|m| fmt::rate(m.system.net_rx_per_sec)).unwrap_or_default())}
                    </span>
                    <Sparkline data=app.rx_hist min=0.0 height=40 />
                </div>
                <div>
                    <span class="eyebrow">"↑ Enviado"</span>
                    <span class="net-value num">
                        {move || m.with(|m| m.as_ref().map(|m| fmt::rate(m.system.net_tx_per_sec)).unwrap_or_default())}
                    </span>
                    <Sparkline data=app.tx_hist min=0.0 height=40 tone="muted" />
                </div>
            </div>
        </section>
    }
}

#[component]
fn MachineCard() -> impl IntoView {
    let app = use_app();
    let field = move |f: fn(&iclear_protocol::MachineInfo) -> String| {
        Signal::derive(move || app.machine.with(|m| m.as_ref().map(f).unwrap_or_else(|| "—".into())))
    };
    view! {
        <section class="card">
            <header class="card-head">
                <span class="card-icon"><Icon name="info" size=16 /></span>
                <div class="card-head-text">
                    <h3 class="card-title">"Equipo"</h3>
                    <p class="card-sub">{move || app.machine.with(|m| m.as_ref().map(|m| m.hostname.clone()).unwrap_or_default())}</p>
                </div>
            </header>
            <div class="info-list">
                <InfoRow label="Modelo" value=field(|m| m.model.clone()) />
                <InfoRow label="Chip" value=field(|m| m.chip.clone()) />
                <InfoRow label="Sistema" value=field(|m| m.os_version.clone()) />
                <InfoRow label="Núcleos" value=field(|m| m.cpu_cores.to_string()) />
                <InfoRow label="Memoria" value=field(|m| fmt::mem(m.mem_total)) />
                <InfoRow
                    label="Encendido hace"
                    value=Signal::derive(move || {
                        app.metrics.with(|m| m.as_ref().map(|m| fmt::uptime(m.system.uptime_secs)).unwrap_or_default())
                    })
                />
            </div>
        </section>
    }
}
