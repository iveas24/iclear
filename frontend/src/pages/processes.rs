use std::time::Duration;

use iclear_protocol::ProcessInfo;
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::PageIntro;
use crate::components::*;
use crate::state::{use_app, ToastKind};
use crate::{api, fmt};

const MAX_ROWS: usize = 250;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Col {
    Name,
    Pid,
    Cpu,
    Mem,
    User,
}

#[component]
pub fn ProcessesPage() -> impl IntoView {
    let app = use_app();
    let procs = RwSignal::new(None::<Vec<ProcessInfo>>);
    let err = RwSignal::new(None::<String>);
    let search = RwSignal::new(String::new());
    let sort = RwSignal::new((Col::Cpu, true));
    let auto = RwSignal::new(true);

    let refresh = move || {
        spawn_local(async move {
            match api::get_processes().await {
                Ok(list) => {
                    procs.set(Some(list));
                    err.set(None);
                }
                Err(e) => err.set(Some(e)),
            }
        });
    };
    refresh();
    if let Ok(handle) = set_interval_with_handle(
        move || {
            if auto.get_untracked() {
                refresh()
            }
        },
        Duration::from_secs(3),
    ) {
        on_cleanup(move || handle.clear());
    }

    let rows = Memo::new(move |_| {
        let q = search.get().to_lowercase();
        let (col, desc) = sort.get();
        let mut list: Vec<ProcessInfo> = procs.with(|p| {
            p.iter()
                .flatten()
                .filter(|p| {
                    q.is_empty()
                        || p.name.to_lowercase().contains(&q)
                        || p.pid.to_string().starts_with(&q)
                        || p.user.as_deref().is_some_and(|u| u.to_lowercase().contains(&q))
                })
                .cloned()
                .collect()
        });
        list.sort_by(|a, b| {
            let o = match col {
                Col::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                Col::Pid => a.pid.cmp(&b.pid),
                Col::Cpu => a.cpu.total_cmp(&b.cpu),
                Col::Mem => a.memory.cmp(&b.memory),
                Col::User => a.user.cmp(&b.user),
            };
            if desc { o.reverse() } else { o }
        });
        list
    });
    let totals = Memo::new(move |_| {
        procs.with(|p| {
            let p = p.as_deref().unwrap_or_default();
            (p.len(), p.iter().map(|x| x.cpu).sum::<f32>(), p.iter().map(|x| x.memory).sum::<u64>())
        })
    });

    // Termination
    let pending = RwSignal::new(None::<(ProcessInfo, bool)>);
    let killing = RwSignal::new(false);
    let confirm_kill = move |_| {
        let Some((p, force)) = pending.get_untracked() else { return };
        killing.set(true);
        spawn_local(async move {
            match api::kill_process(p.pid, force).await {
                Ok(()) => {
                    app.toast(
                        ToastKind::Success,
                        if force { "Proceso forzado a cerrar" } else { "Proceso terminado" },
                        Some(format!("{} (PID {})", p.name, p.pid)),
                    );
                    refresh();
                }
                Err(e) => app.toast(ToastKind::Error, format!("No se pudo terminar {}", p.name), Some(e)),
            }
            killing.set(false);
            pending.set(None);
        });
    };

    view! {
        <PageIntro text="Procesos en ejecución ordenados por consumo. Solo puedes terminar procesos de tu usuario; los del sistema están protegidos.">
            <label class="inline-toggle">
                <Toggle checked=auto label="Actualizar automáticamente" on_toggle=move |v: bool| auto.set(v) />
                <span>"Cada 3 s"</span>
            </label>
            <Pill variant="ghost" icon="refresh" on_click=move |_| refresh()>"Actualizar"</Pill>
        </PageIntro>

        <div class="grid grid-3 tight">
            <StatTile label="Procesos" icon="list" value=Signal::derive(move || fmt::thousands(totals.get().0 as u64)) />
            <StatTile label="CPU total" icon="cpu" value=Signal::derive(move || fmt::dec(totals.get().1 as f64, 1)) unit="%" sub="Suma de núcleos (puede superar 100 %)" />
            <StatTile label="Memoria" icon="memory" value=Signal::derive(move || fmt::mem(totals.get().2)) sub="Memoria residente sumada" />
        </div>

        <section class="card table-card">
            <header class="card-head wrap">
                <SearchField value=search placeholder="Buscar por nombre, PID o usuario" />
                <span class="muted">
                    {move || {
                        let shown = rows.with(|r| r.len());
                        if shown > MAX_ROWS {
                            format!("Mostrando {MAX_ROWS} de {}", fmt::thousands(shown as u64))
                        } else {
                            fmt::plural(shown, "proceso", "procesos")
                        }
                    }}
                </span>
            </header>
            {move || err.get().map(|e| view! { <ErrorNote message=e /> })}
            <Show
                when=move || procs.with(|p| p.is_some())
                fallback=|| view! { <div class="stack">{(0..8).map(|_| view! { <Skeleton height=34 /> }).collect_view()}</div> }
            >
                <div class="table-wrap">
                    <table class="table processes">
                        <thead>
                            <tr>
                                <SortHeader label="Nombre" key=Col::Name sort=sort />
                                <SortHeader label="PID" key=Col::Pid sort=sort numeric=true />
                                <SortHeader label="CPU" key=Col::Cpu sort=sort numeric=true class="col-cpu" />
                                <SortHeader label="Memoria" key=Col::Mem sort=sort numeric=true />
                                <SortHeader label="Usuario" key=Col::User sort=sort class="hide-narrow" />
                                <th class="col-actions-wide"><span class="sr-only">"Acciones"</span></th>
                            </tr>
                        </thead>
                        <tbody>
                            {move || {
                                rows.get()
                                    .into_iter()
                                    .take(MAX_ROWS)
                                    .map(|p| {
                                        let cpu = p.cpu;
                                        let p_term = p.clone();
                                        let p_force = p.clone();
                                        view! {
                                            <tr>
                                                <td class="proc-name truncate">{p.name.clone()}</td>
                                                <td class="num-col num muted">{p.pid}</td>
                                                <td class="num-col col-cpu">
                                                    <div class="cpu-cell">
                                                        <span class="cpu-bar">
                                                            <span class="cpu-fill" class:hot={cpu >= 50.0} style=format!("width:{:.1}%", cpu.clamp(0.0, 100.0)) />
                                                        </span>
                                                        <span class="num">{fmt::dec(cpu as f64, 1)}</span>
                                                    </div>
                                                </td>
                                                <td class="num-col num">{fmt::mem(p.memory)}</td>
                                                <td class="hide-narrow muted">{p.user.clone().unwrap_or_else(|| "—".into())}</td>
                                                <td class="col-actions-wide">
                                                    {if p.killable {
                                                        view! {
                                                            <div class="row-actions">
                                                                <button type="button" class="mini-btn" on:click=move |_| pending.set(Some((p_term.clone(), false)))>
                                                                    "Terminar"
                                                                </button>
                                                                <button type="button" class="mini-btn danger" on:click=move |_| pending.set(Some((p_force.clone(), true)))>
                                                                    "Forzar"
                                                                </button>
                                                            </div>
                                                        }
                                                            .into_any()
                                                    } else {
                                                        view! {
                                                            <span class="protected" title="Proceso del sistema">
                                                                <Icon name="lock" size=13 />
                                                                "Sistema"
                                                            </span>
                                                        }
                                                            .into_any()
                                                    }}
                                                </td>
                                            </tr>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </tbody>
                    </table>
                </div>
                <Show when=move || rows.with(|r| r.is_empty())>
                    <EmptyState icon="search" title="Sin coincidencias" text="Prueba con otro nombre o PID." />
                </Show>
            </Show>
        </section>

        <ConfirmDialog
            open=Signal::derive(move || pending.with(|p| p.is_some()))
            title=Signal::derive(move || {
                pending.with(|p| match p {
                    Some((p, true)) => format!("¿Forzar el cierre de {}?", p.name),
                    Some((p, false)) => format!("¿Terminar {}?", p.name),
                    None => String::new(),
                })
            })
            confirm_label=Signal::derive(move || {
                if pending.with(|p| p.as_ref().is_some_and(|p| p.1)) { "Forzar cierre".to_string() } else { "Terminar".to_string() }
            })
            danger=true
            busy=killing
            on_confirm=confirm_kill
            on_cancel=move |_| pending.set(None)
        >
            <p class="dialog-text">
                {move || {
                    pending.with(|p| match p {
                        Some((p, true)) => format!(
                            "Se enviará SIGKILL al PID {}. El proceso se cerrará de inmediato y perderás cualquier cambio sin guardar.",
                            p.pid
                        ),
                        Some((p, false)) => format!(
                            "Se pedirá al PID {} que se cierre de forma ordenada (SIGTERM). La app puede guardar su estado antes de salir.",
                            p.pid
                        ),
                        None => String::new(),
                    })
                }}
            </p>
        </ConfirmDialog>
    }
}
