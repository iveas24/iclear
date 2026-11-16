use leptos::prelude::*;

/// Sortable column header. `sort` holds (key, descending).
#[component]
pub fn SortHeader<K>(
    #[prop(into)] label: String,
    key: K,
    sort: RwSignal<(K, bool)>,
    #[prop(default = false)] numeric: bool,
    #[prop(into, default = String::new())] class: String,
) -> impl IntoView
where
    K: Copy + PartialEq + Send + Sync + 'static,
{
    let active = move || sort.get().0 == key;
    let aria = move || match (active(), sort.get().1) {
        (false, _) => "none",
        (true, true) => "descending",
        (true, false) => "ascending",
    };
    view! {
        <th class=format!("{class} {}", if numeric { "num-col" } else { "" }) aria-sort=aria>
            <button
                type="button"
                class="th-btn"
                class:active=active
                on:click=move |_| {
                    sort.update(|(k, desc)| {
                        if *k == key {
                            *desc = !*desc;
                        } else {
                            *k = key;
                            *desc = numeric;
                        }
                    })
                }
            >
                <span>{label}</span>
                <span class="sort-ind">
                    {move || match (active(), sort.get().1) {
                        (false, _) => "",
                        (true, true) => "↓",
                        (true, false) => "↑",
                    }}
                </span>
            </button>
        </th>
    }
}

/// Thin horizontal meter (0–100).
#[component]
pub fn Meter(
    #[prop(into)] value: Signal<f64>,
    #[prop(into, default = "accent".to_string().into())] tone: Signal<String>,
) -> impl IntoView {
    view! {
        <div class=move || format!("meter tone-{}", tone.get())>
            <div
                class="meter-fill"
                style=move || format!("width:{:.2}%", value.get().clamp(0.0, 100.0))
            />
        </div>
    }
}
