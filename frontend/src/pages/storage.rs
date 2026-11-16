use std::collections::HashSet;

use iclear_protocol::{CleanupPreview, DeleteReport, DuplicateGroup, LargeFile, UsageReport};
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::{root_disk, PageIntro};
use crate::components::*;
use crate::state::{use_app, AppState, ToastKind};
use crate::{api, fmt};

const MIN_MB_OPTIONS: &[u64] = &[50, 100, 250, 500, 1000];
const LARGE_LIMIT: usize = 200;

fn reveal(app: AppState, path: String) {
    spawn_local(async move {
        if let Err(e) = api::reveal_in_finder(path).await {
            app.toast(ToastKind::Error, "No se pudo abrir Finder", Some(e));
        }
    });
}

#[component]
pub fn StoragePage() -> impl IntoView {
    let app = use_app();

    // Folder breakdown
    let stack = RwSignal::new(Vec::<String>::new());
    let usage = RwSignal::new(None::<UsageReport>);
    let usage_loading = RwSignal::new(false);
    let usage_err = RwSignal::new(None::<String>);
    let current = move || stack.with(|s| s.last().cloned());

    let load_usage = move || {
        let path = stack.with_untracked(|s| s.last().cloned());
        usage_loading.set(true);
        usage_err.set(None);
        spawn_local(async move {
            match api::disk_usage(path).await {
                Ok(r) => usage.set(Some(r)),
                Err(e) => usage_err.set(Some(e)),
            }
            usage_loading.set(false);
        });
    };
    load_usage();

    // Scans
    let tab = RwSignal::new("large".to_string());
    let min_mb = RwSignal::new(100_u64);
    let large = RwSignal::new(None::<Vec<LargeFile>>);
    let large_loading = RwSignal::new(false);
    let dups = RwSignal::new(None::<Vec<DuplicateGroup>>);
    let dups_loading = RwSignal::new(false);
    let scan_err = RwSignal::new(None::<String>);
    let selected = RwSignal::new(HashSet::<String>::new());

    let scan = move || {
        let path = stack.with_untracked(|s| s.last().cloned());
        let mb = min_mb.get_untracked();
        scan_err.set(None);
        selected.set(HashSet::new());
        if tab.get_untracked() == "large" {
            large_loading.set(true);
            spawn_local(async move {
                match api::large_files(path, mb, LARGE_LIMIT).await {
                    Ok(v) => large.set(Some(v)),
                    Err(e) => scan_err.set(Some(e)),
                }
                large_loading.set(false);
            });
        } else {
            dups_loading.set(true);
            spawn_local(async move {
                match api::find_duplicates(path, mb).await {
                    Ok(v) => dups.set(Some(v)),
                    Err(e) => scan_err.set(Some(e)),
                }
                dups_loading.set(false);
            });
        }
    };

    let size_of = move |path: &str| -> u64 {
        large
            .with_untracked(|l| l.as_ref().and_then(|l| l.iter().find(|f| f.path == path).map(|f| f.size_bytes)))
            .or_else(|| {
                dups.with_untracked(|d| {
                    d.as_ref().and_then(|d| d.iter().find(|g| g.paths.iter().any(|p| p == path)).map(|g| g.size_bytes))
                })
            })
            .unwrap_or(0)
    };
    let selected_total = Memo::new(move |_| {
        large.track();
        dups.track();
        selected.with(|s| s.iter().map(|p| size_of(p)).sum::<u64>())
    });

    // Delete flow
    let preview = RwSignal::new(None::<CleanupPreview>);
    let previewing = RwSignal::new(false);
    let review = move |_| {
        let paths: Vec<String> = selected.get_untracked().into_iter().collect();
        if paths.is_empty() {
            return;
        }
        previewing.set(true);
        spawn_local(async move {
            match api::preview_paths(paths).await {
                Ok(p) => preview.set(Some(p)),
                Err(e) => app.toast(ToastKind::Error, "No se pudo preparar el borrado", Some(e)),
            }
            previewing.set(false);
        });
    };
    let on_done = move |r: DeleteReport| {
        report_toast(app, &r);
        selected.set(HashSet::new());
        load_usage();
        if large.with_untracked(|l| l.is_some()) || dups.with_untracked(|d| d.is_some()) {
            scan();
        }
    };

    let disk = Memo::new(move |_| app.metrics.with(|m| m.as_ref().and_then(|m| root_disk(&m.system).cloned())));
    let disk_pct = Signal::derive(move || {
        disk.with(|d| {
            d.as_ref()
                .filter(|d| d.total_bytes > 0)
                .map(|d| (d.total_bytes - d.available_bytes) as f64 / d.total_bytes as f64 * 100.0)
                .unwrap_or(0.0)
        })
    });

    view! {
        <PageIntro text="Descubre qué ocupa espacio. Navega por las carpetas, busca archivos grandes o duplicados y elimina lo que ya no necesitas." />

        <div class="grid storage-top">
            <section class="card disk-card">
                <header class="card-head">
                    <span class="card-icon"><Icon name="disk" size=16 /></span>
                    <div class="card-head-text">
                        <h3 class="card-title">{move || disk.with(|d| d.as_ref().map(|d| d.name.clone()).unwrap_or_else(|| "Disco".into()))}</h3>
                        <p class="card-sub">"Volumen de arranque"</p>
                    </div>
                </header>
                <div class="disk-body">
                    <Ring value=disk_pct size=168 stroke=10.0 tone=Signal::derive(move || load_tone(disk_pct.get()))>
                        <span class="ring-value-text num">{move || fmt::pct(disk_pct.get() as f32)}</span>
                        <span class="ring-caption">"en uso"</span>
                    </Ring>
                    <div class="legend">
                        <div class="legend-row">
                            <span class="legend-dot used" />
                            <span>"Usado"</span>
                            <span class="num">{move || disk.with(|d| d.as_ref().map(|d| fmt::bytes(d.total_bytes - d.available_bytes)).unwrap_or_default())}</span>
                        </div>
                        <div class="legend-row">
                            <span class="legend-dot free" />
                            <span>"Disponible"</span>
                            <span class="num">{move || disk.with(|d| d.as_ref().map(|d| fmt::bytes(d.available_bytes)).unwrap_or_default())}</span>
                        </div>
                        <div class="legend-row total">
                            <span />
                            <span>"Capacidad"</span>
                            <span class="num">{move || disk.with(|d| d.as_ref().map(|d| fmt::bytes(d.total_bytes)).unwrap_or_default())}</span>
                        </div>
                    </div>
                </div>
            </section>

            <section class="card breakdown">
                <header class="card-head">
                    <span class="card-icon"><Icon name="folder" size=16 /></span>
                    <nav class="crumbs" aria-label="Ruta">
                        <button
                            type="button"
                            class="crumb"
                            class:current=move || stack.with(|s| s.is_empty())
                            on:click=move |_| {
                                stack.set(vec![]);
                                load_usage();
                            }
                        >
                            <Icon name="home" size=14 />
                            "Inicio"
                        </button>
                        {move || {
                            let s = stack.get();
                            let n = s.len();
                            s.into_iter()
                                .enumerate()
                                .map(|(i, p)| {
                                    view! {
                                        <Icon name="chevron-right" size=12 class="crumb-sep" />
                                        <button
                                            type="button"
                                            class="crumb"
                                            class:current=i + 1 == n
                                            on:click=move |_| {
                                                stack.update(|s| s.truncate(i + 1));
                                                load_usage();
                                            }
                                        >
                                            {fmt::file_name(&p)}
                                        </button>
                                    }
                                })
                                .collect_view()
                        }}
                    </nav>
                    <span class="card-figure num">
                        {move || usage.with(|u| u.as_ref().map(|u| fmt::bytes(u.total_bytes)).unwrap_or_default())}
                    </span>
                    <button
                        type="button"
                        class="icon-btn"
                        title="Mostrar en Finder"
                        aria-label="Mostrar carpeta actual en Finder"
                        on:click=move |_| {
                            if let Some(root) = usage.with_untracked(|u| u.as_ref().map(|u| u.root.clone())) {
                                reveal(app, root)
                            }
                        }
                    >
                        <Icon name="external" size=15 />
                    </button>
                </header>
                {move || {
                    if let Some(e) = usage_err.get() {
                        return view! { <ErrorNote message=e /> }.into_any();
                    }
                    if usage_loading.get() || usage.with(|u| u.is_none()) {
                        return view! {
                            <div class="stack">
                                <Loading label="Calculando tamaños de carpetas…" />
                                {(0..6).map(|_| view! { <Skeleton height=30 /> }).collect_view()}
                            </div>
                        }
                            .into_any();
                    }
                    let report = usage.get().unwrap();
                    if report.children.is_empty() {
                        return view! { <EmptyState icon="folder" title="Carpeta vacía" /> }.into_any();
                    }
                    let largest = report.children.iter().map(|c| c.size_bytes).max().unwrap_or(1).max(1);
                    let total = report.total_bytes.max(1);
                    let skipped = report.skipped_errors;
                    view! {
                        <ul class="usage-list">
                            {report
                                .children
                                .into_iter()
                                .map(|c| {
                                    let w = c.size_bytes as f64 / largest as f64 * 100.0;
                                    let share = c.size_bytes as f64 / total as f64 * 100.0;
                                    let path = c.path.clone();
                                    let path_reveal = c.path.clone();
                                    let is_dir = c.is_dir;
                                    view! {
                                        <li class="usage-row" class:is-dir=is_dir>
                                            <button
                                                type="button"
                                                class="usage-main"
                                                disabled=!is_dir
                                                on:click=move |_| {
                                                    if is_dir {
                                                        stack.update(|s| s.push(path.clone()));
                                                        load_usage();
                                                    }
                                                }
                                            >
                                                <Icon name=if is_dir { "folder" } else { "file" } size=15 />
                                                <span class="usage-name truncate">{c.name}</span>
                                                <span class="usage-bar">
                                                    <span class="usage-fill" style=format!("width:{w:.1}%") />
                                                </span>
                                                <span class="usage-share num">{format!("{}%", fmt::dec(share, 1))}</span>
                                                <span class="usage-size num">{fmt::bytes(c.size_bytes)}</span>
                                            </button>
                                            <button
                                                type="button"
                                                class="icon-btn subtle"
                                                title="Mostrar en Finder"
                                                aria-label="Mostrar en Finder"
                                                on:click=move |_| reveal(app, path_reveal.clone())
                                            >
                                                <Icon name="external" size=14 />
                                            </button>
                                        </li>
                                    }
                                })
                                .collect_view()}
                        </ul>
                        {(skipped > 0).then(|| view! {
                            <p class="footnote">
                                {format!("{} sin acceso. Concede Acceso total al disco en Ajustes para un análisis completo.", fmt::plural(skipped, "elemento", "elementos"))}
                            </p>
                        })}
                    }
                        .into_any()
                }}
            </section>
        </div>

        <section class="card">
            <header class="card-head wrap">
                <div class="tabs" role="tablist">
                    <button
                        type="button"
                        role="tab"
                        class="tab"
                        class:active=move || tab.get() == "large"
                        aria-selected=move || (tab.get() == "large").to_string()
                        on:click=move |_| {
                            tab.set("large".into());
                            selected.set(HashSet::new());
                        }
                    >
                        <Icon name="file" size=15 />
                        "Archivos grandes"
                    </button>
                    <button
                        type="button"
                        role="tab"
                        class="tab"
                        class:active=move || tab.get() == "dups"
                        aria-selected=move || (tab.get() == "dups").to_string()
                        on:click=move |_| {
                            tab.set("dups".into());
                            selected.set(HashSet::new());
                        }
                    >
                        <Icon name="copy" size=15 />
                        "Duplicados"
                    </button>
                </div>
                <div class="toolbar">
                    <span class="eyebrow">"Mínimo"</span>
                    {MIN_MB_OPTIONS
                        .iter()
                        .map(|&mb| {
                            view! {
                                <Chip selected=Signal::derive(move || min_mb.get() == mb) on_click=move |_| min_mb.set(mb)>
                                    {if mb >= 1000 { format!("{} GB", mb / 1000) } else { format!("{mb} MB") }}
                                </Chip>
                            }
                        })
                        .collect_view()}
                    <Pill
                        variant="primary"
                        icon="search"
                        busy=Signal::derive(move || large_loading.get() || dups_loading.get())
                        on_click=move |_| scan()
                    >
                        "Buscar"
                    </Pill>
                </div>
            </header>
            <p class="scope-line">
                "Buscando en "
                <span class="mono">{move || current().map(|p| fmt::short_path(&p)).unwrap_or_else(|| "~ (carpeta de inicio)".into())}</span>
            </p>
            {move || scan_err.get().map(|e| view! { <ErrorNote message=e /> })}
            {move || {
                if tab.get() == "large" {
                    view! { <LargeFilesView large=large loading=large_loading selected=selected /> }.into_any()
                } else {
                    view! { <DuplicatesView dups=dups loading=dups_loading selected=selected /> }.into_any()
                }
            }}
        </section>

        <Show when=move || selected.with(|s| !s.is_empty())>
            <div class="selection-bar">
                <span class="selection-info">
                    <span class="num">{move || selected.with(|s| s.len())}</span>
                    " seleccionados · "
                    <span class="num accent">{move || fmt::bytes(selected_total.get())}</span>
                </span>
                <Pill variant="ghost" on_click=move |_| selected.set(HashSet::new())>"Quitar selección"</Pill>
                <Pill variant="danger" icon="trash" busy=previewing on_click=review>"Revisar y eliminar"</Pill>
            </div>
        </Show>

        <CleanupReview preview=preview on_done=on_done />
    }
}

pub fn report_toast(app: AppState, r: &DeleteReport) {
    let body = if r.errors.is_empty() {
        Some(format!("{} eliminados.", fmt::plural(r.deleted, "elemento", "elementos")))
    } else {
        Some(format!(
            "{} eliminados · {} con errores: {}",
            fmt::plural(r.deleted, "elemento", "elementos"),
            r.errors.len(),
            r.errors.first().cloned().unwrap_or_default()
        ))
    };
    let kind = if r.errors.is_empty() { ToastKind::Success } else { ToastKind::Error };
    app.toast(kind, format!("Se liberaron {}", fmt::bytes(r.freed_bytes)), body);
}

fn toggle_in(set: RwSignal<HashSet<String>>, path: String, on: bool) {
    set.update(|s| {
        if on {
            s.insert(path);
        } else {
            s.remove(&path);
        }
    });
}

#[component]
fn LargeFilesView(
    large: RwSignal<Option<Vec<LargeFile>>>,
    loading: RwSignal<bool>,
    selected: RwSignal<HashSet<String>>,
) -> impl IntoView {
    let app = use_app();
    move || {
        if loading.get() {
            return view! {
                <div class="stack">
                    <Loading label="Buscando archivos grandes… puede tardar unos segundos." />
                    {(0..5).map(|_| view! { <Skeleton height=38 /> }).collect_view()}
                </div>
            }
            .into_any();
        }
        let Some(files) = large.get() else {
            return view! {
                <EmptyState icon="search" title="Aún no hay resultados" text="Elige un tamaño mínimo y pulsa Buscar." />
            }
            .into_any();
        };
        if files.is_empty() {
            return view! {
                <EmptyState icon="check" title="No hay archivos sobre ese tamaño" text="Prueba con un mínimo menor." />
            }
            .into_any();
        }
        view! {
            <div class="table-wrap">
                <table class="table">
                    <thead>
                        <tr>
                            <th class="col-check" />
                            <th>"Nombre"</th>
                            <th class="num-col">"Tamaño"</th>
                            <th class="hide-narrow">"Modificado"</th>
                            <th class="col-actions" />
                        </tr>
                    </thead>
                    <tbody>
                        {files
                            .into_iter()
                            .map(|f| {
                                let p1 = f.path.clone();
                                let p2 = f.path.clone();
                                let p3 = f.path.clone();
                                let checked = Signal::derive(move || selected.with(|s| s.contains(&p1)));
                                view! {
                                    <tr class:selected=checked>
                                        <td class="col-check">
                                            <Check checked=checked label="Seleccionar" on_toggle=move |on: bool| toggle_in(selected, p2.clone(), on) />
                                        </td>
                                        <td>
                                            <div class="file-cell">
                                                <span class="truncate">{fmt::file_name(&f.path)}</span>
                                                <span class="file-path truncate">{fmt::parent_dir(&f.path)}</span>
                                            </div>
                                        </td>
                                        <td class="num-col num">{fmt::bytes(f.size_bytes)}</td>
                                        <td class="hide-narrow muted">{fmt::days_ago(f.modified_days_ago)}</td>
                                        <td class="col-actions">
                                            <button type="button" class="icon-btn subtle" title="Mostrar en Finder" aria-label="Mostrar en Finder" on:click=move |_| reveal(app, p3.clone())>
                                                <Icon name="external" size=14 />
                                            </button>
                                        </td>
                                    </tr>
                                }
                            })
                            .collect_view()}
                    </tbody>
                </table>
            </div>
        }
        .into_any()
    }
}

#[component]
fn DuplicatesView(
    dups: RwSignal<Option<Vec<DuplicateGroup>>>,
    loading: RwSignal<bool>,
    selected: RwSignal<HashSet<String>>,
) -> impl IntoView {
    let app = use_app();
    move || {
        if loading.get() {
            return view! {
                <div class="stack">
                    <Loading label="Comparando contenido de archivos… puede tardar." />
                    {(0..4).map(|_| view! { <Skeleton height=64 /> }).collect_view()}
                </div>
            }
            .into_any();
        }
        let Some(groups) = dups.get() else {
            return view! {
                <EmptyState icon="copy" title="Busca archivos duplicados" text="Se comparan por contenido, no por nombre." />
            }
            .into_any();
        };
        if groups.is_empty() {
            return view! { <EmptyState icon="check" title="Sin duplicados" text="No se encontraron copias sobre ese tamaño." /> }
                .into_any();
        }
        let recoverable: u64 = groups.iter().map(|g| g.size_bytes * (g.paths.len() as u64 - 1)).sum();
        let all_copies: Vec<String> = groups.iter().flat_map(|g| g.paths.iter().skip(1).cloned()).collect();
        view! {
            <div class="dups-summary">
                <span>
                    {fmt::plural(groups.len(), "grupo", "grupos")}
                    " · "
                    <span class="num accent">{fmt::bytes(recoverable)}</span>
                    " recuperables"
                </span>
                <button
                    type="button"
                    class="link-btn"
                    on:click=move |_| selected.set(all_copies.iter().cloned().collect())
                >
                    "Marcar todas las copias (conserva la primera)"
                </button>
            </div>
            <div class="dup-groups">
                {groups
                    .into_iter()
                    .map(|g| {
                        let n = g.paths.len();
                        view! {
                            <div class="dup-group">
                                <div class="dup-head">
                                    <Badge tone="neutral".to_string()>{format!("{n} copias")}</Badge>
                                    <span class="num">{fmt::bytes(g.size_bytes)} " c/u"</span>
                                    <span class="dup-hash mono">{g.hash.chars().take(10).collect::<String>()}</span>
                                </div>
                                <ul class="dup-list">
                                    {g.paths
                                        .into_iter()
                                        .enumerate()
                                        .map(|(i, p)| {
                                            let p1 = p.clone();
                                            let p2 = p.clone();
                                            let p3 = p.clone();
                                            let checked = Signal::derive(move || selected.with(|s| s.contains(&p1)));
                                            view! {
                                                <li class="dup-row" class:selected=checked>
                                                    <Check checked=checked label="Seleccionar" on_toggle=move |on: bool| toggle_in(selected, p2.clone(), on) />
                                                    <div class="file-cell">
                                                        <span class="truncate">{fmt::file_name(&p)}</span>
                                                        <span class="file-path truncate">{fmt::parent_dir(&p)}</span>
                                                    </div>
                                                    {(i == 0).then(|| view! { <Badge tone="accent".to_string()>"Primera"</Badge> })}
                                                    <button type="button" class="icon-btn subtle" title="Mostrar en Finder" aria-label="Mostrar en Finder" on:click=move |_| reveal(app, p3.clone())>
                                                        <Icon name="external" size=14 />
                                                    </button>
                                                </li>
                                            }
                                        })
                                        .collect_view()}
                                </ul>
                            </div>
                        }
                    })
                    .collect_view()}
            </div>
        }
        .into_any()
    }
}
