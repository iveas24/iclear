use leptos::prelude::*;

use super::Icon;

/// Dark surface with hairline border and optional header.
#[component]
pub fn Card(
    #[prop(optional, into)] title: Option<String>,
    #[prop(optional, into)] subtitle: Option<String>,
    #[prop(optional, into)] icon: Option<String>,
    #[prop(optional, into)] class: String,
    children: Children,
) -> impl IntoView {
    let head = title.map(|t| {
        view! {
            <header class="card-head">
                {icon.map(|i| view! { <span class="card-icon"><Icon name=i size=16 /></span> })}
                <div class="card-head-text">
                    <h3 class="card-title">{t}</h3>
                    {subtitle.map(|s| view! { <p class="card-sub">{s}</p> })}
                </div>
            </header>
        }
    });
    view! {
        <section class=format!("card {class}")>
            {head}
            {children()}
        </section>
    }
}

/// Compact metric: small label, big mono value, optional unit and sub-line.
#[component]
pub fn StatTile(
    #[prop(into)] label: String,
    #[prop(into)] value: Signal<String>,
    #[prop(optional, into)] unit: Option<String>,
    #[prop(into, default = String::new().into())] sub: Signal<String>,
    #[prop(optional, into)] icon: Option<String>,
    #[prop(into, default = String::new().into())] tone: Signal<String>,
) -> impl IntoView {
    view! {
        <div class=move || format!("stat-tile tone-{}", tone.get())>
            <div class="stat-label">
                {icon.map(|i| view! { <Icon name=i size=14 /> })}
                <span>{label}</span>
            </div>
            <div class="stat-value">
                <span class="num">{move || value.get()}</span>
                {unit.map(|u| view! { <span class="stat-unit">{u}</span> })}
            </div>
            <Show when=move || !sub.get().is_empty()>
                <div class="stat-sub">{move || sub.get()}</div>
            </Show>
        </div>
    }
}

/// Label/value row used in info lists.
#[component]
pub fn InfoRow(#[prop(into)] label: String, #[prop(into)] value: Signal<String>) -> impl IntoView {
    view! {
        <div class="info-row">
            <span class="info-label">{label}</span>
            <span class="info-value">{move || value.get()}</span>
        </div>
    }
}
