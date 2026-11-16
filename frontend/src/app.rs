use leptos::prelude::*;

use crate::components::*;
use crate::fmt;
use crate::pages;
use crate::state::{use_app, AppState, Page};

#[component]
pub fn App() -> impl IntoView {
    let app = AppState::new();
    provide_context(app);
    app.bootstrap();

    let page_view = move || match app.page.get() {
        Page::Panel => view! { <pages::panel::PanelPage /> }.into_any(),
        Page::Memoria => view! { <pages::memory::MemoryPage /> }.into_any(),
        Page::Bateria => view! { <pages::battery::BatteryPage /> }.into_any(),
        Page::Ventiladores => view! { <pages::fans::FansPage /> }.into_any(),
        Page::Almacenamiento => view! { <pages::storage::StoragePage /> }.into_any(),
        Page::Limpieza => view! { <pages::cleanup::CleanupPage /> }.into_any(),
        Page::Apps => view! { <pages::apps::AppsPage /> }.into_any(),
        Page::Nube => view! { <pages::cloud::CloudPage /> }.into_any(),
        Page::Procesos => view! { <pages::processes::ProcessesPage /> }.into_any(),
        Page::Optimizar => view! { <pages::optimize::OptimizePage /> }.into_any(),
        Page::Ajustes => view! { <pages::settings::SettingsPage /> }.into_any(),
    };

    // Each section starts at the top.
    let content_ref = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        app.page.track();
        if let Some(el) = content_ref.get_untracked() {
            el.set_scroll_top(0);
        }
    });

    view! {
        <div class="shell">
            <Sidebar />
            <main class="main">
                <TopBar />
                <div class="content" node_ref=content_ref>
                    <div class="page">{page_view}</div>
                </div>
            </main>
            <ToastHost />
        </div>
    }
}

#[component]
fn TopBar() -> impl IntoView {
    let app = use_app();
    let helper_chip = move || {
        let (tone, text) = app.helper.with(|h| match h {
            None => ("neutral", "Asistente…"),
            Some(h) if h.installed && h.reachable => ("accent", "Asistente activo"),
            Some(h) if h.installed => ("warn", "Asistente sin respuesta"),
            Some(_) => ("neutral", "Sin asistente"),
        });
        view! {
            <button
                type="button"
                class=format!("status-chip tone-{tone}")
                on:click=move |_| app.page.set(Page::Ajustes)
            >
                <span class="status-dot" />
                {text}
            </button>
        }
    };
    view! {
        <header class="topbar" data-tauri-drag-region="">
            <div class="topbar-title" data-tauri-drag-region="">
                <span class="eyebrow">{move || app.page.get().section()}</span>
                <h1>{move || app.page.get().title()}</h1>
            </div>
            <div class="topbar-right">
                {app.mock.then(|| view! { <Badge tone="warn".to_string()>"Vista previa · datos simulados"</Badge> })}
                <span class="live">
                    <span class="live-dot" />
                    "En vivo"
                </span>
                {helper_chip}
            </div>
        </header>
    }
}

#[component]
fn Sidebar() -> impl IntoView {
    let app = use_app();
    let sections: [(&str, &[Page]); 3] = [
        ("Monitor", &[Page::Panel, Page::Memoria, Page::Bateria, Page::Ventiladores]),
        ("Disco", &[Page::Almacenamiento, Page::Limpieza, Page::Apps, Page::Nube]),
        ("Sistema", &[Page::Procesos, Page::Optimizar]),
    ];

    let machine_line = move || {
        app.machine
            .with(|m| m.as_ref().map(|m| m.chip.clone()).unwrap_or_else(|| "Mac".into()))
    };
    let helper_ok = move || app.helper_ready();

    view! {
        <aside class="sidebar">
            <div class="sidebar-head" data-tauri-drag-region="">
                <Logo />
                <span class="brand">"iclear"</span>
            </div>
            <nav class="nav" aria-label="Secciones">
                {sections
                    .into_iter()
                    .map(|(label, pages)| {
                        view! {
                            <div class="nav-section">
                                <p class="nav-label">{label}</p>
                                {pages.iter().map(|p| view! { <NavItem page=*p /> }).collect_view()}
                            </div>
                        }
                    })
                    .collect_view()}
            </nav>
            <div class="sidebar-foot">
                <NavItem page=Page::Ajustes />
                <div class="machine-card">
                    <span class="status-dot" class:ok=helper_ok />
                    <div class="machine-text">
                        <span class="machine-chip">{machine_line}</span>
                        <span class="machine-sub">
                            {move || if helper_ok() { "Control completo" } else { "Solo lectura" }}
                        </span>
                    </div>
                </div>
            </div>
        </aside>
    }
}

#[component]
fn NavItem(page: Page) -> impl IntoView {
    let app = use_app();
    let meta = move || {
        if page == Page::Apps {
            return app.apps.with(|r| {
                r.as_ref().map(|r| {
                    r.apps.iter().filter(|a| a.verdict == iclear_protocol::UsageVerdict::Unused).count().to_string()
                })
            });
        }
        if page == Page::Nube {
            // Live progress of the running cloud transfer, if any.
            return app.transfers.with(|ts| {
                ts.iter().find(|t| !t.phase.is_finished()).map(|t| {
                    let (arrow, pct) = crate::pages::cloud::headline_pct(t);
                    format!("{arrow}{}", fmt::progress(pct))
                })
            });
        }
        app.metrics.with(|m| {
            let m = m.as_ref()?;
            match page {
                Page::Panel => Some(fmt::pct(m.system.cpu_total)),
                Page::Memoria => (m.system.mem_total > 0)
                    .then(|| fmt::pct(m.system.mem_used as f32 / m.system.mem_total as f32 * 100.0)),
                Page::Bateria => m.battery.as_ref().map(|b| format!("{}%", b.percent)),
                Page::Ventiladores => m.fans.first().map(|f| fmt::rpm(f.actual_rpm)),
                Page::Almacenamiento => m
                    .system
                    .disks
                    .iter()
                    .find(|d| d.mount_point == "/")
                    .or(m.system.disks.first())
                    .map(|d| fmt::bytes(d.available_bytes)),
                _ => None,
            }
        })
    };
    view! {
        <button
            type="button"
            class="nav-item"
            aria-label=page.title()
            class:active=move || app.page.get() == page
            aria-current=move || (app.page.get() == page).then_some("page")
            on:click=move |_| app.page.set(page)
        >
            <Icon name=page.icon() size=17 />
            <span class="nav-text">{page.title()}</span>
            <span class="nav-meta num">{meta}</span>
        </button>
    }
}
