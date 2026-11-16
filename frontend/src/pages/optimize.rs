use iclear_protocol::{LaunchAgent, OptimizeAction};
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::PageIntro;
use crate::components::*;
use crate::state::{use_app, ToastKind};
use crate::{api, fmt};

struct ActionDef {
    action: OptimizeAction,
    icon: &'static str,
    title: &'static str,
    text: &'static str,
    confirm: Option<&'static str>,
}

const ACTIONS: &[ActionDef] = &[
    ActionDef {
        action: OptimizeAction::PurgeMemory,
        icon: "memory",
        title: "Liberar RAM",
        text: "Libera la memoria inactiva y en caché. Las apps pueden tardar un poco más en responder la primera vez.",
        confirm: None,
    },
    ActionDef {
        action: OptimizeAction::FlushDns,
        icon: "globe",
        title: "Vaciar caché DNS",
        text: "Útil cuando un sitio no carga tras cambiar de red o de servidor.",
        confirm: None,
    },
    ActionDef {
        action: OptimizeAction::ThinLocalSnapshots,
        icon: "layers",
        title: "Reducir snapshots locales de Time Machine",
        text: "Elimina instantáneas locales para recuperar espacio. Los respaldos en tu disco externo no se tocan.",
        confirm: Some("Se eliminarán las instantáneas locales de Time Machine guardadas en este Mac. Los respaldos del disco externo no se ven afectados."),
    },
    ActionDef {
        action: OptimizeAction::ReindexSpotlight,
        icon: "search",
        title: "Reindexar Spotlight",
        text: "Reconstruye el índice de búsqueda. El equipo puede ir más lento durante un rato.",
        confirm: Some("Spotlight borrará su índice y lo reconstruirá en segundo plano. Las búsquedas estarán incompletas por un rato."),
    },
    ActionDef {
        action: OptimizeAction::CleanSystemCaches,
        icon: "trash",
        title: "Limpiar cachés del sistema",
        text: "Borra las cachés de /Library/Caches. macOS las regenera cuando las necesita.",
        confirm: Some("Se borrarán las cachés del sistema en /Library/Caches. Es seguro, pero algunas apps tardarán más en abrir la próxima vez."),
    },
];

#[component]
pub fn OptimizePage() -> impl IntoView {
    let app = use_app();
    let running = RwSignal::new(None::<usize>);
    let results = RwSignal::new(vec![None::<(bool, String)>; ACTIONS.len()]);
    let pending = RwSignal::new(None::<usize>);

    let run = move |i: usize| {
        pending.set(None);
        running.set(Some(i));
        let def = &ACTIONS[i];
        let action = def.action;
        let title = def.title;
        spawn_local(async move {
            let res = api::run_optimization(action).await;
            match &res {
                Ok(msg) => app.toast(ToastKind::Success, title, Some(msg.clone())),
                Err(e) => app.toast(ToastKind::Error, format!("{title}: falló"), Some(e.clone())),
            }
            results.update(|r| r[i] = Some(match res {
                Ok(m) => (true, m),
                Err(e) => (false, e),
            }));
            running.set(None);
        });
    };

    view! {
        <PageIntro text="Acciones de mantenimiento puntuales. Se ejecutan a través del asistente con privilegios y solo cuando tú lo pides." />
        <Show when=move || !app.helper_ready()>
            <HelperCard feature="La optimización del sistema" />
        </Show>
        <div class="grid grid-3">
            {ACTIONS
                .iter()
                .enumerate()
                .map(|(i, def)| {
                    let busy = Signal::derive(move || running.get() == Some(i));
                    let disabled = Signal::derive(move || !app.helper_ready() || (running.get().is_some() && running.get() != Some(i)));
                    let has_confirm = def.confirm.is_some();
                    view! {
                        <section class="card action-card">
                            <span class="action-icon"><Icon name=def.icon size=20 /></span>
                            <h3 class="card-title">{def.title}</h3>
                            <p class="action-text">{def.text}</p>
                            {move || {
                                results.with(|r| r[i].clone()).map(|(ok, msg)| {
                                    view! {
                                        <p class="action-result" class:error=!ok>
                                            <Icon name=if ok { "check" } else { "warning" } size=13 />
                                            {msg}
                                        </p>
                                    }
                                })
                            }}
                            <div class="action-foot">
                                <Pill
                                    variant="primary sm"
                                    icon="bolt"
                                    busy=busy
                                    disabled=disabled
                                    on_click=move |_| if has_confirm { pending.set(Some(i)) } else { run(i) }
                                >
                                    {move || if busy.get() { "Ejecutando…" } else { "Ejecutar" }}
                                </Pill>
                            </div>
                        </section>
                    }
                })
                .collect_view()}
        </div>

        <ConfirmDialog
            open=Signal::derive(move || pending.get().is_some())
            title=Signal::derive(move || pending.get().map(|i| format!("¿{}?", ACTIONS[i].title)).unwrap_or_default())
            confirm_label="Ejecutar".to_string()
            on_confirm=move |_| {
                if let Some(i) = pending.get_untracked() {
                    run(i)
                }
            }
            on_cancel=move |_| pending.set(None)
        >
            <p class="dialog-text">{move || pending.get().and_then(|i| ACTIONS[i].confirm).unwrap_or_default()}</p>
        </ConfirmDialog>

        <LaunchItems />
    }
}

#[component]
fn LaunchItems() -> impl IntoView {
    let app = use_app();
    let agents = RwSignal::new(None::<Vec<LaunchAgent>>);
    let err = RwSignal::new(None::<String>);
    let busy_path = RwSignal::new(None::<String>);

    let load = move || {
        spawn_local(async move {
            match api::list_launch_agents().await {
                Ok(list) => agents.set(Some(list)),
                Err(e) => err.set(Some(e)),
            }
        });
    };
    load();

    let set_enabled = move |path: String, enabled: bool| {
        busy_path.set(Some(path.clone()));
        spawn_local(async move {
            match api::set_launch_agent_enabled(path.clone(), enabled).await {
                Ok(()) => agents.update(|a| {
                    if let Some(a) = a.as_mut().and_then(|a| a.iter_mut().find(|x| x.path == path)) {
                        a.disabled = !enabled;
                    }
                }),
                Err(e) => app.toast(ToastKind::Error, "No se pudo cambiar el elemento de inicio", Some(e)),
            }
            busy_path.set(None);
        });
    };

    let section = move |scope: &'static str, title: &'static str, sub: &'static str| {
        let items = move || {
            agents.with(|a| a.iter().flatten().filter(|x| x.scope == scope).cloned().collect::<Vec<_>>())
        };
        view! {
            <Show when=move || !items().is_empty()>
                <div class="agent-section">
                    <div class="agent-section-head">
                        <h4>{title}</h4>
                        <span class="muted">{sub}</span>
                    </div>
                    <ul class="agent-list">
                        {move || {
                            items()
                                .into_iter()
                                .map(|a| {
                                    let path = a.path.clone();
                                    let path_busy = a.path.clone();
                                    let enabled = !a.disabled;
                                    let read_only = a.scope == "global-daemon";
                                    view! {
                                        <li class="agent-row">
                                            <span class="status-dot" class:ok=a.loaded title=if a.loaded { "Cargado" } else { "No cargado" } />
                                            <div class="agent-text">
                                                <span class="agent-label truncate">{a.label.clone()}</span>
                                                <span class="agent-program mono truncate">
                                                    {a.program.as_deref().map(fmt::short_path).unwrap_or_else(|| fmt::short_path(&a.path))}
                                                </span>
                                            </div>
                                            {if read_only {
                                                view! { <Badge>"Solo lectura"</Badge> }.into_any()
                                            } else {
                                                view! {
                                                    <Toggle
                                                        checked=enabled
                                                        label=format!("Activar {}", a.label)
                                                        disabled=Signal::derive(move || busy_path.with(|b| b.as_deref() == Some(path_busy.as_str())))
                                                        on_toggle=move |on: bool| set_enabled(path.clone(), on)
                                                    />
                                                }
                                                    .into_any()
                                            }}
                                        </li>
                                    }
                                })
                                .collect_view()
                        }}
                    </ul>
                </div>
            </Show>
        }
    };

    view! {
        <h2 class="section-title">"Elementos de inicio"</h2>
        <section class="card">
            <header class="card-head">
                <span class="card-icon"><Icon name="power" size=16 /></span>
                <div class="card-head-text">
                    <h3 class="card-title">"Agentes y daemons de launchd"</h3>
                    <p class="card-sub">"Desactivar un agente evita que se abra al iniciar sesión. Los daemons del sistema se muestran solo como referencia."</p>
                </div>
            </header>
            {move || err.get().map(|e| view! { <ErrorNote message=e /> })}
            <Show
                when=move || agents.with(|a| a.is_some())
                fallback=|| view! { <div class="stack">{(0..5).map(|_| view! { <Skeleton height=40 /> }).collect_view()}</div> }
            >
                {section("user", "De tu usuario", "~/Library/LaunchAgents")}
                {section("global-agent", "Globales", "/Library/LaunchAgents")}
                {section("global-daemon", "Daemons", "/Library/LaunchDaemons · solo lectura")}
                <Show when=move || agents.with(|a| a.as_ref().is_some_and(|a| a.is_empty()))>
                    <EmptyState icon="power" title="Sin elementos de inicio" />
                </Show>
            </Show>
        </section>
    }
}
