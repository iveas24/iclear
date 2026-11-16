use std::collections::HashSet;

use iclear_protocol::{CleanupCategory, CleanupPreview, DeleteReport, Risk};
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::storage::report_toast;
use super::PageIntro;
use crate::components::*;
use crate::state::{use_app, ToastKind};
use crate::{api, fmt};

fn risk_badge(r: Risk) -> (&'static str, &'static str) {
    match r {
        Risk::Safe => ("accent", "Seguro"),
        Risk::Moderate => ("warn", "Moderado"),
        Risk::Review => ("danger", "Revisar"),
    }
}

#[component]
pub fn CleanupPage() -> impl IntoView {
    let app = use_app();
    let cats = RwSignal::new(None::<Vec<CleanupCategory>>);
    let loading = RwSignal::new(false);
    let err = RwSignal::new(None::<String>);
    let selected = RwSignal::new(HashSet::<String>::new());

    let scan = move || {
        loading.set(true);
        err.set(None);
        spawn_local(async move {
            match api::cleanup_categories().await {
                Ok(list) => {
                    selected.set(
                        list.iter()
                            .filter(|c| c.selected_by_default && c.size_bytes > 0)
                            .map(|c| c.id.clone())
                            .collect(),
                    );
                    cats.set(Some(list));
                }
                Err(e) => err.set(Some(e)),
            }
            loading.set(false);
        });
    };
    scan();

    let totals = Memo::new(move |_| {
        cats.with(|c| {
            selected.with(|s| {
                let chosen: Vec<&CleanupCategory> =
                    c.iter().flatten().filter(|c| s.contains(&c.id)).collect();
                (chosen.len(), chosen.iter().map(|c| c.size_bytes).sum::<u64>())
            })
        })
    });
    let reclaimable = Memo::new(move |_| cats.with(|c| c.iter().flatten().map(|c| c.size_bytes).sum::<u64>()));

    let preview = RwSignal::new(None::<CleanupPreview>);
    let previewing = RwSignal::new(false);
    let review = move |_| {
        let ids: Vec<String> = selected.get_untracked().into_iter().collect();
        if ids.is_empty() {
            return;
        }
        previewing.set(true);
        spawn_local(async move {
            match api::preview_cleanup(ids).await {
                Ok(p) => preview.set(Some(p)),
                Err(e) => app.toast(ToastKind::Error, "No se pudo preparar la limpieza", Some(e)),
            }
            previewing.set(false);
        });
    };
    let on_done = move |r: DeleteReport| {
        report_toast(app, &r);
        scan();
    };

    view! {
        <PageIntro text="Elimina archivos que macOS y tus apps regeneran solos. Antes de borrar verás la lista exacta y podrás desmarcar lo que quieras conservar.">
            <Pill variant="ghost" icon="refresh" busy=loading on_click=move |_| scan()>
                "Volver a analizar"
            </Pill>
        </PageIntro>

        <div class="cleanup-hero card">
            <div>
                <span class="eyebrow">"Recuperable"</span>
                <span class="hero-number num">
                    {move || if loading.get() && cats.with(|c| c.is_none()) { "…".to_string() } else { fmt::bytes(reclaimable.get()) }}
                </span>
            </div>
            <div class="hero-split">
                <div>
                    <span class="eyebrow">"Seleccionado"</span>
                    <span class="hero-sub num accent">{move || fmt::bytes(totals.get().1)}</span>
                </div>
                <div>
                    <span class="eyebrow">"Categorías"</span>
                    <span class="hero-sub num">{move || totals.get().0}</span>
                </div>
            </div>
            <Pill
                variant="primary"
                icon="sparkle"
                busy=previewing
                disabled=Signal::derive(move || totals.get().0 == 0 || loading.get())
                on_click=review
            >
                "Revisar y limpiar"
            </Pill>
        </div>

        {move || err.get().map(|e| view! { <ErrorNote message=e /> })}

        {move || {
            if loading.get() && cats.with(|c| c.is_none()) {
                return view! {
                    <div class="grid grid-3">
                        {(0..6)
                            .map(|_| {
                                view! {
                                    <div class="card stack">
                                        <Skeleton height=14 width="40%" />
                                        <Skeleton height=12 />
                                        <Skeleton height=12 width="70%" />
                                        <Skeleton height=26 width="30%" />
                                    </div>
                                }
                            })
                            .collect_view()}
                    </div>
                }
                .into_any();
            }
            let list = cats.get().unwrap_or_default();
            view! {
                <div class="grid grid-3" class:dimmed=move || loading.get()>
                    {list.into_iter().map(|c| view! { <CategoryCard cat=c selected=selected /> }).collect_view()}
                </div>
            }
            .into_any()
        }}

        <CleanupReview preview=preview on_done=on_done />
    }
}

#[component]
fn CategoryCard(cat: CleanupCategory, selected: RwSignal<HashSet<String>>) -> impl IntoView {
    let app = use_app();
    let id = cat.id.clone();
    let id2 = cat.id.clone();
    let checked = Signal::derive(move || selected.with(|s| s.contains(&id)));
    let empty = cat.size_bytes == 0;
    let needs_helper = cat.requires_helper;
    let blocked = Signal::derive(move || empty || (needs_helper && !app.helper_ready()));
    let toggle = Callback::new(move |on: bool| {
        if blocked.get_untracked() {
            return;
        }
        selected.update(|s| {
            if on {
                s.insert(id2.clone());
            } else {
                s.remove(&id2);
            }
        })
    });
    let (tone, risk_label) = risk_badge(cat.risk);
    view! {
        <section
            class="card category"
            class:selected=checked
            class:disabled=blocked
            on:click=move |_| toggle.run(!checked.get_untracked())
        >
            <header class="category-head">
                <Check checked=checked disabled=blocked label=format!("Incluir {}", cat.name) on_toggle=toggle />
                <h3 class="card-title">{cat.name.clone()}</h3>
                <Badge tone=tone.to_string() dot=true>{risk_label}</Badge>
            </header>
            <p class="category-desc">{cat.description.clone()}</p>
            <p class="category-roots mono truncate" title=cat.roots.join(", ")>{cat.roots.join(" · ")}</p>
            <footer class="category-foot">
                <span class="category-size num" class:muted=empty>
                    {if empty { "Vacío".to_string() } else { fmt::bytes(cat.size_bytes) }}
                </span>
                <span class="muted">{fmt::plural(cat.item_count, "elemento", "elementos")}</span>
                {needs_helper.then(|| view! { <Badge tone="neutral".to_string()>"Requiere asistente"</Badge> })}
            </footer>
        </section>
    }
}
