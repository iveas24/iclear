//! Memoria: explains *why* the Mac uses so much memory and what to do.

use std::time::Duration;

use iclear_protocol::{
    AppKind, AppMemory, Insight, InsightAction, MemoryBreakdown, MemoryPressure, OptimizeAction,
    Severity,
};
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::PageIntro;
use crate::components::*;
use crate::state::{use_app, AppState, Page, ToastKind};
use crate::{api, fmt};

const TOP_APPS: usize = 25;
const POLL: Duration = Duration::from_secs(5);
/// Growth above this (bytes/hour) is flagged as a possible leak.
const LEAK_PER_HOUR: i64 = 200 * 1_048_576;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Col {
    Name,
    Mem,
    Cpu,
}

/// App to quit, pending confirmation.
#[derive(Clone, PartialEq)]
struct QuitTarget {
    name: String,
    pid: u32,
    footprint: Option<u64>,
    gui: bool,
}

/// Apps iclear must never offer to quit (closing Claude would end the
/// user's own Claude session).
const NEVER_QUIT: &[&str] = &["com.anthropic.claudefordesktop"];

fn quit_protected(a: &AppMemory) -> bool {
    a.bundle_id.as_deref().is_some_and(|b| NEVER_QUIT.contains(&b))
}

pub fn kind_label(k: AppKind) -> &'static str {
    match k {
        AppKind::Browser => "Navegador",
        AppKind::Electron => "Electron",
        AppKind::Developer => "Desarrollo",
        AppKind::Virtualization => "Virtualización",
        AppKind::Background => "Segundo plano",
        AppKind::System => "Sistema",
        AppKind::App => "App",
    }
}

fn pressure_badge(p: MemoryPressure) -> (&'static str, &'static str) {
    match p {
        MemoryPressure::Normal => ("accent", "Normal"),
        MemoryPressure::Warning => ("warn", "Advertencia"),
        MemoryPressure::Critical => ("danger", "Crítica"),
    }
}

fn score_tone(score: u8) -> String {
    match score {
        70.. => "accent",
        40..=69 => "warn",
        _ => "danger",
    }
    .into()
}

fn severity_style(s: Severity) -> (&'static str, &'static str) {
    match s {
        Severity::Critical => ("danger", "warning"),
        Severity::Warning => ("warn", "warning"),
        Severity::Suggestion => ("accent", "sparkle"),
        Severity::Info => ("neutral", "info"),
    }
}

fn optimize_label(a: OptimizeAction) -> &'static str {
    match a {
        OptimizeAction::PurgeMemory => "Liberar RAM",
        OptimizeAction::FlushDns => "Vaciar DNS",
        OptimizeAction::ThinLocalSnapshots => "Reducir snapshots",
        OptimizeAction::ReindexSpotlight => "Reindexar Spotlight",
        OptimizeAction::CleanSystemCaches => "Limpiar cachés",
    }
}

/// One plain-language sentence summarising the breakdown.
fn diagnosis(b: &MemoryBreakdown) -> String {
    let total = fmt::mem_short(b.total);
    let demand = b.app + b.wired + b.compressed_original;
    let swap_gb = (b.swap_used > 256 * 1_048_576).then(|| fmt::mem_short(b.swap_used));
    let swap = swap_gb.as_ref().map(|g| format!(" y usando {g} de swap")).unwrap_or_default();
    let swap_also = swap_gb.as_ref().map(|g| format!(" y ya usa {g} de swap")).unwrap_or_default();
    let squeezing = b.compressed_original > 0 && b.compressed > 0;
    if demand > b.total && squeezing {
        format!(
            "Tus apps piden {} en un Mac de {total}: macOS está comprimiendo {} en {}{swap}.",
            fmt::mem_short(demand),
            fmt::mem_short(b.compressed_original),
            fmt::mem_short(b.compressed),
        )
    } else if b.pressure != MemoryPressure::Normal && squeezing {
        format!(
            "La RAM está justa: macOS comprime {} en {} para hacer espacio{swap_also}.",
            fmt::mem_short(b.compressed_original),
            fmt::mem_short(b.compressed),
        )
    } else if b.pressure != MemoryPressure::Normal {
        format!("La memoria está bajo presión{swap_also}: cerrar una app grande debería aliviarla.")
    } else {
        format!(
            "Tu memoria está sana: {} en apps, {} de caché que macOS libera al instante y {} libres.",
            fmt::mem_short(b.app),
            fmt::mem_short(b.cached),
            fmt::mem_short(b.free),
        )
    }
}

fn pct_of(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64 * 100.0
    }
}

#[component]
pub fn MemoryPage() -> impl IntoView {
    let app = use_app();
    app.refresh_memory();
    if let Ok(handle) = set_interval_with_handle(
        move || {
            let hidden = web_sys::window()
                .and_then(|w| w.document())
                .is_some_and(|d| d.hidden());
            if !hidden {
                app.refresh_memory();
            }
        },
        POLL,
    ) {
        on_cleanup(move || handle.clear());
    }

    let pending_quit = RwSignal::new(None::<QuitTarget>);
    let quitting = RwSignal::new(false);
    let confirm_quit = move |_| {
        let Some(t) = pending_quit.get_untracked() else { return };
        quitting.set(true);
        spawn_local(async move {
            match api::quit_app(t.pid).await {
                Ok(()) => {
                    app.toast(
                        ToastKind::Success,
                        format!("{} se cerró", t.name),
                        t.footprint.map(|b| format!("Se liberan cerca de {}.", fmt::mem_short(b))),
                    );
                    app.refresh_memory();
                }
                Err(e) => app.toast(ToastKind::Error, format!("No se pudo cerrar {}", t.name), Some(e)),
            }
            quitting.set(false);
            pending_quit.set(None);
        });
    };
    let ask_quit = Callback::new(move |t: QuitTarget| pending_quit.set(Some(t)));

    let loaded = move || app.memory.with(|r| r.is_some());

    view! {
        <PageIntro text="Por qué tu Mac usa tanta memoria y qué puedes hacer. Se actualiza cada 5 segundos.">
            <Pill variant="ghost" icon="refresh" on_click=move |_| app.refresh_memory()>
                "Actualizar"
            </Pill>
        </PageIntro>

        <Show when=loaded fallback=|| view! { <MemorySkeleton /> }>
            <div class="grid memory-top">
                <MemoryHero />
                <BreakdownCard />
            </div>
            <InsightsCard ask_quit=ask_quit />
            <AppsMemoryCard ask_quit=ask_quit />
        </Show>

        <ConfirmDialog
            open=Signal::derive(move || pending_quit.with(|p| p.is_some()))
            title=Signal::derive(move || {
                pending_quit.with(|p| p.as_ref().map(|t| format!("¿Cerrar {}?", t.name)).unwrap_or_default())
            })
            confirm_label="Cerrar app".to_string()
            busy=quitting
            on_confirm=confirm_quit
            on_cancel=move |_| pending_quit.set(None)
        >
            <p class="dialog-text">
                {move || {
                    pending_quit
                        .with(|p| {
                            p.as_ref()
                                .map(|t| {
                                    let how = if t.gui {
                                        format!(
                                            "iclear le pedirá a {} que se cierre de forma ordenada, como si eligieras Salir en su menú. Si tienes cambios sin guardar, la app te preguntará antes.",
                                            t.name,
                                        )
                                    } else {
                                        format!(
                                            "Se enviará SIGTERM a {} (PID {}) para que termine de forma ordenada.",
                                            t.name,
                                            t.pid,
                                        )
                                    };
                                    match t.footprint {
                                        Some(b) => format!("{how} Liberarás cerca de {}.", fmt::mem_short(b)),
                                        None => how,
                                    }
                                })
                                .unwrap_or_default()
                        })
                }}
            </p>
        </ConfirmDialog>
    }
}

#[component]
fn MemorySkeleton() -> impl IntoView {
    view! {
        <div class="grid memory-top">
            <div class="card hero-card">
                <Skeleton height=150 width="150px" />
                <div class="stack" style="flex:1">
                    <Skeleton height=22 width="30%" />
                    <Skeleton height=18 />
                    <Skeleton height=18 width="80%" />
                </div>
            </div>
            <div class="card stack">
                <Skeleton height=14 width="40%" />
                <Skeleton height=16 />
                <Skeleton height=60 />
            </div>
        </div>
        <div class="card stack">
            {(0..4).map(|_| view! { <Skeleton height=48 /> }).collect_view()}
        </div>
    }
}

#[component]
fn MemoryHero() -> impl IntoView {
    let app = use_app();
    let b = Memo::new(move |_| app.memory.with(|r| r.as_ref().map(|r| r.breakdown.clone()).unwrap_or_default()));
    let score = Memo::new(move |_| app.memory.with(|r| r.as_ref().map(|r| r.score).unwrap_or(0)));
    let swap_pct = Signal::derive(move || b.with(|b| pct_of(b.swap_used, b.swap_total)));
    view! {
        <section class="card hero-card memory-hero">
            <Ring
                value=Signal::derive(move || score.get() as f64)
                size=150
                stroke=9.0
                tone=Signal::derive(move || score_tone(score.get()))
            >
                <span class="ring-big num">{move || score.get()}</span>
                <span class="ring-caption">"salud"</span>
            </Ring>
            <div class="hero-info">
                {move || {
                    let (tone, label) = pressure_badge(b.with(|b| b.pressure));
                    view! { <Badge tone=tone.to_string() dot=true>{format!("Presión: {label}")}</Badge> }
                }}
                <p class="hero-line memory-diagnosis">{move || b.with(diagnosis)}</p>
                <div class="memory-facts">
                    <div class="memory-fact">
                        <Tip text="Cuando la RAM no alcanza, macOS mueve memoria al SSD. Es mucho más lento que la RAM y desgasta el disco.">
                            <span class="eyebrow">"Swap"<Icon name="info" size=11 /></span>
                        </Tip>
                        <span class="num">
                            {move || b.with(|b| format!("{} / {}", fmt::mem_short(b.swap_used), fmt::mem_short(b.swap_total)))}
                        </span>
                        <Meter value=swap_pct tone=Signal::derive(move || load_tone(swap_pct.get())) />
                    </div>
                    <div class="memory-fact">
                        <Tip text="macOS comprime la memoria de apps que no estás usando para que quepa más. Funciona, pero cuesta CPU: es un síntoma de que falta RAM.">
                            <span class="eyebrow">"Comprimido"<Icon name="info" size=11 /></span>
                        </Tip>
                        <span class="num">
                            {move || {
                                b.with(|b| {
                                    format!("{} → {}", fmt::mem_short(b.compressed_original), fmt::mem_short(b.compressed))
                                })
                            }}
                        </span>
                        <span class="memory-fact-sub">
                            {move || {
                                b.with(|b| {
                                    if b.compressed > 0 {
                                        format!(
                                            "Relación {}×",
                                            fmt::dec(b.compressed_original as f64 / b.compressed as f64, 1),
                                        )
                                    } else {
                                        "Sin compresión".into()
                                    }
                                })
                            }}
                        </span>
                    </div>
                    <div class="memory-fact">
                        <Tip text="Porcentaje de memoria que macOS considera disponible (kern.memorystatus_level).">
                            <span class="eyebrow">"Disponible"<Icon name="info" size=11 /></span>
                        </Tip>
                        <span class="num">{move || format!("{}%", b.with(|b| b.free_percent))}</span>
                        <span class="memory-fact-sub">
                            {move || b.with(|b| format!("{} enviados a swap desde el arranque", fmt::mem_short(b.swapouts_bytes)))}
                        </span>
                    </div>
                </div>
            </div>
        </section>
    }
}

#[component]
fn BreakdownCard() -> impl IntoView {
    let app = use_app();
    let b = Memo::new(move |_| app.memory.with(|r| r.as_ref().map(|r| r.breakdown.clone()).unwrap_or_default()));
    let buckets = move || {
        b.with(|b| {
            [
                ("app", "App", b.app, "Memoria que tus apps usan activamente: pestañas, documentos, editores."),
                ("wired", "Sistema (fija)", b.wired, "Memoria del kernel y los drivers. macOS no puede liberarla."),
                ("comp", "Comprimida", b.compressed, "Memoria de apps comprimida para ganar espacio. Si crece mucho, a tu Mac le falta RAM."),
                ("cache", "Caché de archivos", b.cached, "Archivos recientes guardados en RAM para abrir rápido. macOS la libera al instante: no es un problema."),
                ("free", "Libre", b.free, "RAM sin usar en este momento."),
            ]
            .into_iter()
            .map(|(class, label, bytes, tip)| (class, label, bytes, tip, pct_of(bytes, b.total)))
            .collect::<Vec<_>>()
        })
    };

    view! {
        <section class="card breakdown-card">
            <header class="card-head">
                <span class="card-icon"><Icon name="memory" size=16 /></span>
                <div class="card-head-text">
                    <h3 class="card-title">"En qué se va la RAM"</h3>
                    <p class="card-sub">{move || format!("{} en total · mismos grupos que Monitor de Actividad", b.with(|b| fmt::mem_short(b.total)))}</p>
                </div>
            </header>
            <div class="mem-bar" role="img" aria-label="Distribución de la memoria">
                {move || {
                    buckets()
                        .into_iter()
                        .filter(|x| x.4 > 0.2)
                        .map(|(class, label, bytes, tip, pct)| {
                            view! {
                                <span
                                    class=format!("mem-seg seg-{class}")
                                    style=format!("width:{pct:.2}%")
                                    title=format!("{label}: {} · {tip}", fmt::mem(bytes))
                                />
                            }
                        })
                        .collect_view()
                }}
            </div>
            <div class="mem-legend">
                {move || {
                    buckets()
                        .into_iter()
                        .map(|(class, label, bytes, tip, pct)| {
                            view! {
                                <Tip text=tip>
                                    <span class="mem-legend-item">
                                        <span class=format!("legend-dot seg-{class}") />
                                        <span class="mem-legend-label">{label}</span>
                                        <span class="mem-legend-value num">{fmt::mem(bytes)}</span>
                                        <span class="mem-legend-pct num">{format!("{}%", pct.round() as i64)}</span>
                                    </span>
                                </Tip>
                            }
                        })
                        .collect_view()
                }}
            </div>
            <p class="footnote">
                "La caché se libera sola cuando hace falta. Lo que indica falta de RAM es la memoria "
                <strong class="warn-text">"comprimida"</strong>
                " y el "
                <strong class="warn-text">"swap"</strong>
                "."
            </p>
        </section>
    }
}

#[component]
fn InsightsCard(ask_quit: Callback<QuitTarget>) -> impl IntoView {
    let app = use_app();
    let insights = Memo::new(move |_| {
        let mut list: Vec<Insight> = app.memory.with(|r| r.as_ref().map(|r| r.insights.clone()).unwrap_or_default());
        list.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then(b.savings_bytes.unwrap_or(0).cmp(&a.savings_bytes.unwrap_or(0)))
        });
        list
    });
    view! {
        <section class="card">
            <header class="card-head">
                <span class="card-icon"><Icon name="sparkle" size=16 /></span>
                <div class="card-head-text">
                    <h3 class="card-title">"Recomendaciones inteligentes"</h3>
                    <p class="card-sub">"Ordenadas por impacto en la memoria"</p>
                </div>
            </header>
            <Show
                when=move || insights.with(|i| !i.is_empty())
                fallback=|| view! { <EmptyState icon="check" title="Todo en orden" text="No hay nada que recomendar por ahora." /> }
            >
                <ul class="insight-list">
                    <For
                        each=move || insights.get()
                        key=|i| (i.id.clone(), i.title.clone(), i.savings_bytes)
                        let:insight
                    >
                        <InsightRow insight=insight ask_quit=ask_quit />
                    </For>
                </ul>
            </Show>
        </section>
    }
}

fn find_app(app: AppState, pid: u32, app_id: &str) -> Option<AppMemory> {
    app.memory.with_untracked(|r| {
        r.as_ref()?
            .apps
            .iter()
            .find(|a| a.main_pid == Some(pid) || a.id == app_id)
            .cloned()
    })
}

#[component]
fn InsightRow(insight: Insight, ask_quit: Callback<QuitTarget>) -> impl IntoView {
    let app = use_app();
    let (tone, icon) = severity_style(insight.severity);
    let running = RwSignal::new(false);

    let action = insight.action.clone().filter(|action| match action {
        InsightAction::QuitApp { app_id, main_pid } => {
            !find_app(app, *main_pid, app_id).is_some_and(|a| quit_protected(&a))
        }
        _ => true,
    });
    let action = action.map(|action| match action {
        InsightAction::QuitApp { app_id, main_pid } => {
            let fallback = insight.title.clone();
            view! {
                <Pill
                    variant="sm"
                    icon="power"
                    on_click=move |_| {
                        let found = find_app(app, main_pid, &app_id);
                        ask_quit.run(QuitTarget {
                            name: found.as_ref().map(|a| a.name.clone()).unwrap_or_else(|| fallback.clone()),
                            pid: main_pid,
                            footprint: found.as_ref().map(|a| a.footprint_bytes),
                            gui: found.as_ref().is_none_or(|a| a.is_gui),
                        })
                    }
                >
                    "Cerrar app"
                </Pill>
            }
            .into_any()
        }
        InsightAction::Optimize { action } => {
            let label = optimize_label(action);
            view! {
                <Pill
                    variant="sm"
                    icon="bolt"
                    busy=running
                    on_click=move |_| {
                        if !app.helper_ready() {
                            app.toast(
                                ToastKind::Info,
                                "Se necesita el asistente de iclear",
                                Some("Instálalo una vez desde Ajustes para usar esta acción.".into()),
                            );
                            app.page.set(Page::Ajustes);
                            return;
                        }
                        running.set(true);
                        spawn_local(async move {
                            match api::run_optimization(action).await {
                                Ok(msg) => {
                                    app.toast(ToastKind::Success, label, Some(msg));
                                    app.refresh_memory();
                                }
                                Err(e) => app.toast(ToastKind::Error, format!("{label}: falló"), Some(e)),
                            }
                            running.set(false);
                        });
                    }
                >
                    {label}
                </Pill>
            }
            .into_any()
        }
        InsightAction::OpenPage { page } => {
            let target = Page::from_id(&page);
            let label = target.map(|p| format!("Ir a {}", p.title())).unwrap_or_else(|| "Abrir".into());
            view! {
                <Pill
                    variant="sm"
                    icon="chevron-right"
                    disabled=target.is_none()
                    on_click=move |_| {
                        if let Some(p) = target {
                            app.page.set(p)
                        }
                    }
                >
                    {label}
                </Pill>
            }
            .into_any()
        }
    });

    view! {
        <li class=format!("insight tone-{tone}")>
            <span class="insight-icon"><Icon name=icon size=16 /></span>
            <div class="insight-text">
                <p class="insight-title">{insight.title.clone()}</p>
                <p class="insight-detail">{insight.detail.clone()}</p>
            </div>
            <div class="insight-side">
                {insight
                    .savings_bytes
                    .map(|b| view! { <Badge tone="accent".to_string()>{format!("libera ~{}", fmt::mem_short(b))}</Badge> })}
                {action}
            </div>
        </li>
    }
}

#[component]
fn AppsMemoryCard(ask_quit: Callback<QuitTarget>) -> impl IntoView {
    let app = use_app();
    let sort = RwSignal::new((Col::Mem, true));
    let show_all = RwSignal::new(false);

    let total_ram = Memo::new(move |_| app.memory.with(|r| r.as_ref().map(|r| r.breakdown.total).unwrap_or(0)));
    let observed = Memo::new(move |_| app.memory.with(|r| r.as_ref().map(|r| r.observed_secs).unwrap_or(0)));
    let rows = Memo::new(move |_| {
        let (col, desc) = sort.get();
        let mut list: Vec<AppMemory> = app.memory.with(|r| r.as_ref().map(|r| r.apps.clone()).unwrap_or_default());
        list.sort_by(|a, b| {
            let o = match col {
                Col::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                Col::Mem => a.footprint_bytes.cmp(&b.footprint_bytes),
                Col::Cpu => a.cpu.total_cmp(&b.cpu),
            };
            if desc { o.reverse() } else { o }
        });
        list
    });
    let max_fp = Memo::new(move |_| rows.with(|r| r.iter().map(|a| a.footprint_bytes).max().unwrap_or(1).max(1)));

    view! {
        <section class="card table-card">
            <header class="card-head wrap">
                <div class="card-head-row">
                    <span class="card-icon"><Icon name="list" size=16 /></span>
                    <div class="card-head-text">
                        <h3 class="card-title">"Qué está usando tu memoria"</h3>
                        <p class="card-sub">"Procesos agrupados por app: ayudantes y pestañas se suman a su app"</p>
                    </div>
                </div>
                <Show when=move || rows.with(|r| r.len() > TOP_APPS)>
                    <button type="button" class="link-btn" on:click=move |_| show_all.update(|v| *v = !*v)>
                        {move || {
                            if show_all.get() {
                                format!("Ver solo las {TOP_APPS} principales")
                            } else {
                                format!("Ver todas ({})", rows.with(|r| r.len()))
                            }
                        }}
                    </button>
                </Show>
            </header>
            <Show when=move || { observed.get() < 120 }>
                <div class="note learn-note">
                    <Icon name="clock" size=16 />
                    <span class="note-text">
                        "iclear aprende tus hábitos: los datos de uso mejoran tras unos minutos abierto."
                    </span>
                </div>
            </Show>
            <div class="table-wrap">
                <table class="table mem-apps">
                    <thead>
                        <tr>
                            <SortHeader label="App" key=Col::Name sort=sort />
                            <SortHeader label="Memoria" key=Col::Mem sort=sort numeric=true class="col-mem" />
                            <th class="num-col hide-narrow">"% RAM"</th>
                            <SortHeader label="CPU" key=Col::Cpu sort=sort numeric=true class="hide-narrow" />
                            <th>"Actividad"</th>
                            <th class="col-actions"><span class="sr-only">"Acciones"</span></th>
                        </tr>
                    </thead>
                    <tbody>
                        {move || {
                            let limit = if show_all.get() { usize::MAX } else { TOP_APPS };
                            let max = max_fp.get();
                            let total = total_ram.get();
                            rows.get()
                                .into_iter()
                                .take(limit)
                                .map(|a| view! { <AppMemoryRow a=a max=max total=total ask_quit=ask_quit /> })
                                .collect_view()
                        }}
                    </tbody>
                </table>
            </div>
        </section>
    }
}

#[component]
fn AppMemoryRow(a: AppMemory, max: u64, total: u64, ask_quit: Callback<QuitTarget>) -> impl IntoView {
    let bar = pct_of(a.footprint_bytes, max);
    let share = pct_of(a.footprint_bytes, total);
    let helpers = if a.renderer_count > 0 {
        let what = if a.kind == AppKind::Browser { "pestañas" } else { "renderers" };
        format!("{} · {} {what}", fmt::plural(a.process_count, "proceso", "procesos"), a.renderer_count)
    } else {
        fmt::plural(a.process_count, "proceso", "procesos")
    };
    let activity = if a.is_gui {
        match a.last_focused_secs_ago {
            Some(s) if s < 60 => Some(view! { <span class="activity-now">"En uso"</span> }.into_any()),
            Some(s) => Some(view! { <span class="muted">{format!("Sin usar {}", fmt::ago_secs(s))}</span> }.into_any()),
            None => None,
        }
    } else {
        Some(view! { <span class="muted">"Segundo plano"</span> }.into_any())
    };
    let leak = a.growth_per_hour.filter(|g| *g > LEAK_PER_HOUR).map(|g| {
        view! {
            <Tip text="Su memoria crece de forma sostenida: posible fuga. Reiniciar la app la devuelve a lo normal.">
                <Badge tone="warn".to_string()>
                    <Icon name="trend-up" size=12 />
                    {fmt::per_hour(g)}
                </Badge>
            </Tip>
        }
    });
    let action = match (a.killable && !quit_protected(&a), a.main_pid) {
        (true, Some(pid)) => {
            let target = QuitTarget { name: a.name.clone(), pid, footprint: Some(a.footprint_bytes), gui: a.is_gui };
            view! {
                <button type="button" class="mini-btn" on:click=move |_| ask_quit.run(target.clone())>
                    "Cerrar"
                </button>
            }
            .into_any()
        }
        _ if a.killable => ().into_any(),
        _ => view! {
            <span class="protected" title="Parte de macOS">
                <Icon name="lock" size=13 />
            </span>
        }
        .into_any(),
    };
    let hot = a.cpu >= 50.0;
    let kind_class = format!("kind-chip kind-{}", format!("{:?}", a.kind).to_lowercase());

    view! {
        <tr>
            <td class="mapp-cell">
                <div class="mapp-name">
                    <span class="truncate">{a.name.clone()}</span>
                    <span class=kind_class>{kind_label(a.kind)}</span>
                </div>
                <span class="mapp-sub truncate">{helpers}</span>
            </td>
            <td class="num-col col-mem">
                <div class="cpu-cell">
                    <span class="cpu-bar mem-bar-cell">
                        <span class="cpu-fill" style=format!("width:{bar:.1}%") />
                    </span>
                    <span class="num">{fmt::mem(a.footprint_bytes)}</span>
                </div>
            </td>
            <td class="num-col num muted hide-narrow">{format!("{}%", fmt::dec(share, 1))}</td>
            <td class="num-col num hide-narrow" class:warn-text=hot>{fmt::dec(a.cpu as f64, 1)}</td>
            <td class="activity-cell">
                <div class="activity">
                    {activity}
                    {leak}
                </div>
            </td>
            <td class="col-actions">{action}</td>
        </tr>
    }
}
