//! Apps: detects installed apps the user doesn't use, plus leftovers of
//! apps that were already removed.

use std::collections::HashSet;

use iclear_protocol::{CleanupPreview, DeleteReport, InstalledApp, OrphanGroup, UsageVerdict};
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::storage::report_toast;
use super::PageIntro;
use crate::components::*;
use crate::state::{use_app, ToastKind};
use crate::{api, fmt};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Unused,
    Occasional,
    Active,
    Login,
}

impl Filter {
    fn matches(self, a: &InstalledApp) -> bool {
        match self {
            Filter::All => true,
            Filter::Unused => a.verdict == UsageVerdict::Unused,
            Filter::Occasional => a.verdict == UsageVerdict::Occasional,
            Filter::Active => a.verdict == UsageVerdict::Active,
            Filter::Login => a.launches_at_login,
        }
    }
}

const FILTERS: &[(Filter, &str)] = &[
    (Filter::All, "Todas"),
    (Filter::Unused, "Sin uso"),
    (Filter::Occasional, "Ocasionales"),
    (Filter::Active, "Activas"),
    (Filter::Login, "Se abren al iniciar"),
];

const SCAN_STEPS: &[&str] = &[
    "Buscando apps en /Applications",
    "Consultando la última apertura en Spotlight",
    "Midiendo el tamaño de cada app",
    "Buscando restos en ~/Library",
    "Detectando restos de apps desinstaladas",
];

fn verdict_badge(v: UsageVerdict) -> (&'static str, &'static str) {
    match v {
        UsageVerdict::Active => ("accent", "Activa"),
        UsageVerdict::Occasional => ("neutral", "Ocasional"),
        UsageVerdict::Unused => ("warn", "Sin uso"),
        UsageVerdict::Unknown => ("neutral", "Sin datos"),
    }
}

fn last_used_text(a: &InstalledApp) -> String {
    if a.is_running {
        return "ahora".into();
    }
    match (a.verdict, a.last_used_days_ago) {
        (UsageVerdict::Unknown, None) => "sin datos".into(),
        (_, d) => fmt::last_used(d),
    }
}

#[component]
pub fn AppsPage() -> impl IntoView {
    let app = use_app();
    if app.apps.with_untracked(|r| r.is_none()) {
        app.scan_apps();
    }

    let filter = RwSignal::new(Filter::All);
    let sort = RwSignal::new("size".to_string());
    let expanded = RwSignal::new(HashSet::<String>::new());

    let report = app.apps;
    let all_apps = Memo::new(move |_| report.with(|r| r.as_ref().map(|r| r.apps.clone()).unwrap_or_default()));
    let rows = Memo::new(move |_| {
        let f = filter.get();
        let mut list: Vec<InstalledApp> = all_apps.with(|a| a.iter().filter(|a| f.matches(a)).cloned().collect());
        match sort.get().as_str() {
            "last" => list.sort_by_key(|a| {
                // Least recently used first; never opened beats everything,
                // apps without data go last.
                let unknown = a.verdict == UsageVerdict::Unknown;
                let days = if a.is_running { 0 } else { a.last_used_days_ago.unwrap_or(u64::MAX) };
                (unknown, std::cmp::Reverse(days))
            }),
            "name" => list.sort_by_key(|a| a.name.to_lowercase()),
            _ => list.sort_by_key(|a| std::cmp::Reverse(a.size_bytes + a.leftovers_bytes)),
        }
        list
    });
    let count = move |f: Filter| all_apps.with(|a| a.iter().filter(|a| f.matches(a)).count());

    // Uninstall / orphan removal share one review dialog.
    let preview = RwSignal::new(None::<CleanupPreview>);
    let review_title = RwSignal::new(String::new());
    let preparing = RwSignal::new(None::<String>);
    let uninstall = Callback::new(move |a: InstalledApp| {
        preparing.set(Some(a.path.clone()));
        spawn_local(async move {
            match api::preview_uninstall(a.path.clone()).await {
                Ok(p) => {
                    review_title.set(format!("Desinstalar {}", a.name));
                    preview.set(Some(p));
                }
                Err(e) => app.toast(ToastKind::Error, format!("No se pudo preparar la desinstalación de {}", a.name), Some(e)),
            }
            preparing.set(None);
        });
    });
    let on_done = move |r: DeleteReport| {
        report_toast(app, &r);
        app.scan_apps();
    };

    let has_report = move || report.with(|r| r.is_some());
    let first_scan = move || app.apps_loading.get() && !has_report();

    view! {
        <PageIntro text="Detecta las apps que no usas según la última vez que las abriste, cuánto ocupan con sus archivos de soporte y qué restos dejaron las que ya borraste.">
            <Pill variant="ghost" icon="refresh" busy=app.apps_loading on_click=move |_| app.scan_apps()>
                "Volver a analizar"
            </Pill>
        </PageIntro>

        {move || app.apps_error.get().map(|e| view! { <ErrorNote message=e /> })}

        <Show when=move || app.apps_loading.get()>
            <Scanning
                title=if has_report() { "Actualizando el análisis…" } else { "Analizando tus apps…" }
                steps=SCAN_STEPS.to_vec()
                icon="apps"
            />
        </Show>

        <Show when=first_scan>
            <div class="card stack">
                {(0..6).map(|_| view! { <Skeleton height=46 /> }).collect_view()}
            </div>
        </Show>

        <Show when=move || !first_scan()>
            <div class="grid grid-4 tight">
                <StatTile
                    label="Apps instaladas"
                    icon="apps"
                    value=Signal::derive(move || all_apps.with(|a| a.len().to_string()))
                    sub=Signal::derive(move || {
                        let n = count(Filter::Login);
                        if n == 0 { String::new() } else { format!("{} se abren al iniciar", n) }
                    })
                />
                <StatTile
                    label="Sin uso"
                    icon="clock"
                    value=Signal::derive(move || count(Filter::Unused).to_string())
                    sub="Más de 90 días sin abrir"
                    tone=Signal::derive(move || if count(Filter::Unused) > 0 { "warn".to_string() } else { String::new() })
                />
                <StatTile
                    label="Espacio recuperable"
                    icon="disk"
                    value=Signal::derive(move || report.with(|r| fmt::bytes(r.as_ref().map(|r| r.unused_bytes).unwrap_or(0))))
                    sub="Apps sin uso y sus restos"
                />
                <StatTile
                    label="Restos huérfanos"
                    icon="package"
                    value=Signal::derive(move || report.with(|r| fmt::bytes(r.as_ref().map(|r| r.orphans_bytes).unwrap_or(0))))
                    sub=Signal::derive(move || {
                        report.with(|r| {
                            let n = r.as_ref().map(|r| r.orphans.len()).unwrap_or(0);
                            fmt::plural(n, "app desinstalada", "apps desinstaladas")
                        })
                    })
                />
            </div>

            <section class="card apps-card" class:dimmed=move || app.apps_loading.get()>
                <header class="card-head wrap">
                    <div class="toolbar">
                        {FILTERS
                            .iter()
                            .map(|&(f, label)| {
                                view! {
                                    <Chip selected=Signal::derive(move || filter.get() == f) on_click=move |_| filter.set(f)>
                                        {label}
                                        <span class="chip-count num">{move || count(f)}</span>
                                    </Chip>
                                }
                            })
                            .collect_view()}
                    </div>
                    <Segmented
                        options=vec![("size", "Tamaño"), ("last", "Último uso"), ("name", "Nombre")]
                        selected=sort
                        on_select=move |v: String| sort.set(v)
                    />
                </header>
                <Show
                    when=move || rows.with(|r| !r.is_empty())
                    fallback=|| view! { <EmptyState icon="check" title="Nada por aquí" text="Ninguna app coincide con este filtro." /> }
                >
                    <ul class="app-list">
                        <For each=move || rows.get() key=|a| (a.path.clone(), a.leftovers_bytes, a.is_running) let:a>
                            <AppRow a=a expanded=expanded preparing=preparing on_uninstall=uninstall />
                        </For>
                    </ul>
                </Show>
            </section>

            <OrphansCard preview=preview review_title=review_title />
        </Show>

        <CleanupReview preview=preview on_done=on_done title=review_title />
    }
}

#[component]
fn AppRow(
    a: InstalledApp,
    expanded: RwSignal<HashSet<String>>,
    preparing: RwSignal<Option<String>>,
    on_uninstall: Callback<InstalledApp>,
) -> impl IntoView {
    let path = a.path.clone();
    let path_toggle = a.path.clone();
    let path_busy = a.path.clone();
    let is_open = Signal::derive(move || expanded.with(|e| e.contains(&path)));
    let toggle = move || {
        expanded.update(|e| {
            if !e.remove(&path_toggle) {
                e.insert(path_toggle.clone());
            }
        })
    };
    let busy = Signal::derive(move || preparing.with(|p| p.as_deref() == Some(path_busy.as_str())));
    let (tone, verdict) = verdict_badge(a.verdict);
    let initial = a.name.chars().next().unwrap_or('?').to_uppercase().to_string();
    let unused = a.verdict == UsageVerdict::Unused;

    let chips = [
        (a.is_running, "En ejecución", "accent"),
        (a.launches_at_login, "Al iniciar", "warn"),
        (a.is_electron, "Electron", "neutral"),
        (a.from_app_store, "App Store", "neutral"),
    ]
    .into_iter()
    .filter(|c| c.0)
    .map(|(_, label, tone)| view! { <span class=format!("tag tone-{tone}")>{label}</span> })
    .collect_view();

    let uninstall_btn = {
        let target = a.clone();
        let btn = view! {
            <Pill
                variant="sm danger-text"
                icon="trash"
                busy=busy
                disabled=!a.removable
                on_click=move |_| on_uninstall.run(target.clone())
            >
                "Desinstalar"
            </Pill>
        };
        if a.removable {
            btn.into_any()
        } else {
            let why = if a.is_running {
                "Ciérrala antes de desinstalarla."
            } else {
                "Las apps de Apple y las protegidas por macOS no se pueden desinstalar."
            };
            view! { <Tip text=why side="left">{btn}</Tip> }.into_any()
        }
    };

    let has_leftovers = !a.leftovers.is_empty();
    let detail_rows = {
        let mut list = vec![("App".to_string(), a.path.clone(), a.size_bytes)];
        list.extend(a.leftovers.iter().map(|l| (l.kind.clone(), l.path.clone(), l.size_bytes)));
        list
    };

    view! {
        <li class="app-row" class:open=is_open class:unused=unused>
            <div class="app-main">
                <span class="app-avatar" aria-hidden="true">{initial}</span>
                <div class="app-info">
                    <div class="app-title">
                        <span class="app-name truncate">{a.name.clone()}</span>
                        {a.version.clone().map(|v| view! { <span class="app-version mono">{v}</span> })}
                        <Badge tone=tone.to_string() dot=true>{verdict}</Badge>
                    </div>
                    <p class="app-reason">{a.reason.clone()}</p>
                    <div class="app-tags">{chips}</div>
                </div>
                <div class="app-meta">
                    <span class="app-size num">{fmt::bytes(a.size_bytes + a.leftovers_bytes)}</span>
                    <span class="app-size-sub">
                        {if has_leftovers {
                            format!("{} + {} de restos", fmt::bytes(a.size_bytes), fmt::bytes(a.leftovers_bytes))
                        } else {
                            "sin restos".to_string()
                        }}
                    </span>
                    <span class="app-last">
                        <Icon name="clock" size=12 />
                        {format!("Último uso: {}", last_used_text(&a))}
                    </span>
                </div>
                <div class="app-actions">
                    {uninstall_btn}
                    <button
                        type="button"
                        class="icon-btn subtle app-expand"
                        aria-label="Ver archivos de la app"
                        aria-expanded=move || is_open.get().to_string()
                        on:click=move |_| toggle()
                    >
                        <Icon name="chevron-down" size=16 />
                    </button>
                </div>
            </div>
            <Show when=move || is_open.get()>
                <ul class="leftover-list">
                    {detail_rows
                        .iter()
                        .map(|(kind, path, size)| {
                            view! {
                                <li class="leftover-row">
                                    <span class="tag">{kind.clone()}</span>
                                    <span class="leftover-path mono truncate" title=path.clone()>{fmt::short_path(path)}</span>
                                    <span class="leftover-size num">{fmt::bytes(*size)}</span>
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
            </Show>
        </li>
    }
}

#[component]
fn OrphansCard(preview: RwSignal<Option<CleanupPreview>>, review_title: RwSignal<String>) -> impl IntoView {
    let app = use_app();
    let orphans = Memo::new(move |_| app.apps.with(|r| r.as_ref().map(|r| r.orphans.clone()).unwrap_or_default()));
    let selected = RwSignal::new(HashSet::<String>::new());
    // Preselect every group whenever a new report arrives.
    Effect::new(move |_| {
        selected.set(orphans.with(|o| o.iter().map(|g| g.bundle_id.clone()).collect()));
    });
    let chosen = Memo::new(move |_| {
        orphans.with(|o| {
            selected.with(|s| {
                let picked: Vec<&OrphanGroup> = o.iter().filter(|g| s.contains(&g.bundle_id)).collect();
                (picked.len(), picked.iter().map(|g| g.total_bytes).sum::<u64>())
            })
        })
    });
    let previewing = RwSignal::new(false);
    let remove = move |_| {
        let ids: Vec<String> = selected.get_untracked().into_iter().collect();
        if ids.is_empty() {
            return;
        }
        previewing.set(true);
        spawn_local(async move {
            match api::preview_orphans(ids).await {
                Ok(p) => {
                    review_title.set("Eliminar restos de apps desinstaladas".into());
                    preview.set(Some(p));
                }
                Err(e) => app.toast(ToastKind::Error, "No se pudo preparar la limpieza de restos", Some(e)),
            }
            previewing.set(false);
        });
    };

    view! {
        <section class="card">
            <header class="card-head wrap">
                <div class="card-head-row">
                    <span class="card-icon"><Icon name="package" size=16 /></span>
                    <div class="card-head-text">
                        <h3 class="card-title">"Restos de apps ya desinstaladas"</h3>
                        <p class="card-sub">"Soporte, cachés y preferencias que quedaron en ~/Library"</p>
                    </div>
                </div>
                <Show when=move || orphans.with(|o| !o.is_empty())>
                    <div class="toolbar">
                        <button
                            type="button"
                            class="link-btn"
                            on:click=move |_| selected.set(orphans.with_untracked(|o| o.iter().map(|g| g.bundle_id.clone()).collect()))
                        >
                            "Todos"
                        </button>
                        <button type="button" class="link-btn" on:click=move |_| selected.set(HashSet::new())>
                            "Ninguno"
                        </button>
                        <Pill
                            variant="danger"
                            icon="trash"
                            busy=previewing
                            disabled=Signal::derive(move || chosen.get().0 == 0)
                            on_click=remove
                        >
                            {move || {
                                let (n, bytes) = chosen.get();
                                if n == 0 { "Eliminar seleccionados".to_string() } else { format!("Eliminar seleccionados · {}", fmt::bytes(bytes)) }
                            }}
                        </Pill>
                    </div>
                </Show>
            </header>
            <Show
                when=move || orphans.with(|o| !o.is_empty())
                fallback=|| view! { <EmptyState icon="check" title="Sin restos" text="No encontramos archivos de apps que ya no estén instaladas." /> }
            >
                <ul class="orphan-list">
                    <For each=move || orphans.get() key=|g| (g.bundle_id.clone(), g.total_bytes) let:g>
                        {
                            let id = g.bundle_id.clone();
                            let id_toggle = g.bundle_id.clone();
                            let checked = Signal::derive(move || selected.with(|s| s.contains(&id)));
                            view! {
                                <li class="orphan" class:off=move || !checked.get()>
                                    <div class="orphan-head">
                                        <Check
                                            checked=checked
                                            label=format!("Incluir {}", g.bundle_id)
                                            on_toggle=move |on: bool| {
                                                selected.update(|s| {
                                                    if on {
                                                        s.insert(id_toggle.clone());
                                                    } else {
                                                        s.remove(&id_toggle);
                                                    }
                                                })
                                            }
                                        />
                                        <span class="orphan-id mono truncate">{g.bundle_id.clone()}</span>
                                        <span class="orphan-age">{format!("modificado {}", fmt::days_ago(g.modified_days_ago))}</span>
                                        <span class="orphan-size num">{fmt::bytes(g.total_bytes)}</span>
                                    </div>
                                    <ul class="leftover-list nested">
                                        {g.leftovers
                                            .iter()
                                            .map(|l| {
                                                view! {
                                                    <li class="leftover-row">
                                                        <span class="tag">{l.kind.clone()}</span>
                                                        <span class="leftover-path mono truncate" title=l.path.clone()>{fmt::short_path(&l.path)}</span>
                                                        <span class="leftover-size num">{fmt::bytes(l.size_bytes)}</span>
                                                    </li>
                                                }
                                            })
                                            .collect_view()}
                                    </ul>
                                </li>
                            }
                        }
                    </For>
                </ul>
            </Show>
        </section>
    }
}
