use leptos::prelude::*;

use super::{Icon, Spinner};

/// Pill-shaped button. Variants: "" (default), "primary", "ghost", "danger", "sm".
#[component]
pub fn Pill(
    #[prop(into, default = String::new())] variant: String,
    #[prop(optional, into)] icon: Option<String>,
    #[prop(into, default = false.into())] disabled: Signal<bool>,
    #[prop(into, default = false.into())] busy: Signal<bool>,
    #[prop(optional, into)] title: Option<String>,
    #[prop(into)] on_click: Callback<()>,
    children: Children,
) -> impl IntoView {
    let leading = move || {
        if busy.get() {
            Some(view! { <Spinner /> }.into_any())
        } else {
            icon.clone().map(|i| view! { <Icon name=i size=15 /> }.into_any())
        }
    };
    view! {
        <button
            type="button"
            class=format!("pill {variant}")
            class:is-busy=move || busy.get()
            title=title
            disabled=move || disabled.get() || busy.get()
            on:click=move |_| on_click.run(())
        >
            {leading}
            <span>{children()}</span>
        </button>
    }
}

/// Filter chip.
#[component]
pub fn Chip(
    #[prop(into)] selected: Signal<bool>,
    #[prop(into)] on_click: Callback<()>,
    children: Children,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class="chip"
            class:active=move || selected.get()
            aria-pressed=move || selected.get().to_string()
            on:click=move |_| on_click.run(())
        >
            {children()}
        </button>
    }
}

/// Small status badge. Tones: accent, warn, danger, neutral.
#[component]
pub fn Badge(
    #[prop(into, default = "neutral".to_string().into())] tone: Signal<String>,
    #[prop(default = false)] dot: bool,
    children: Children,
) -> impl IntoView {
    view! {
        <span class=move || format!("badge tone-{}", tone.get())>
            {dot.then(|| view! { <span class="badge-dot" /> })}
            {children()}
        </span>
    }
}

/// iOS-like switch.
#[component]
pub fn Toggle(
    #[prop(into)] checked: Signal<bool>,
    #[prop(into)] on_toggle: Callback<bool>,
    #[prop(into, default = false.into())] disabled: Signal<bool>,
    #[prop(into, default = String::new())] label: String,
) -> impl IntoView {
    view! {
        <button
            type="button"
            role="switch"
            class="toggle"
            class:on=move || checked.get()
            aria-checked=move || checked.get().to_string()
            aria-label=label
            disabled=move || disabled.get()
            on:click=move |_| on_toggle.run(!checked.get_untracked())
        >
            <span class="toggle-knob" />
        </button>
    }
}

/// Square checkbox rendered as a button (keeps styling consistent).
#[component]
pub fn Check(
    #[prop(into)] checked: Signal<bool>,
    #[prop(into)] on_toggle: Callback<bool>,
    #[prop(into, default = false.into())] disabled: Signal<bool>,
    #[prop(into, default = String::new())] label: String,
) -> impl IntoView {
    view! {
        <button
            type="button"
            role="checkbox"
            class="check"
            class:on=move || checked.get()
            aria-checked=move || checked.get().to_string()
            aria-label=label
            disabled=move || disabled.get()
            on:click=move |ev| {
                ev.stop_propagation();
                on_toggle.run(!checked.get_untracked())
            }
        >
            <Icon name="check" size=12 />
        </button>
    }
}

/// Range slider. `on_input` fires while dragging, `on_change` on release.
#[component]
pub fn Slider(
    #[prop(into)] value: Signal<f64>,
    min: f64,
    max: f64,
    #[prop(default = 1.0)] step: f64,
    #[prop(into)] on_input: Callback<f64>,
    #[prop(into, default = Callback::new(|_| {}))] on_change: Callback<f64>,
    #[prop(into, default = false.into())] disabled: Signal<bool>,
    #[prop(into, default = String::new())] label: String,
) -> impl IntoView {
    let fill = move || {
        let p = ((value.get() - min) / (max - min) * 100.0).clamp(0.0, 100.0);
        format!("--fill:{p:.2}%")
    };
    let parse = |ev: &leptos::ev::Event| event_target_value(ev).parse::<f64>().ok();
    view! {
        <input
            type="range"
            class="slider"
            min=min
            max=max
            step=step
            aria-label=label
            prop:value=move || value.get().to_string()
            style=fill
            disabled=move || disabled.get()
            on:input=move |ev| {
                if let Some(v) = parse(&ev) {
                    on_input.run(v)
                }
            }
            on:change=move |ev| {
                if let Some(v) = parse(&ev) {
                    on_change.run(v)
                }
            }
        />
    }
}

/// Segmented control (radio group).
#[component]
pub fn Segmented(
    options: Vec<(&'static str, &'static str)>,
    #[prop(into)] selected: Signal<String>,
    #[prop(into)] on_select: Callback<String>,
    #[prop(into, default = false.into())] disabled: Signal<bool>,
) -> impl IntoView {
    view! {
        <div class="segmented" role="radiogroup">
            {options
                .into_iter()
                .map(|(value, label)| {
                    view! {
                        <button
                            type="button"
                            role="radio"
                            class="seg"
                            class:active=move || selected.get() == value
                            aria-checked=move || (selected.get() == value).to_string()
                            disabled=move || disabled.get()
                            on:click=move |_| on_select.run(value.to_string())
                        >
                            {label}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

/// Text search field with icon.
#[component]
pub fn SearchField(
    #[prop(into)] value: RwSignal<String>,
    #[prop(into)] placeholder: String,
) -> impl IntoView {
    view! {
        <label class="search">
            <Icon name="search" size=15 />
            <input
                type="search"
                placeholder=placeholder
                prop:value=move || value.get()
                on:input=move |ev| value.set(event_target_value(&ev))
            />
        </label>
    }
}
