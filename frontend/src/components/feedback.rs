use leptos::prelude::*;

use super::Icon;

#[component]
pub fn Spinner() -> impl IntoView {
    view! { <span class="spinner" aria-hidden="true" /> }
}

/// Inline loading line for slow scans.
#[component]
pub fn Loading(#[prop(into)] label: String) -> impl IntoView {
    view! {
        <div class="loading" role="status">
            <Spinner />
            <span>{label}</span>
        </div>
    }
}

/// Shimmering placeholder block.
#[component]
pub fn Skeleton(
    #[prop(default = 16)] height: u32,
    #[prop(into, default = "100%".to_string())] width: String,
) -> impl IntoView {
    view! { <div class="skeleton" style=format!("height:{height}px;width:{width}") /> }
}

#[component]
pub fn EmptyState(
    #[prop(into, default = "info".to_string())] icon: String,
    #[prop(into)] title: String,
    #[prop(into, default = String::new())] text: String,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    view! {
        <div class="empty">
            <span class="empty-icon"><Icon name=icon size=20 /></span>
            <p class="empty-title">{title}</p>
            {(!text.is_empty()).then(|| view! { <p class="empty-text">{text}</p> })}
            {children.map(|c| c())}
        </div>
    }
}

/// Error callout with retry slot.
#[component]
pub fn ErrorNote(#[prop(into)] message: String) -> impl IntoView {
    view! {
        <div class="note tone-danger" role="alert">
            <Icon name="warning" size=16 />
            <span>{message}</span>
        </div>
    }
}

/// Hover/focus tooltip around any inline content. Works on disabled
/// buttons too (the wrapper receives the hover, not the button).
#[component]
pub fn Tip(
    #[prop(into)] text: String,
    #[prop(into, default = "top".to_string())] side: String,
    children: Children,
) -> impl IntoView {
    view! {
        <span class=format!("tip tip-{side}") tabindex="0">
            {children()}
            <span class="tip-bubble" role="tooltip">{text}</span>
        </span>
    }
}

/// Slow-scan placeholder: sweeping radar ring plus rotating step labels.
#[component]
pub fn Scanning(
    #[prop(into)] title: String,
    steps: Vec<&'static str>,
    #[prop(optional, into)] icon: Option<String>,
) -> impl IntoView {
    let step = RwSignal::new(0usize);
    let n = steps.len().max(1);
    if let Ok(h) = set_interval_with_handle(
        move || step.update(|s| *s = (*s + 1) % n),
        std::time::Duration::from_millis(1100),
    ) {
        on_cleanup(move || h.clear());
    }
    let label = move || steps.get(step.get()).copied().unwrap_or_default();
    view! {
        <div class="scanning card" role="status">
            <div class="scan-radar" aria-hidden="true">
                <span class="scan-sweep" />
                <span class="scan-core"><Icon name=icon.unwrap_or_else(|| "search".into()) size=20 /></span>
            </div>
            <div class="scan-text">
                <p class="scan-title">{title}</p>
                <p class="scan-step">{label}<span class="scan-dots" /></p>
            </div>
        </div>
    }
}
