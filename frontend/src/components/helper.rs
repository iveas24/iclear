use leptos::prelude::*;

use super::{Icon, Pill};
use crate::state::use_app;

/// Renders `children` only when the privileged helper is installed and
/// reachable; otherwise a calm card that offers to install it.
#[component]
pub fn HelperRequired(#[prop(into)] feature: String, children: ChildrenFn) -> impl IntoView {
    let app = use_app();
    view! {
        <Show
            when=move || app.helper_ready()
            fallback=move || view! { <HelperCard feature=feature.clone() /> }
        >
            {children()}
        </Show>
    }
}

#[component]
pub fn HelperCard(#[prop(into)] feature: String) -> impl IntoView {
    let app = use_app();
    let detail = move || {
        app.helper.with(|h| match h {
            None => "Comprobando el estado del asistente…".to_string(),
            Some(h) if h.installed && !h.reachable => {
                "El asistente está instalado pero no responde. Reintenta o reinstálalo.".to_string()
            }
            Some(_) => "Se instala una sola vez; macOS te pedirá tu contraseña de administrador."
                .to_string(),
        })
    };
    view! {
        <div class="helper-card">
            <span class="helper-icon"><Icon name="shield" size=20 /></span>
            <div class="helper-text">
                <p class="helper-title">"Se necesita el asistente de iclear"</p>
                <p class="helper-body">
                    {feature}
                    " requiere un pequeño servicio con privilegios que solo acepta comandos acotados y validados. "
                    {detail}
                </p>
            </div>
            <div class="helper-actions">
                <Pill variant="ghost" icon="refresh" on_click=move |_| app.refresh_helper()>
                    "Reintentar"
                </Pill>
                <Pill
                    variant="primary"
                    icon="shield"
                    busy=app.helper_busy
                    on_click=move |_| app.install_helper()
                >
                    "Instalar asistente"
                </Pill>
            </div>
        </div>
    }
}

/// Calm note shown when the hardware/OS does not allow a feature.
#[component]
pub fn UnsupportedNote(
    #[prop(into)] message: String,
    #[prop(optional, into)] action_label: Option<String>,
    #[prop(optional, into)] on_action: Option<Callback<()>>,
) -> impl IntoView {
    view! {
        <div class="note tone-neutral unsupported">
            <Icon name="info" size=16 />
            <span class="note-text">{message}</span>
            {action_label
                .zip(on_action)
                .map(|(label, cb)| {
                    view! {
                        <Pill variant="sm" icon="external" on_click=move |_| cb.run(())>
                            {label}
                        </Pill>
                    }
                })}
        </div>
    }
}
