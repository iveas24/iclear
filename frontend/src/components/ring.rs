use leptos::prelude::*;

/// Circular progress ring (0–100) with a soft glow and an inner tick track.
#[component]
pub fn Ring(
    #[prop(into)] value: Signal<f64>,
    #[prop(default = 120)] size: u32,
    #[prop(default = 8.0)] stroke: f64,
    #[prop(into, default = "accent".to_string().into())] tone: Signal<String>,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    let half = size as f64 / 2.0;
    let r = (size as f64 - stroke) / 2.0 - 2.0;
    let c = std::f64::consts::TAU * r;
    let tick_r = (r - stroke - 4.0).max(4.0);
    let offset = move || format!("{:.2}", c * (1.0 - value.get().clamp(0.0, 100.0) / 100.0));
    view! {
        <div
            class=move || format!("ring tone-{}", tone.get())
            style=format!("width:{size}px;height:{size}px")
        >
            <svg width=size height=size viewBox=format!("0 0 {size} {size}") aria-hidden="true">
                <circle class="ring-ticks" cx=half cy=half r=tick_r fill="none" />
                <circle class="ring-track" cx=half cy=half r=r stroke-width=stroke fill="none" />
                <circle
                    class="ring-value"
                    cx=half
                    cy=half
                    r=r
                    fill="none"
                    stroke-width=stroke
                    stroke-linecap="round"
                    stroke-dasharray=format!("{c:.2} {c:.2}")
                    stroke-dashoffset=offset
                    transform=format!("rotate(-90 {half} {half})")
                />
            </svg>
            <div class="ring-center">{children.map(|c| c())}</div>
        </div>
    }
}

/// Picks a tone for a percentage where high is bad (CPU, disk, temps).
pub fn load_tone(pct: f64) -> String {
    if pct >= 90.0 {
        "danger".into()
    } else if pct >= 75.0 {
        "warn".into()
    } else {
        "accent".into()
    }
}
