use std::collections::HashSet;

use iclear_protocol::{CleanupPreview, DeleteReport};
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::{Check, EmptyState, Icon, Pill};
use crate::{api, fmt, state::use_app, state::ToastKind};

/// Modal confirmation dialog. Closes on Escape or backdrop click.
#[component]
pub fn ConfirmDialog(
    #[prop(into)] open: Signal<bool>,
    #[prop(into)] title: Signal<String>,
    #[prop(into, default = "Confirmar".to_string().into())] confirm_label: Signal<String>,
    #[prop(default = false)] danger: bool,
    #[prop(into, default = false.into())] busy: Signal<bool>,
    #[prop(into, default = false.into())] confirm_disabled: Signal<bool>,
    #[prop(into, default = String::new())] class: String,
    #[prop(into)] on_confirm: Callback<()>,
    #[prop(into)] on_cancel: Callback<()>,
    children: ChildrenFn,
) -> impl IntoView {
    let handle = window_event_listener(leptos::ev::keydown, move |ev| {
        if ev.key() == "Escape" && open.get_untracked() && !busy.get_untracked() {
            on_cancel.run(());
        }
    });
    on_cleanup(move || handle.remove());

    let variant = if danger { "danger" } else { "primary" };
    view! {
        <Show when=move || open.get()>
            <div
                class="overlay"
                on:click=move |_| {
                    if !busy.get_untracked() {
                        on_cancel.run(())
                    }
                }
            >
                <div
                    class=format!("dialog {class}")
                    role="dialog"
                    aria-modal="true"
                    on:click=|ev| ev.stop_propagation()
                >
                    <header class="dialog-head">
                        <h2>{move || title.get()}</h2>
                        <button
                            type="button"
                            class="icon-btn"
                            aria-label="Cerrar"
                            on:click=move |_| on_cancel.run(())
                        >
                            <Icon name="x" size=16 />
                        </button>
                    </header>
                    <div class="dialog-body">{children()}</div>
                    <footer class="dialog-foot">
                        <Pill variant="ghost" disabled=busy on_click=move |_| on_cancel.run(())>
                            "Cancelar"
                        </Pill>
                        <Pill
                            variant=variant
                            busy=busy
                            disabled=confirm_disabled
                            on_click=move |_| on_confirm.run(())
                        >
                            {move || confirm_label.get()}
                        </Pill>
                    </footer>
                </div>
            </div>
        </Show>
    }
}

/// Review dialog for a `CleanupPreview`: every item can be deselected, then
/// `execute_cleanup` runs with the remaining paths. Shared by Limpieza and
/// Almacenamiento.
#[component]
pub fn CleanupReview(
    preview: RwSignal<Option<CleanupPreview>>,
    #[prop(into)] on_done: Callback<DeleteReport>,
    #[prop(into, default = "Revisar y confirmar".to_string().into())] title: Signal<String>,
) -> impl IntoView {
    let app = use_app();
    let excluded = RwSignal::new(HashSet::<String>::new());
    let busy = RwSignal::new(false);

    // Fresh selection every time a new preview arrives.
    Effect::new(move |_| {
        preview.track();
        excluded.set(HashSet::new());
    });

    let selected = Memo::new(move |_| {
        preview.with(|p| {
            excluded.with(|ex| {
                p.as_ref()
                    .map(|p| {
                        p.items
                            .iter()
                            .filter(|i| !ex.contains(&i.path))
                            .map(|i| (i.path.clone(), i.size_bytes))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
        })
    });
    let total = Memo::new(move |_| selected.with(|s| s.iter().map(|x| x.1).sum::<u64>()));
    let count = Memo::new(move |_| selected.with(|s| s.len()));
    let item_total = move || preview.with(|p| p.as_ref().map(|p| p.items.len()).unwrap_or(0));

    let confirm = move |_| {
        let Some(token) = preview.with_untracked(|p| p.as_ref().map(|p| p.token.clone())) else {
            return;
        };
        let paths: Vec<String> = selected.get_untracked().into_iter().map(|x| x.0).collect();
        busy.set(true);
        spawn_local(async move {
            match api::execute_cleanup(token, paths).await {
                Ok(report) => {
                    preview.set(None);
                    on_done.run(report);
                }
                Err(e) => app.toast(ToastKind::Error, "No se pudo completar el borrado", Some(e)),
            }
            busy.set(false);
        });
    };

    let rows = move || {
        preview.with(|p| {
            p.as_ref()
                .map(|p| p.items.clone())
                .unwrap_or_default()
                .into_iter()
                .map(|item| {
                    let path = item.path.clone();
                    let path_check = path.clone();
                    let path_toggle = path.clone();
                    let checked = Signal::derive(move || excluded.with(|ex| !ex.contains(&path_check)));
                    view! {
                        <li class="review-item" class:off=move || !checked.get()>
                            <Check
                                checked=checked
                                label=format!("Incluir {}", fmt::file_name(&path))
                                on_toggle=move |on: bool| {
                                    excluded.update(|ex| {
                                        if on {
                                            ex.remove(&path_toggle);
                                        } else {
                                            ex.insert(path_toggle.clone());
                                        }
                                    })
                                }
                            />
                            <span class="review-kind">
                                <Icon name=if item.is_dir { "folder" } else { "file" } size=15 />
                            </span>
                            <div class="review-name">
                                <span class="truncate">{fmt::file_name(&item.path)}</span>
                                <span class="review-path truncate">{fmt::parent_dir(&item.path)}</span>
                            </div>
                            <span class="review-age">{fmt::days_ago(item.modified_days_ago)}</span>
                            <span class="review-size num">{fmt::bytes(item.size_bytes)}</span>
                        </li>
                    }
                })
                .collect_view()
        })
    };

    view! {
        <ConfirmDialog
            open=Signal::derive(move || preview.with(|p| p.is_some()))
            title=title
            confirm_label=Signal::derive(move || format!("Borrar {}", fmt::bytes(total.get())))
            danger=true
            busy=busy
            confirm_disabled=Signal::derive(move || count.get() == 0)
            class="dialog-wide"
            on_confirm=confirm
            on_cancel=move |_| preview.set(None)
        >
            <div class="review-summary">
                <div>
                    <span class="eyebrow">"Se liberarán"</span>
                    <span class="review-total num">{move || fmt::bytes(total.get())}</span>
                </div>
                <div class="review-count">
                    <span class="num">{move || count.get()}</span>
                    " de "
                    <span class="num">{item_total}</span>
                    " elementos"
                </div>
            </div>
            <div class="note tone-danger">
                <Icon name="warning" size=16 />
                <span>
                    <strong>"Borrado permanente — no se puede deshacer."</strong>
                    " Los elementos no pasan por la Papelera."
                </span>
            </div>
            <div class="review-tools">
                <button type="button" class="link-btn" on:click=move |_| excluded.set(HashSet::new())>
                    "Seleccionar todo"
                </button>
                <button
                    type="button"
                    class="link-btn"
                    on:click=move |_| {
                        let all = preview
                            .with_untracked(|p| {
                                p.as_ref()
                                    .map(|p| p.items.iter().map(|i| i.path.clone()).collect())
                                    .unwrap_or_default()
                            });
                        excluded.set(all)
                    }
                >
                    "Ninguno"
                </button>
            </div>
            <Show
                when=move || item_total() != 0
                fallback=|| {
                    view! {
                        <EmptyState
                            icon="check"
                            title="Nada que borrar"
                            text="La selección no contiene elementos."
                        />
                    }
                }
            >
                <ul class="review-list">{rows}</ul>
            </Show>
        </ConfirmDialog>
    }
}
