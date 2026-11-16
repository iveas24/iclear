use std::sync::atomic::{AtomicUsize, Ordering};

use leptos::prelude::*;

static GRADIENT_SEQ: AtomicUsize = AtomicUsize::new(0);

const W: f64 = 300.0;

/// Live line + area chart. Fills from the right as samples arrive.
#[component]
pub fn Sparkline(
    #[prop(into)] data: Signal<Vec<f32>>,
    #[prop(optional)] min: Option<f32>,
    #[prop(optional)] max: Option<f32>,
    #[prop(default = crate::state::HISTORY_LEN)] capacity: usize,
    #[prop(default = 72)] height: u32,
    #[prop(into, default = "accent".to_string())] tone: String,
) -> impl IntoView {
    let id = format!("spark-{}", GRADIENT_SEQ.fetch_add(1, Ordering::Relaxed));
    let h = height as f64;
    let pad = 6.0;

    let geometry = Memo::new(move |_| {
        let d = data.get();
        if d.is_empty() {
            return (String::new(), String::new(), None);
        }
        let lo = min.unwrap_or_else(|| d.iter().copied().fold(f32::MAX, f32::min)) as f64;
        let hi = max.unwrap_or_else(|| d.iter().copied().fold(f32::MIN, f32::max)) as f64;
        let (lo, hi) = if min.is_none() || max.is_none() {
            let span = (hi - lo).max(1.0);
            (
                if min.is_some() { lo } else { lo - span * 0.15 },
                if max.is_some() { hi } else { hi + span * 0.15 },
            )
        } else {
            (lo, hi.max(lo + 1.0))
        };
        let cap = capacity.max(2);
        let offset = cap.saturating_sub(d.len());
        let pts: Vec<(f64, f64)> = d
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let x = (i + offset) as f64 / (cap - 1) as f64 * W;
                let t = ((*v as f64 - lo) / (hi - lo)).clamp(0.0, 1.0);
                (x, h - pad - t * (h - pad * 2.0))
            })
            .collect();
        let line = pts.iter().map(|(x, y)| format!("{x:.1},{y:.1}")).collect::<Vec<_>>().join(" ");
        let first_x = pts[0].0;
        let last = *pts.last().unwrap();
        let area = format!("M{first_x:.1},{h} L{} L{:.1},{h} Z", line.replace(' ', " L"), last.0);
        (line, area, Some((last.0 / W * 100.0, last.1 / h * 100.0)))
    });

    let grad_ref = format!("url(#{id})");
    view! {
        <div class=format!("sparkline tone-{tone}") style=format!("height:{height}px")>
            <svg
                viewBox=format!("0 0 {W} {height}")
                preserveAspectRatio="none"
                width="100%"
                height=height
                aria-hidden="true"
            >
                <defs>
                    <linearGradient id=id.clone() x1="0" y1="0" x2="0" y2="1">
                        <stop offset="0%" class="spark-stop-top" />
                        <stop offset="100%" class="spark-stop-bottom" />
                    </linearGradient>
                </defs>
                <path class="spark-area" d=move || geometry.with(|g| g.1.clone()) fill=grad_ref.clone() />
                <polyline
                    class="spark-line"
                    points=move || geometry.with(|g| g.0.clone())
                    fill="none"
                    vector-effect="non-scaling-stroke"
                />
            </svg>
            {move || {
                geometry
                    .with(|g| g.2)
                    .map(|(x, y)| {
                        view! { <span class="spark-dot" style=format!("left:{x:.2}%;top:{y:.2}%") /> }
                    })
            }}
        </div>
    }
}
