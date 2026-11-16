use leptos::prelude::*;

use super::Icon;
use crate::state::{use_app, ToastKind};

#[component]
pub fn ToastHost() -> impl IntoView {
    let app = use_app();
    view! {
        <div class="toasts" aria-live="polite">
            <For each=move || app.toasts.get() key=|t| t.id let:toast>
                {
                    let (tone, icon) = match toast.kind {
                        ToastKind::Success => ("accent", "check"),
                        ToastKind::Error => ("danger", "warning"),
                        ToastKind::Info => ("neutral", "info"),
                    };
                    let id = toast.id;
                    view! {
                        <div class=format!("toast tone-{tone}") role="status">
                            <span class="toast-icon"><Icon name=icon size=16 /></span>
                            <div class="toast-text">
                                <p class="toast-title">{toast.title.clone()}</p>
                                {toast.body.clone().map(|b| view! { <p class="toast-body">{b}</p> })}
                            </div>
                            <button
                                type="button"
                                class="icon-btn"
                                aria-label="Cerrar"
                                on:click=move |_| app.dismiss_toast(id)
                            >
                                <Icon name="x" size=14 />
                            </button>
                        </div>
                    }
                }
            </For>
        </div>
    }
}
