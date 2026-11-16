//! Nube: every synced drive (iCloud, Google Drive, Dropbox, OneDrive…) with
//! how much is on this Mac and how much is uploaded, a file explorer with
//! per-item sync state, live transfers and backups into any drive.

use std::collections::HashMap;

use iclear_protocol::*;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::PageIntro;
use crate::components::*;
use crate::state::{use_app, AppState, ToastKind};
use crate::{api, fmt};

/// Arrow and percentage that best summarise a transfer right now.
pub fn headline_pct(t: &Transfer) -> (&'static str, f32) {
    match (t.kind, t.phase) {
        (_, TransferPhase::Uploading) => ("↑", t.upload_pct),
        (TransferKind::Download, _) | (_, TransferPhase::Downloading) => ("↓", t.download_pct),
        (TransferKind::Restore, _) => ("↓", t.copy_pct),
        _ => ("↑", t.copy_pct),
    }
}

fn now_s() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

fn in_secs(ts: u64) -> String {
    let now = now_s();
    if ts <= now {
        return "en cualquier momento".into();
    }
    match ts - now {
        0..=59 => "en menos de 1 min".into(),
        s @ 60..=3599 => format!("en {} min", s / 60),
        s @ 3600..=86_399 => format!("en {} h", s / 3600),
        s => format!("en {} d", s / 86_400),
    }
}

fn kind_label(k: CloudKind) -> &'static str {
    match k {
        CloudKind::ICloud => "Apple",
        CloudKind::GoogleDrive => "Google",
        CloudKind::OneDrive => "Microsoft",
        CloudKind::Dropbox => "Dropbox",
        CloudKind::Box => "Box",
        CloudKind::ProtonDrive => "Proton",
        CloudKind::Device => "Dispositivo",
        CloudKind::Other => "Proveedor",
    }
}

fn state_badge(s: CloudItemState) -> (&'static str, &'static str) {
    match s {
        CloudItemState::Synced => ("accent", "En este Mac"),
        CloudItemState::CloudOnly => ("neutral", "Solo en la nube"),
        CloudItemState::Partial => ("neutral", "Parcial"),
        CloudItemState::Downloading => ("accent", "Descargando"),
        CloudItemState::Uploading => ("accent", "Subiendo"),
        CloudItemState::PendingUpload => ("warn", "Pendiente de subir"),
        CloudItemState::Conflict => ("danger", "Conflicto"),
        CloudItemState::Unknown => ("neutral", "Local"),
    }
}

fn phase_label(t: &Transfer) -> &'static str {
    match t.phase {
        TransferPhase::Preparing => "Preparando",
        TransferPhase::Copying if t.kind == TransferKind::Restore => "Restaurando",
        TransferPhase::Copying => "Copiando",
        TransferPhase::Downloading => "Descargando",
        TransferPhase::Uploading => "Subiendo",
        TransferPhase::Done => "Listo",
        TransferPhase::Failed => "Error",
        TransferPhase::Cancelled => "Cancelado",
    }
}

fn reveal(app: AppState, path: String) {
    spawn_local(async move {
        if let Err(e) = api::reveal_in_finder(path).await {
            app.toast(ToastKind::Error, "No se pudo abrir Finder", Some(e));
        }
    });
}

#[component]
pub fn CloudPage() -> impl IntoView {
    let app = use_app();
    let providers = RwSignal::new(None::<Vec<CloudProvider>>);
    let providers_err = RwSignal::new(None::<String>);
    let stats = RwSignal::new(HashMap::<String, CloudStats>::new());
    let selected = RwSignal::new(None::<String>);
    let backups = RwSignal::new(None::<Vec<BackupJob>>);
    let editing = RwSignal::new(None::<BackupDraft>);

    let load_stats = move |id: String, rescan: bool| {
        spawn_local(async move {
            let r = if rescan { api::rescan_cloud(id.clone()).await } else { api::cloud_stats(id.clone()).await };
            match r {
                Ok(s) => stats.update(|m| {
                    m.insert(id, s);
                }),
                Err(e) => app.toast(ToastKind::Error, "No se pudo analizar la nube", Some(e)),
            }
        });
    };

    let load_providers = move || {
        spawn_local(async move {
            match api::list_cloud_providers().await {
                Ok(list) => {
                    for p in list.iter().filter(|p| p.accessible) {
                        load_stats(p.id.clone(), false);
                    }
                    if selected.get_untracked().is_none() {
                        selected.set(list.iter().find(|p| p.accessible).map(|p| p.id.clone()));
                    }
                    providers.set(Some(list));
                }
                Err(e) => providers_err.set(Some(e)),
            }
        });
    };
    let load_backups = move || {
        spawn_local(async move {
            match api::list_backups().await {
                Ok(b) => backups.set(Some(b)),
                Err(e) => app.toast(ToastKind::Error, "No se pudieron leer los backups", Some(e)),
            }
        });
    };
    load_providers();
    load_backups();

    // While a drive is being scanned its figures grow: refresh them with the
    // live metrics tick. Also refresh backups and stats when transfers end.
    Effect::new(move |prev: Option<usize>| {
        let finished = app.transfers.with(|ts| ts.iter().filter(|t| t.phase.is_finished()).count());
        let scanning: Vec<String> =
            stats.with_untracked(|m| m.values().filter(|s| s.scanning).map(|s| s.provider_id.clone()).collect());
        for id in scanning {
            load_stats(id, false);
        }
        if prev.is_some_and(|p| p != finished) {
            load_backups();
        }
        finished
    });

    let new_backup = move |_| {
        let provider_id = selected
            .get_untracked()
            .or_else(|| providers.with_untracked(|p| p.as_ref().and_then(|p| p.first().map(|p| p.id.clone()))))
            .unwrap_or_default();
        editing.set(Some(BackupDraft {
            id: None,
            name: "Documentos".into(),
            source: "~/Documents".into(),
            provider_id,
            dest_folder: None,
            mode: BackupMode::Snapshot,
            schedule: BackupSchedule::Daily,
            keep_versions: 7,
            excludes: vec![".git".into(), "node_modules".into()],
            enabled: true,
        }))
    };

    let totals = Memo::new(move |_| {
        stats.with(|m| {
            let total: u64 = m.values().map(|s| s.total_bytes).sum();
            let local: u64 = m.values().map(|s| s.downloaded_bytes).sum();
            let pending: usize = m.values().map(|s| s.pending_upload_count).sum();
            let pending_bytes: u64 = m.values().map(|s| s.pending_upload_bytes).sum();
            (total, local, pending, pending_bytes)
        })
    });

    view! {
        <PageIntro text="Todas tus nubes en un lugar: cuánto ocupa cada una, qué está en este Mac y qué falta por subir. Descarga, libera espacio local y programa backups en la nube que prefieras.">
            <Pill icon="refresh" on_click=move |_| {
                stats.with_untracked(|m| m.keys().cloned().collect::<Vec<_>>()).into_iter().for_each(|id| load_stats(id, true));
            }>"Reanalizar"</Pill>
            <Pill variant="primary" icon="archive" on_click=new_backup>"Nuevo backup"</Pill>
        </PageIntro>

        <div class="grid grid-4 tight cloud-summary">
            <StatTile label="Nubes" icon="cloud" value=Signal::derive(move || providers.with(|p| p.as_ref().map(|p| p.len().to_string()).unwrap_or_else(|| "—".into())))
                sub=Signal::derive(move || providers.with(|p| p.as_ref().map(|p| p.iter().map(|p| kind_label(p.kind)).collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>().join(" · ")).unwrap_or_default())) />
            <StatTile label="En la nube" icon="layers" value=Signal::derive(move || fmt::bytes(totals.get().0)) sub="Suma de todas las cuentas" />
            <StatTile label="En este Mac" icon="download" value=Signal::derive(move || fmt::bytes(totals.get().1))
                sub=Signal::derive(move || { let (t, l, _, _) = totals.get(); if t == 0 { String::new() } else { format!("{} descargado", fmt::progress((l as f64 / t as f64 * 100.0) as f32)) } }) />
            <StatTile label="Por subir" icon="upload" value=Signal::derive(move || fmt::thousands(totals.get().2 as u64))
                tone=Signal::derive(move || if totals.get().2 > 0 { "warn".to_string() } else { String::new() })
                sub=Signal::derive(move || { let (_, _, n, b) = totals.get(); if n == 0 { "Todo subido".into() } else { format!("{} pendientes", fmt::bytes(b)) } }) />
        </div>

        {move || providers_err.get().map(|e| view! { <ErrorNote message=e /> })}
        {move || match providers.get() {
            None => view! { <div class="grid grid-3">{(0..3).map(|_| view! { <Skeleton height=190 /> }).collect_view()}</div> }.into_any(),
            Some(list) if list.is_empty() => view! {
                <section class="card">
                    <EmptyState icon="cloud" title="No hay nubes sincronizadas" text="Instala Google Drive, Dropbox, OneDrive o activa iCloud Drive: aparecerán aquí automáticamente." />
                </section>
            }.into_any(),
            Some(list) => view! {
                <div class="grid grid-3 cloud-grid">
                    {list.into_iter().map(|p| view! { <ProviderCard provider=p stats=stats selected=selected on_rescan=Callback::new(move |id| load_stats(id, true)) /> }).collect_view()}
                </div>
            }.into_any(),
        }}

        <TransfersCard />

        {move || {
            let id = selected.get()?;
            let prov = providers.with(|p| p.as_ref().and_then(|p| p.iter().find(|p| p.id == id).cloned()))?;
            Some(view! { <Explorer provider=prov on_changed=Callback::new(move |id| load_stats(id, false)) /> })
        }}

        <BackupsCard backups=backups providers=providers editing=editing on_changed=Callback::new(move |_| load_backups()) />

        <BackupForm editing=editing providers=providers on_saved=Callback::new(move |_| load_backups()) />
    }
}

// ---------------------------------------------------------------------------
// Provider card
// ---------------------------------------------------------------------------

#[component]
fn ProviderCard(
    provider: CloudProvider,
    stats: RwSignal<HashMap<String, CloudStats>>,
    selected: RwSignal<Option<String>>,
    on_rescan: Callback<String>,
) -> impl IntoView {
    let app = use_app();
    let id = StoredValue::new(provider.id.clone());
    let root = StoredValue::new(provider.root.clone());
    let s = Memo::new(move |_| stats.with(|m| m.get(&id.get_value()).cloned()));
    let is_selected = move || selected.get().as_deref() == Some(id.get_value().as_str());
    let accessible = provider.accessible;

    let status = move || {
        let Some(s) = s.get() else {
            return if accessible {
                view! { <Badge>"Analizando…"</Badge> }.into_any()
            } else {
                view! { <Badge tone="warn".to_string()>"Sin acceso"</Badge> }.into_any()
            };
        };
        if s.scanning {
            view! { <Badge tone="accent".to_string() dot=true>{format!("Analizando · {}", fmt::thousands(s.file_count as u64))}</Badge> }.into_any()
        } else if s.conflict_count > 0 {
            view! { <Badge tone="danger".to_string() dot=true>{fmt::plural(s.conflict_count, "conflicto", "conflictos")}</Badge> }.into_any()
        } else if s.uploading_count > 0 {
            view! { <Badge tone="accent".to_string() dot=true>{format!("Subiendo {}", fmt::thousands(s.uploading_count as u64))}</Badge> }.into_any()
        } else if s.pending_upload_count > 0 {
            view! { <Badge tone="warn".to_string() dot=true>{format!("{} por subir", fmt::thousands(s.pending_upload_count as u64))}</Badge> }.into_any()
        } else {
            view! { <Badge tone="accent".to_string()>"Al día"</Badge> }.into_any()
        }
    };

    let dl = Signal::derive(move || s.with(|s| s.as_ref().map(|s| s.download_pct as f64).unwrap_or(0.0)));
    let up = Signal::derive(move || s.with(|s| s.as_ref().map(|s| s.upload_pct as f64).unwrap_or(0.0)));

    view! {
        <section class="card cloud-card" class:selected=is_selected class:dimmed=!accessible>
            <header class="card-head">
                <span class="card-icon"><Icon name="cloud" size=16 /></span>
                <div class="card-head-text">
                    <h3 class="card-title">{provider.name.clone()}</h3>
                    <p class="card-sub truncate" title=provider.account.clone().unwrap_or_default()>
                        {provider.account.clone().unwrap_or_else(|| kind_label(provider.kind).to_string())}
                    </p>
                </div>
                {status}
            </header>
            <div class="cloud-figure">
                <span class="num">{move || s.with(|s| s.as_ref().map(|s| fmt::bytes(s.total_bytes)).unwrap_or_else(|| "—".into()))}</span>
                <span class="muted">
                    {move || s.with(|s| s.as_ref().map(|s| format!("{} · {}", fmt::plural(s.file_count, "archivo", "archivos"), if s.complete || s.scanning { "" } else { "conteo parcial" })).unwrap_or_default().trim_end_matches(" · ").to_string())}
                </span>
            </div>
            <div class="cloud-meters">
                <div class="cloud-meter">
                    <div class="cloud-meter-head">
                        <span><Icon name="download" size=13 />"En este Mac"</span>
                        <span class="num">{move || fmt::progress(dl.get() as f32)}</span>
                    </div>
                    <Meter value=dl />
                    <span class="cloud-meter-sub num">
                        {move || s.with(|s| s.as_ref().map(|s| format!("{} local · {} solo en la nube", fmt::bytes(s.downloaded_bytes), fmt::bytes(s.cloud_only_bytes))).unwrap_or_default())}
                    </span>
                </div>
                <div class="cloud-meter">
                    <div class="cloud-meter-head">
                        <span><Icon name="upload" size=13 />"Subido"</span>
                        <span class="num">{move || fmt::progress(up.get() as f32)}</span>
                    </div>
                    <Meter value=up tone=Signal::derive(move || if up.get() < 99.95 { "warn".to_string() } else { "accent".to_string() }) />
                    <span class="cloud-meter-sub num">
                        {move || s.with(|s| s.as_ref().map(|s| if s.pending_upload_count == 0 { "Todo confirmado por el proveedor".to_string() } else { format!("{} pendientes ({})", fmt::plural(s.pending_upload_count, "archivo", "archivos"), fmt::bytes(s.pending_upload_bytes)) }).unwrap_or_default())}
                    </span>
                </div>
            </div>
            <footer class="cloud-card-foot">
                <Pill variant="sm" icon="folder" disabled=!accessible on_click=move |_| selected.set(Some(id.get_value()))>"Explorar"</Pill>
                <span class="grow" />
                <button type="button" class="icon-btn subtle" title="Reanalizar" aria-label="Reanalizar" disabled=!accessible
                    on:click=move |_| on_rescan.run(id.get_value())>
                    <Icon name="refresh" size=14 />
                </button>
                <button type="button" class="icon-btn subtle" title="Mostrar en Finder" aria-label="Mostrar en Finder"
                    on:click=move |_| reveal(app, root.get_value())>
                    <Icon name="external" size=14 />
                </button>
            </footer>
        </section>
    }
}

// ---------------------------------------------------------------------------
// Transfers
// ---------------------------------------------------------------------------

#[component]
fn TransfersCard() -> impl IntoView {
    let app = use_app();
    let has_any = move || app.transfers.with(|t| !t.is_empty());
    let active = move || app.transfers.with(|t| t.iter().filter(|t| !t.phase.is_finished()).count());
    let clear = move |_| {
        spawn_local(async move {
            let _ = api::clear_transfers().await;
            app.poll_transfers();
        });
    };
    view! {
        <Show when=has_any>
            <section class="card">
                <header class="card-head">
                    <span class="card-icon"><Icon name="network" size=16 /></span>
                    <div class="card-head-text">
                        <h3 class="card-title">"Transferencias"</h3>
                        <p class="card-sub">{move || match active() {
                            0 => "Sin transferencias en curso".to_string(),
                            n => format!("{} en curso", n),
                        }}</p>
                    </div>
                    <Pill variant="ghost sm" on_click=clear>"Limpiar terminadas"</Pill>
                </header>
                <ul class="transfer-list">
                    <For
                        each=move || app.transfers.get()
                        key=|t| (t.id, t.phase as u8, (t.copy_pct * 10.0) as u32, (t.upload_pct * 10.0) as u32, (t.download_pct * 10.0) as u32, t.bytes_per_sec)
                        children=move |t| view! { <TransferRow t=t /> }
                    />
                </ul>
            </section>
        </Show>
    }
}

#[component]
fn TransferRow(t: Transfer) -> impl IntoView {
    let app = use_app();
    let id = t.id;
    let finished = t.phase.is_finished();
    let icon = match t.kind {
        TransferKind::Download => "download",
        TransferKind::Backup => "archive",
        TransferKind::Restore => "history",
    };
    let tone = match t.phase {
        TransferPhase::Done => "accent",
        TransferPhase::Failed => "danger",
        TransferPhase::Cancelled => "neutral",
        _ => "accent",
    };
    let bar = |label: &'static str, icon: &'static str, v: f32, tone: &'static str| {
        view! {
            <div class="transfer-bar">
                <span class="transfer-bar-label"><Icon name=icon size=12 />{label}</span>
                <Meter value=Signal::derive(move || v as f64) tone=tone.to_string() />
                <span class="num">{fmt::progress(v)}</span>
            </div>
        }
    };
    let bars = match t.kind {
        TransferKind::Download => vec![bar("Bajada", "download", t.download_pct, "accent")],
        TransferKind::Backup => vec![bar("Copia", "copy", t.copy_pct, "accent"), bar("Subida", "upload", t.upload_pct, if t.upload_pct < 100.0 && finished { "warn" } else { "accent" })],
        TransferKind::Restore => vec![bar("Bajada", "download", t.download_pct, "accent"), bar("Copia", "copy", t.copy_pct, "accent")],
    };
    let detail = if t.bytes_total > 0 || t.files_total > 0 {
        format!(
            "{} de {} · {}{}",
            fmt::bytes(t.bytes_done),
            fmt::bytes(t.bytes_total),
            fmt::plural(t.files_total, "archivo", "archivos"),
            if t.bytes_per_sec > 0 { format!(" · {}", fmt::rate(t.bytes_per_sec)) } else { String::new() }
        )
    } else {
        String::new()
    };
    let dest = t.dest.clone();
    let phase = phase_label(&t);
    let cancel = move |_| {
        spawn_local(async move {
            if let Err(e) = api::cancel_transfer(id).await {
                app.toast(ToastKind::Error, "No se pudo cancelar", Some(e));
            }
            app.poll_transfers();
        });
    };
    view! {
        <li class="transfer-row" class:finished=finished>
            <span class="transfer-icon"><Icon name=icon size=16 /></span>
            <div class="transfer-main">
                <div class="transfer-head">
                    <span class="transfer-label truncate">{t.label.clone()}</span>
                    <Badge tone=tone.to_string() dot=!finished>{phase}</Badge>
                </div>
                <div class="transfer-bars">{bars}</div>
                <p class="transfer-detail num">{detail}</p>
                {t.message.clone().map(|m| view! { <p class="transfer-msg">{m}</p> })}
            </div>
            <div class="transfer-actions">
                {(!finished).then(|| view! {
                    <button type="button" class="icon-btn subtle" title="Cancelar" aria-label="Cancelar" on:click=cancel>
                        <Icon name="x" size=14 />
                    </button>
                })}
                <button type="button" class="icon-btn subtle" title="Mostrar en Finder" aria-label="Mostrar en Finder"
                    on:click=move |_| reveal(app, dest.clone())>
                    <Icon name="external" size=14 />
                </button>
            </div>
        </li>
    }
}

// ---------------------------------------------------------------------------
// Explorer
// ---------------------------------------------------------------------------

#[component]
fn Explorer(provider: CloudProvider, on_changed: Callback<String>) -> impl IntoView {
    let app = use_app();
    let pid = StoredValue::new(provider.id.clone());
    let root = StoredValue::new(provider.root.clone());
    let path = RwSignal::new(None::<String>);
    let listing = RwSignal::new(None::<CloudListing>);
    let loading = RwSignal::new(false);
    let err = RwSignal::new(None::<String>);
    let filter = RwSignal::new(String::new());
    let free_target = RwSignal::new(None::<CloudItem>);
    let freeing = RwSignal::new(false);

    let load = move || {
        let p = path.get_untracked();
        loading.set(true);
        err.set(None);
        spawn_local(async move {
            match api::list_cloud_folder(pid.get_value(), p).await {
                Ok(l) => listing.set(Some(l)),
                Err(e) => err.set(Some(e)),
            }
            loading.set(false);
        });
    };
    load();

    let download = move |item: CloudItem| {
        spawn_local(async move {
            match api::cloud_download(item.path.clone()).await {
                Ok(_) => {
                    app.toast(ToastKind::Info, format!("Descargando {}", item.name), Some("Sigue el avance en Transferencias.".into()));
                    app.poll_transfers();
                }
                Err(e) => app.toast(ToastKind::Error, "No se pudo descargar", Some(e)),
            }
        });
    };
    let confirm_free = move |_| {
        let Some(item) = free_target.get_untracked() else { return };
        freeing.set(true);
        spawn_local(async move {
            match api::cloud_free_up(item.path.clone()).await {
                Ok(r) => {
                    let mut body = format!("{} ahora están solo en la nube.", fmt::plural(r.files, "archivo", "archivos"));
                    if r.skipped_not_uploaded > 0 {
                        body.push_str(&format!(" {} sin subir se mantuvieron.", fmt::plural(r.skipped_not_uploaded, "archivo", "archivos")));
                    }
                    if let Some(e) = r.errors.first() {
                        body.push_str(&format!(" Error: {e}"));
                    }
                    let kind = if r.errors.is_empty() { ToastKind::Success } else { ToastKind::Error };
                    app.toast(kind, format!("Se liberaron {}", fmt::bytes(r.bytes)), Some(body));
                    on_changed.run(pid.get_value());
                    load();
                }
                Err(e) => app.toast(ToastKind::Error, "No se pudo liberar espacio", Some(e)),
            }
            freeing.set(false);
            free_target.set(None);
        });
    };

    let crumbs = move || {
        let r = root.get_value();
        let p = path.get().unwrap_or_else(|| r.clone());
        let rel = p.strip_prefix(&r).unwrap_or("").trim_start_matches('/').to_string();
        let mut acc = r.clone();
        rel.split('/')
            .filter(|s| !s.is_empty())
            .map(|seg| {
                acc = format!("{acc}/{seg}");
                (seg.to_string(), acc.clone())
            })
            .collect::<Vec<_>>()
    };

    view! {
        <section class="card">
            <header class="card-head wrap">
                <span class="card-icon"><Icon name="folder" size=16 /></span>
                <nav class="crumbs" aria-label="Ruta">
                    <button type="button" class="crumb" class:current=move || path.with(|p| p.is_none())
                        on:click=move |_| { path.set(None); load(); }>
                        <Icon name="cloud" size=14 />
                        {provider.name.clone()}
                        {provider.account.clone().map(|a| view! { <span class="muted">{format!(" · {a}")}</span> })}
                    </button>
                    {move || {
                        let c = crumbs();
                        let n = c.len();
                        c.into_iter().enumerate().map(|(i, (name, full))| view! {
                            <Icon name="chevron-right" size=12 class="crumb-sep" />
                            <button type="button" class="crumb" class:current=i + 1 == n
                                on:click=move |_| { path.set(Some(full.clone())); load(); }>
                                {name}
                            </button>
                        }).collect_view()
                    }}
                </nav>
                <div class="toolbar">
                    <SearchField value=filter placeholder="Filtrar en esta carpeta" />
                    <button type="button" class="icon-btn" title="Actualizar" aria-label="Actualizar" on:click=move |_| load()>
                        <Icon name="refresh" size=15 />
                    </button>
                </div>
            </header>
            {move || err.get().map(|e| view! { <ErrorNote message=e /> })}
            {move || {
                if loading.get() && listing.with(|l| l.is_none()) {
                    return view! { <div class="stack">{(0..6).map(|_| view! { <Skeleton height=40 /> }).collect_view()}</div> }.into_any();
                }
                let Some(l) = listing.get() else { return ().into_any() };
                let q = filter.get().to_lowercase();
                let items: Vec<CloudItem> = l.items.into_iter().filter(|i| q.is_empty() || i.name.to_lowercase().contains(&q)).collect();
                if items.is_empty() {
                    return view! { <EmptyState icon="folder" title=if q.is_empty() { "Carpeta vacía" } else { "Sin coincidencias" } /> }.into_any();
                }
                view! {
                    <div class="cloud-list" class:is-loading=move || loading.get()>
                        <div class="cloud-row cloud-row-head">
                            <span />
                            <span>"Nombre"</span>
                            <span>"Estado"</span>
                            <span class="num-col">"Tamaño"</span>
                            <span>"Bajada"</span>
                            <span>"Subida"</span>
                            <span />
                        </div>
                        {items.into_iter().map(|it| {
                            let (tone, label) = state_badge(it.state);
                            let is_dir = it.is_dir;
                            let open_path = it.path.clone();
                            let reveal_path = it.path.clone();
                            let can_download = it.download_pct < 99.95 || it.state == CloudItemState::CloudOnly;
                            let can_free = it.downloaded_bytes > 0 && it.upload_pct >= 99.95 && it.state != CloudItemState::Unknown;
                            let it_dl = it.clone();
                            let it_free = it.clone();
                            let sub = if is_dir {
                                format!("{}{}", fmt::plural(it.file_count, "archivo", "archivos"), if it.partial { " (parcial)" } else { "" })
                            } else {
                                it.modified.map(|m| fmt::ago_secs(now_s().saturating_sub(m))).unwrap_or_default()
                            };
                            let dl = it.download_pct as f64;
                            let up = it.upload_pct as f64;
                            view! {
                                <div class="cloud-row" class:is-dir=is_dir>
                                    <span class="cloud-row-icon"><Icon name=if is_dir { "folder" } else { "file" } size=15 /></span>
                                    <button type="button" class="cloud-name" disabled=!is_dir
                                        on:click=move |_| if is_dir { path.set(Some(open_path.clone())); load(); }>
                                        <span class="truncate">{it.name.clone()}</span>
                                        <span class="cloud-name-sub">{sub}</span>
                                    </button>
                                    <span><Badge tone=tone.to_string()>{label}</Badge></span>
                                    <span class="num num-col">{if it.size_bytes == 0 && it.state == CloudItemState::CloudOnly && is_dir { "—".to_string() } else { format!("{}{}", fmt::bytes(it.size_bytes), if it.partial { "+" } else { "" }) }}</span>
                                    <span class="cloud-pct"><Meter value=Signal::derive(move || dl) /><span class="num">{fmt::progress(dl as f32)}</span></span>
                                    <span class="cloud-pct"><Meter value=Signal::derive(move || up) tone=if up < 99.95 { "warn".to_string() } else { "accent".to_string() } /><span class="num">{fmt::progress(up as f32)}</span></span>
                                    <span class="row-actions">
                                        {can_download.then(|| view! {
                                            <button type="button" class="icon-btn subtle" title="Descargar a este Mac" aria-label="Descargar a este Mac"
                                                on:click=move |_| download(it_dl.clone())>
                                                <Icon name="download" size=14 />
                                            </button>
                                        })}
                                        {can_free.then(|| view! {
                                            <button type="button" class="icon-btn subtle" title="Liberar espacio (dejar solo en la nube)" aria-label="Liberar espacio"
                                                on:click=move |_| free_target.set(Some(it_free.clone()))>
                                                <Icon name="cloud" size=14 />
                                            </button>
                                        })}
                                        <button type="button" class="icon-btn subtle" title="Mostrar en Finder" aria-label="Mostrar en Finder"
                                            on:click=move |_| reveal(app, reveal_path.clone())>
                                            <Icon name="external" size=14 />
                                        </button>
                                    </span>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                    <p class="footnote">"El estado de subida se consulta para archivos modificados en los últimos 14 días; los anteriores ya fueron confirmados por el proveedor."</p>
                }.into_any()
            }}
        </section>

        <ConfirmDialog
            open=Signal::derive(move || free_target.with(|t| t.is_some()))
            title=Signal::derive(move || free_target.with(|t| t.as_ref().map(|t| format!("¿Liberar espacio de «{}»?", t.name)).unwrap_or_default()))
            confirm_label="Liberar espacio"
            busy=freeing
            on_confirm=confirm_free
            on_cancel=move |_| free_target.set(None)
        >
            <p class="dialog-text">
                {move || free_target.with(|t| t.as_ref().map(|t| format!("Se quitará la copia local de {} ({}). Los archivos siguen en la nube y se descargan solos cuando los abras.", if t.is_dir { "la carpeta" } else { "el archivo" }, fmt::bytes(t.downloaded_bytes))).unwrap_or_default())}
            </p>
            <p class="dialog-text muted">"Los archivos con cambios que aún no se suben se mantienen en este Mac. No se borra nada de la nube."</p>
        </ConfirmDialog>
    }
}

// ---------------------------------------------------------------------------
// Backups
// ---------------------------------------------------------------------------

fn mode_label(m: BackupMode) -> &'static str {
    match m {
        BackupMode::Mirror => "Espejo",
        BackupMode::Snapshot => "Versiones",
    }
}

fn schedule_label(s: BackupSchedule) -> &'static str {
    match s {
        BackupSchedule::Manual => "Manual",
        BackupSchedule::Hourly => "Cada hora",
        BackupSchedule::Daily => "Diario",
        BackupSchedule::Weekly => "Semanal",
    }
}

#[component]
fn BackupsCard(
    backups: RwSignal<Option<Vec<BackupJob>>>,
    providers: RwSignal<Option<Vec<CloudProvider>>>,
    editing: RwSignal<Option<BackupDraft>>,
    on_changed: Callback<()>,
) -> impl IntoView {
    let app = use_app();
    let to_delete = RwSignal::new(None::<BackupJob>);
    let deleting = RwSignal::new(false);
    let confirm_delete = move |_| {
        let Some(job) = to_delete.get_untracked() else { return };
        deleting.set(true);
        spawn_local(async move {
            match api::delete_backup(job.id.clone()).await {
                Ok(()) => app.toast(ToastKind::Success, format!("Backup «{}» eliminado", job.name), Some("Las copias que ya estaban en la nube se conservan.".into())),
                Err(e) => app.toast(ToastKind::Error, "No se pudo eliminar", Some(e)),
            }
            deleting.set(false);
            to_delete.set(None);
            on_changed.run(());
        });
    };
    let provider_name = move |id: &str| {
        providers.with_untracked(|p| {
            p.as_ref()
                .and_then(|p| p.iter().find(|p| p.id == id))
                .map(|p| match &p.account {
                    Some(a) => format!("{} · {a}", p.name),
                    None => p.name.clone(),
                })
                .unwrap_or_else(|| id.to_string())
        })
    };

    view! {
        <section class="card">
            <header class="card-head">
                <span class="card-icon"><Icon name="archive" size=16 /></span>
                <div class="card-head-text">
                    <h3 class="card-title">"Backups en la nube"</h3>
                    <p class="card-sub">"Copia carpetas de este Mac a cualquiera de tus nubes, a mano o con horario. Los programados corren mientras iclear está abierto."</p>
                </div>
            </header>
            {move || match backups.get() {
                None => view! { <div class="stack"><Skeleton height=64 /><Skeleton height=64 /></div> }.into_any(),
                Some(list) if list.is_empty() => view! {
                    <EmptyState icon="archive" title="Aún no tienes backups" text="Crea uno con «Nuevo backup»: elige una carpeta, la nube de destino y cada cuánto copiar." />
                }.into_any(),
                Some(list) => view! {
                    <ul class="backup-list">
                        {list.into_iter().map(|job| {
                            let running = Memo::new({
                                let id = job.id.clone();
                                move |_| app.transfers.with(|ts| ts.iter().find(|t| t.backup_id.as_deref() == Some(id.as_str()) && !t.phase.is_finished()).cloned())
                            });
                            let show_versions = RwSignal::new(false);
                            let id_run = job.id.clone();
                            let run = move |_| {
                                let id = id_run.clone();
                                spawn_local(async move {
                                    match api::run_backup(id).await {
                                        Ok(_) => app.poll_transfers(),
                                        Err(e) => app.toast(ToastKind::Error, "No se pudo iniciar el backup", Some(e)),
                                    }
                                });
                            };
                            let job_edit = job.clone();
                            let edit = move |_| {
                                let j = job_edit.clone();
                                let folder = providers.with_untracked(|p| {
                                    p.as_ref().and_then(|p| p.iter().find(|p| p.id == j.provider_id)).and_then(|p| p.writable_root.clone())
                                }).and_then(|root| {
                                    let parent = j.dest.rsplit_once('/').map(|(p, _)| p.to_string())?;
                                    parent.strip_prefix(&root).map(|s| s.trim_start_matches('/').to_string())
                                });
                                editing.set(Some(BackupDraft {
                                    id: Some(j.id.clone()),
                                    name: j.name.clone(),
                                    source: j.source.clone(),
                                    provider_id: j.provider_id.clone(),
                                    dest_folder: folder,
                                    mode: j.mode,
                                    schedule: j.schedule,
                                    keep_versions: j.keep_versions,
                                    excludes: j.excludes.clone(),
                                    enabled: j.enabled,
                                }));
                            };
                            let job_del = job.clone();
                            let last = match (job.last_run, job.last_ok) {
                                (None, _) => view! { <span class="muted">"Nunca ejecutado"</span> }.into_any(),
                                (Some(t), ok) => {
                                    let (tone, txt) = if ok == Some(true) { ("accent", "Correcto") } else { ("danger", "Con errores") };
                                    view! {
                                        <span class="backup-last">
                                            <Badge tone=tone.to_string() dot=true>{txt}</Badge>
                                            <span class="muted">{fmt::ago_secs(now_s().saturating_sub(t))}</span>
                                        </span>
                                    }.into_any()
                                }
                            };
                            let job_id = StoredValue::new(job.id.clone());
                            view! {
                                <li class="backup-row">
                                    <div class="backup-main">
                                        <div class="backup-title">
                                            <span class="backup-name">{job.name.clone()}</span>
                                            <Badge>{mode_label(job.mode)}</Badge>
                                            <Badge>{schedule_label(job.schedule)}</Badge>
                                            {(!job.enabled).then(|| view! { <Badge tone="warn".to_string()>"Pausado"</Badge> })}
                                        </div>
                                        <p class="backup-route">
                                            <span class="mono">{fmt::short_path(&job.source)}</span>
                                            <Icon name="chevron-right" size=12 />
                                            <Icon name="cloud" size=13 />
                                            <span>{provider_name(&job.provider_id)}</span>
                                        </p>
                                        <p class="backup-meta">
                                            {last}
                                            {job.last_message.clone().map(|m| { let title = m.clone(); view! { <span class="muted truncate" title=title>{m}</span> } })}
                                            {job.next_run.map(|n| view! { <span class="muted">{format!("Próximo {}", in_secs(n))}</span> })}
                                        </p>
                                        {move || running.get().map(|t| view! {
                                            <div class="backup-progress">
                                                <div class="transfer-bar">
                                                    <span class="transfer-bar-label"><Icon name="copy" size=12 />"Copia"</span>
                                                    <Meter value=Signal::derive(move || t.copy_pct as f64) />
                                                    <span class="num">{fmt::progress(t.copy_pct)}</span>
                                                </div>
                                                <div class="transfer-bar">
                                                    <span class="transfer-bar-label"><Icon name="upload" size=12 />"Subida"</span>
                                                    <Meter value=Signal::derive(move || t.upload_pct as f64) />
                                                    <span class="num">{fmt::progress(t.upload_pct)}</span>
                                                </div>
                                            </div>
                                        })}
                                    </div>
                                    <div class="backup-actions">
                                        <Pill variant="sm primary" icon="play" busy=Signal::derive(move || running.with(|r| r.is_some())) on_click=run>
                                            {move || if running.with(|r| r.is_some()) { "En curso" } else { "Ejecutar" }}
                                        </Pill>
                                        <Pill variant="sm ghost" icon="history" on_click=move |_| show_versions.update(|v| *v = !*v)>"Versiones"</Pill>
                                        <button type="button" class="icon-btn subtle" title="Editar" aria-label="Editar" on:click=edit>
                                            <Icon name="edit" size=14 />
                                        </button>
                                        <button type="button" class="icon-btn subtle" title="Eliminar backup" aria-label="Eliminar backup"
                                            on:click=move |_| to_delete.set(Some(job_del.clone()))>
                                            <Icon name="trash" size=14 />
                                        </button>
                                    </div>
                                    <Show when=move || show_versions.get()>
                                        <Versions id=job_id.get_value() />
                                    </Show>
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                }.into_any(),
            }}
        </section>

        <ConfirmDialog
            open=Signal::derive(move || to_delete.with(|t| t.is_some()))
            title=Signal::derive(move || to_delete.with(|t| t.as_ref().map(|t| format!("¿Eliminar el backup «{}»?", t.name)).unwrap_or_default()))
            confirm_label="Eliminar backup"
            danger=true
            busy=deleting
            on_confirm=confirm_delete
            on_cancel=move |_| to_delete.set(None)
        >
            <p class="dialog-text">"iclear dejará de ejecutarlo. Las copias que ya están en la nube no se borran: puedes eliminarlas desde la nube si ya no las necesitas."</p>
        </ConfirmDialog>
    }
}

#[component]
fn Versions(id: String) -> impl IntoView {
    let app = use_app();
    let id = StoredValue::new(id);
    let versions = RwSignal::new(None::<Result<Vec<BackupVersion>, String>>);
    let to_restore = RwSignal::new(None::<BackupVersion>);
    let restoring = RwSignal::new(false);
    spawn_local(async move {
        versions.set(Some(api::backup_versions(id.get_value()).await));
    });
    let confirm = move |_| {
        let Some(v) = to_restore.get_untracked() else { return };
        restoring.set(true);
        spawn_local(async move {
            match api::restore_backup(id.get_value(), v.path.clone(), None).await {
                Ok(t) => {
                    app.toast(ToastKind::Info, "Restaurando…", Some(format!("Se copiará en {}", fmt::short_path(&t.dest))));
                    app.poll_transfers();
                }
                Err(e) => app.toast(ToastKind::Error, "No se pudo restaurar", Some(e)),
            }
            restoring.set(false);
            to_restore.set(None);
        });
    };
    view! {
        <div class="backup-versions">
            {move || match versions.get() {
                None => view! { <Loading label="Leyendo versiones…" /> }.into_any(),
                Some(Err(e)) => view! { <ErrorNote message=e /> }.into_any(),
                Some(Ok(v)) if v.is_empty() => view! { <p class="muted">"Todavía no hay copias en la nube. Ejecuta el backup para crear la primera."</p> }.into_any(),
                Some(Ok(v)) => v.into_iter().map(|ver| {
                    let ver_r = ver.clone();
                    let path = ver.path.clone();
                    view! {
                        <div class="version-row">
                            <Icon name="history" size=14 />
                            <span class="version-label">{ver.label.clone()}</span>
                            <span class="muted num">{format!("{} · {}", fmt::bytes(ver.size_bytes), fmt::plural(ver.file_count, "archivo", "archivos"))}</span>
                            {(ver.cloud_only_bytes > 0).then(|| view! { <Badge>{format!("{} solo en la nube", fmt::bytes(ver.cloud_only_bytes))}</Badge> })}
                            <span class="version-actions">
                                <Pill variant="sm ghost" icon="external" on_click=move |_| reveal(app, path.clone())>"Finder"</Pill>
                                <Pill variant="sm" icon="download" on_click=move |_| to_restore.set(Some(ver_r.clone()))>"Restaurar"</Pill>
                            </span>
                        </div>
                    }
                }).collect_view().into_any(),
            }}
        </div>
        <ConfirmDialog
            open=Signal::derive(move || to_restore.with(|t| t.is_some()))
            title="Restaurar esta versión"
            confirm_label="Restaurar"
            busy=restoring
            on_confirm=confirm
            on_cancel=move |_| to_restore.set(None)
        >
            <p class="dialog-text">
                {move || to_restore.with(|v| v.as_ref().map(|v| format!("Se copiarán {} ({}) a una carpeta nueva en Descargas. No se reemplaza ningún archivo existente.", fmt::plural(v.file_count, "archivo", "archivos"), fmt::bytes(v.size_bytes))).unwrap_or_default())}
            </p>
            {move || to_restore.with(|v| v.as_ref().filter(|v| v.cloud_only_bytes > 0).map(|v| view! {
                <p class="dialog-text muted">{format!("{} están solo en la nube y se descargarán primero.", fmt::bytes(v.cloud_only_bytes))}</p>
            }))}
        </ConfirmDialog>
    }
}

// ---------------------------------------------------------------------------
// Create / edit form
// ---------------------------------------------------------------------------

const SOURCE_PRESETS: &[(&str, &str)] = &[
    ("Documentos", "~/Documents"),
    ("Escritorio", "~/Desktop"),
    ("Imágenes", "~/Pictures"),
    ("Descargas", "~/Downloads"),
    ("Desarrollo", "~/Developer"),
];

#[component]
fn BackupForm(
    editing: RwSignal<Option<BackupDraft>>,
    providers: RwSignal<Option<Vec<CloudProvider>>>,
    on_saved: Callback<()>,
) -> impl IntoView {
    let app = use_app();
    let saving = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let excludes_text = RwSignal::new(String::new());

    // Sync the excludes text box with the draft being edited.
    Effect::new(move |_| {
        if let Some(d) = editing.get() {
            if excludes_text.get_untracked().is_empty() || d.id.is_some() {
                excludes_text.set(d.excludes.join(", "));
            }
        } else {
            excludes_text.set(String::new());
            error.set(None);
        }
    });

    let set = move |f: &dyn Fn(&mut BackupDraft)| editing.update(|d| {
        if let Some(d) = d.as_mut() {
            f(d)
        }
    });
    let field = move |get: fn(&BackupDraft) -> String| move || editing.with(|d| d.as_ref().map(get).unwrap_or_default());

    let save = move |_| {
        let Some(mut d) = editing.get_untracked() else { return };
        d.excludes = excludes_text.get_untracked().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        saving.set(true);
        error.set(None);
        spawn_local(async move {
            let is_new = d.id.is_none();
            match api::save_backup(d).await {
                Ok(job) => {
                    app.toast(
                        ToastKind::Success,
                        if is_new { format!("Backup «{}» creado", job.name) } else { format!("Backup «{}» actualizado", job.name) },
                        Some(format!("Destino: {}", fmt::short_path(&job.dest))),
                    );
                    editing.set(None);
                    on_saved.run(());
                }
                Err(e) => error.set(Some(e)),
            }
            saving.set(false);
        });
    };

    let mode = Signal::derive(move || editing.with(|d| match d.as_ref().map(|d| d.mode) {
        Some(BackupMode::Mirror) => "mirror".to_string(),
        _ => "snapshot".to_string(),
    }));
    let schedule = Signal::derive(move || editing.with(|d| match d.as_ref().map(|d| d.schedule) {
        Some(BackupSchedule::Manual) => "manual".to_string(),
        Some(BackupSchedule::Hourly) => "hourly".to_string(),
        Some(BackupSchedule::Weekly) => "weekly".to_string(),
        _ => "daily".to_string(),
    }));
    let keep = Signal::derive(move || editing.with(|d| d.as_ref().map(|d| d.keep_versions as f64).unwrap_or(7.0)));
    let dest_preview = move || {
        editing.with(|d| {
            let d = d.as_ref()?;
            let root = providers.with(|p| p.as_ref().and_then(|p| p.iter().find(|p| p.id == d.provider_id)).and_then(|p| p.writable_root.clone()))?;
            let folder = d.dest_folder.clone().filter(|f| !f.trim().is_empty()).unwrap_or_else(|| "iclear Backups".into());
            Some(fmt::short_path(&format!("{root}/{folder}/{}", d.name.trim())))
        })
    };

    view! {
        <ConfirmDialog
            open=Signal::derive(move || editing.with(|d| d.is_some()))
            title=Signal::derive(move || if editing.with(|d| d.as_ref().is_some_and(|d| d.id.is_some())) { "Editar backup".to_string() } else { "Nuevo backup en la nube".to_string() })
            confirm_label=Signal::derive(move || if editing.with(|d| d.as_ref().is_some_and(|d| d.id.is_some())) { "Guardar".to_string() } else { "Crear backup".to_string() })
            busy=saving
            class="dialog-wide"
            on_confirm=save
            on_cancel=move |_| editing.set(None)
        >
            <div class="form">
                <label class="form-field">
                    <span class="form-label">"Nombre"</span>
                    <input class="text-input" type="text" maxlength="80"
                        prop:value=field(|d| d.name.clone())
                        on:input=move |ev| { let v = event_target_value(&ev); set(&|d| d.name = v.clone()) } />
                </label>

                <div class="form-field">
                    <span class="form-label">"Carpeta a respaldar"</span>
                    <input class="text-input mono" type="text" placeholder="~/Documents"
                        prop:value=field(|d| d.source.clone())
                        on:input=move |ev| { let v = event_target_value(&ev); set(&|d| d.source = v.clone()) } />
                    <div class="chips">
                        {SOURCE_PRESETS.iter().map(|&(label, path)| view! {
                            <Chip selected=Signal::derive(move || editing.with(|d| d.as_ref().is_some_and(|d| d.source == path)))
                                on_click=move |_| set(&|d| {
                                    let was_preset_name = SOURCE_PRESETS.iter().any(|(l, _)| *l == d.name) || d.name.trim().is_empty();
                                    d.source = path.to_string();
                                    if was_preset_name { d.name = label.to_string(); }
                                })>
                                {label}
                            </Chip>
                        }).collect_view()}
                    </div>
                </div>

                <div class="form-field">
                    <span class="form-label">"Nube de destino"</span>
                    <div class="chips">
                        {move || providers.get().unwrap_or_default().into_iter().filter(|p| p.accessible && p.writable_root.is_some()).map(|p| {
                            let id = p.id.clone();
                            let id_sel = p.id.clone();
                            view! {
                                <Chip selected=Signal::derive(move || editing.with(|d| d.as_ref().is_some_and(|d| d.provider_id == id_sel)))
                                    on_click=move |_| { let id = id.clone(); set(&|d| d.provider_id = id.clone()) }>
                                    <Icon name="cloud" size=13 />
                                    {match &p.account { Some(a) => format!("{} · {a}", p.name), None => p.name.clone() }}
                                </Chip>
                            }
                        }).collect_view()}
                    </div>
                </div>

                <label class="form-field">
                    <span class="form-label">"Carpeta en la nube"</span>
                    <input class="text-input" type="text" placeholder="iclear Backups"
                        prop:value=field(|d| d.dest_folder.clone().unwrap_or_default())
                        on:input=move |ev| { let v = event_target_value(&ev); set(&|d| d.dest_folder = (!v.trim().is_empty()).then(|| v.clone())) } />
                    <span class="form-hint">{move || dest_preview().map(|p| format!("Se guardará en {p}")).unwrap_or_default()}</span>
                </label>

                <div class="form-row">
                    <div class="form-field">
                        <span class="form-label">"Tipo"</span>
                        <Segmented options=vec![("snapshot", "Versiones"), ("mirror", "Espejo")] selected=mode
                            on_select=move |v: String| set(&|d| d.mode = if v == "mirror" { BackupMode::Mirror } else { BackupMode::Snapshot }) />
                        <span class="form-hint">{move || if mode.get() == "mirror" {
                            "Una sola copia que se actualiza: solo sube lo nuevo o modificado."
                        } else {
                            "Una copia completa con fecha en cada ejecución."
                        }}</span>
                    </div>
                    <div class="form-field">
                        <span class="form-label">"Frecuencia"</span>
                        <Segmented options=vec![("manual", "Manual"), ("hourly", "Cada hora"), ("daily", "Diario"), ("weekly", "Semanal")] selected=schedule
                            on_select=move |v: String| set(&|d| d.schedule = match v.as_str() {
                                "manual" => BackupSchedule::Manual,
                                "hourly" => BackupSchedule::Hourly,
                                "weekly" => BackupSchedule::Weekly,
                                _ => BackupSchedule::Daily,
                            }) />
                    </div>
                </div>

                <Show when=move || mode.get() == "snapshot">
                    <div class="form-field">
                        <span class="form-label">{move || format!("Conservar las últimas {} versiones", keep.get() as u32)}</span>
                        <Slider value=keep min=1.0 max=30.0 label="Versiones a conservar"
                            on_input=move |v: f64| set(&|d| d.keep_versions = v as u32) />
                        <span class="form-hint">"Las versiones más antiguas de este backup se eliminan de la nube automáticamente."</span>
                    </div>
                </Show>

                <label class="form-field">
                    <span class="form-label">"Excluir (nombres separados por coma)"</span>
                    <input class="text-input mono" type="text" placeholder=".git, node_modules"
                        prop:value=move || excludes_text.get()
                        on:input=move |ev| excludes_text.set(event_target_value(&ev)) />
                </label>

                <div class="form-inline">
                    <Toggle checked=Signal::derive(move || editing.with(|d| d.as_ref().is_some_and(|d| d.enabled))) label="Activo"
                        on_toggle=move |v: bool| set(&|d| d.enabled = v) />
                    <span>"Ejecutar según la frecuencia"</span>
                </div>

                {move || error.get().map(|e| view! { <ErrorNote message=e /> })}
            </div>
        </ConfirmDialog>
    }
}
