//! Global app state shared through Leptos context.

use iclear_protocol::*;
use leptos::prelude::*;
use leptos::task::spawn_local;
use std::time::Duration;

use crate::api;

pub const HISTORY_LEN: usize = 60;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Panel,
    Memoria,
    Bateria,
    Ventiladores,
    Almacenamiento,
    Limpieza,
    Apps,
    Nube,
    Procesos,
    Optimizar,
    Ajustes,
}

impl Page {
    /// Maps backend page ids (`InsightAction::OpenPage`) to a page.
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "panel" | "dashboard" => Page::Panel,
            "memory" | "memoria" => Page::Memoria,
            "battery" => Page::Bateria,
            "fans" => Page::Ventiladores,
            "storage" => Page::Almacenamiento,
            "cleanup" => Page::Limpieza,
            "apps" => Page::Apps,
            "cloud" | "nube" => Page::Nube,
            "processes" => Page::Procesos,
            "optimize" => Page::Optimizar,
            "settings" => Page::Ajustes,
            _ => return None,
        })
    }

    pub fn title(self) -> &'static str {
        match self {
            Page::Panel => "Panel",
            Page::Memoria => "Memoria",
            Page::Bateria => "Batería",
            Page::Ventiladores => "Ventiladores",
            Page::Almacenamiento => "Almacenamiento",
            Page::Limpieza => "Limpieza",
            Page::Apps => "Apps",
            Page::Nube => "Nube",
            Page::Procesos => "Procesos",
            Page::Optimizar => "Optimizar",
            Page::Ajustes => "Ajustes",
        }
    }

    pub fn section(self) -> &'static str {
        match self {
            Page::Panel | Page::Memoria | Page::Bateria | Page::Ventiladores => "Monitor",
            Page::Almacenamiento | Page::Limpieza | Page::Apps | Page::Nube => "Disco",
            Page::Procesos | Page::Optimizar => "Sistema",
            Page::Ajustes => "General",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Page::Panel => "dashboard",
            Page::Memoria => "memory",
            Page::Bateria => "battery",
            Page::Ventiladores => "fan",
            Page::Almacenamiento => "disk",
            Page::Limpieza => "sparkle",
            Page::Apps => "apps",
            Page::Nube => "cloud",
            Page::Procesos => "list",
            Page::Optimizar => "bolt",
            Page::Ajustes => "sliders",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    Success,
    Error,
    Info,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: u32,
    pub kind: ToastKind,
    pub title: String,
    pub body: Option<String>,
}

#[derive(Clone, Copy)]
pub struct AppState {
    pub page: RwSignal<Page>,
    pub metrics: RwSignal<Option<Metrics>>,
    pub cpu_hist: RwSignal<Vec<f32>>,
    pub temp_hist: RwSignal<Vec<f32>>,
    pub rx_hist: RwSignal<Vec<f32>>,
    pub tx_hist: RwSignal<Vec<f32>>,
    pub machine: RwSignal<Option<MachineInfo>>,
    pub helper: RwSignal<Option<HelperStatus>>,
    pub helper_busy: RwSignal<bool>,
    pub toasts: RwSignal<Vec<Toast>>,
    /// Last memory report (polled by the Memoria page while visible).
    pub memory: RwSignal<Option<MemoryReport>>,
    /// Installed-apps scan; cached until the user asks to rescan.
    pub apps: RwSignal<Option<AppsReport>>,
    pub apps_loading: RwSignal<bool>,
    pub apps_error: RwSignal<Option<String>>,
    /// Cloud transfers (downloads, backups, restores), polled with metrics.
    pub transfers: RwSignal<Vec<Transfer>>,
    next_toast: RwSignal<u32>,
    pub mock: bool,
}

fn push_hist(sig: RwSignal<Vec<f32>>, v: f32) {
    sig.update(|h| {
        h.push(v);
        if h.len() > HISTORY_LEN {
            let excess = h.len() - HISTORY_LEN;
            h.drain(..excess);
        }
    });
}

impl AppState {
    pub fn new() -> Self {
        Self {
            page: RwSignal::new(Page::Panel),
            metrics: RwSignal::new(None),
            cpu_hist: RwSignal::new(Vec::new()),
            temp_hist: RwSignal::new(Vec::new()),
            rx_hist: RwSignal::new(Vec::new()),
            tx_hist: RwSignal::new(Vec::new()),
            machine: RwSignal::new(None),
            helper: RwSignal::new(None),
            helper_busy: RwSignal::new(false),
            toasts: RwSignal::new(Vec::new()),
            memory: RwSignal::new(None),
            apps: RwSignal::new(None),
            apps_loading: RwSignal::new(false),
            apps_error: RwSignal::new(None),
            transfers: RwSignal::new(Vec::new()),
            next_toast: RwSignal::new(1),
            mock: !api::is_tauri(),
        }
    }

    /// Loads initial data and subscribes to live metrics.
    pub fn bootstrap(self) {
        spawn_local(async move {
            match api::get_machine_info().await {
                Ok(m) => self.machine.set(Some(m)),
                Err(e) => self.toast(ToastKind::Error, "No se pudo leer el equipo", Some(e)),
            }
        });
        spawn_local(async move {
            if let Ok(m) = api::get_metrics().await {
                self.ingest(m);
            }
        });
        self.refresh_helper();
        if self.mock {
            // Pre-fill charts so the preview does not start empty.
            let now = js_sys::Date::now() / 1000.0;
            for i in (1..HISTORY_LEN).rev() {
                self.ingest(crate::mock::metrics_at(now - i as f64 * 1.5));
            }
        }
        self.poll_transfers();
        api::on_metrics(move |m| {
            self.ingest(m);
            self.poll_transfers();
        });
    }

    /// Refreshes cloud transfers and toasts the ones that just finished.
    pub fn poll_transfers(self) {
        spawn_local(async move {
            let Ok(list) = api::list_transfers().await else { return };
            let finished_now: Vec<Transfer> = self.transfers.with_untracked(|old| {
                list.iter()
                    .filter(|t| t.phase.is_finished())
                    .filter(|t| old.iter().any(|o| o.id == t.id && !o.phase.is_finished()))
                    .cloned()
                    .collect()
            });
            for t in finished_now {
                let kind = match t.phase {
                    TransferPhase::Done => ToastKind::Success,
                    TransferPhase::Failed => ToastKind::Error,
                    _ => ToastKind::Info,
                };
                let verb = match t.phase {
                    TransferPhase::Done => "terminó",
                    TransferPhase::Failed => "falló",
                    _ => "se canceló",
                };
                self.toast(kind, format!("{} {verb}", t.label), t.message.clone());
            }
            if self.transfers.with_untracked(|old| old != &list) {
                self.transfers.set(list);
            }
        });
    }

    fn ingest(self, m: Metrics) {
        push_hist(self.cpu_hist, m.system.cpu_total);
        if let Some(t) = m.temp_summary.cpu_avg.or(m.temp_summary.cpu_max) {
            push_hist(self.temp_hist, t);
        }
        push_hist(self.rx_hist, m.system.net_rx_per_sec as f32);
        push_hist(self.tx_hist, m.system.net_tx_per_sec as f32);
        self.metrics.set(Some(m));
    }

    pub fn refresh_helper(self) {
        spawn_local(async move {
            match api::helper_status().await {
                Ok(s) => self.helper.set(Some(s)),
                Err(e) => self.helper.set(Some(HelperStatus {
                    error: Some(e),
                    ..Default::default()
                })),
            }
        });
    }

    /// `true` when the privileged helper is installed and answering.
    pub fn helper_ready(self) -> bool {
        self.helper
            .with(|h| h.as_ref().is_some_and(|h| h.installed && h.reachable))
    }

    pub fn helper_state(self) -> Option<HelperState> {
        self.helper.with(|h| h.as_ref().and_then(|h| h.state.clone()))
    }

    pub fn apply_helper_state(self, state: HelperState) {
        self.helper.update(|h| match h {
            Some(h) => h.state = Some(state),
            None => {
                *h = Some(HelperStatus {
                    installed: true,
                    reachable: true,
                    state: Some(state),
                    error: None,
                })
            }
        });
    }

    pub fn install_helper(self) {
        self.helper_busy.set(true);
        spawn_local(async move {
            match api::install_helper().await {
                Ok(s) => {
                    let ok = s.installed && s.reachable;
                    self.helper.set(Some(s));
                    if ok {
                        self.toast(ToastKind::Success, "Asistente instalado", None);
                    } else {
                        self.toast(
                            ToastKind::Error,
                            "El asistente no responde",
                            Some("Se instaló, pero no se pudo conectar. Intenta nuevamente.".into()),
                        );
                    }
                }
                Err(e) => self.toast(ToastKind::Error, "No se pudo instalar el asistente", Some(e)),
            }
            self.helper_busy.set(false);
        });
    }

    pub fn uninstall_helper(self) {
        self.helper_busy.set(true);
        spawn_local(async move {
            match api::uninstall_helper().await {
                Ok(s) => {
                    self.helper.set(Some(s));
                    self.toast(ToastKind::Info, "Asistente desinstalado", None);
                }
                Err(e) => self.toast(ToastKind::Error, "No se pudo desinstalar", Some(e)),
            }
            self.helper_busy.set(false);
        });
    }

    pub fn refresh_memory(self) {
        spawn_local(async move {
            match api::get_memory_report().await {
                Ok(r) => self.memory.set(Some(r)),
                Err(e) => web_sys::console::warn_1(&format!("memoria: {e}").into()),
            }
        });
    }

    /// Runs the (slow) installed-apps scan unless one is already running.
    pub fn scan_apps(self) {
        if self.apps_loading.get_untracked() {
            return;
        }
        self.apps_loading.set(true);
        self.apps_error.set(None);
        spawn_local(async move {
            match api::get_apps_report().await {
                Ok(r) => self.apps.set(Some(r)),
                Err(e) => self.apps_error.set(Some(e)),
            }
            self.apps_loading.set(false);
        });
    }

    pub fn toast(self, kind: ToastKind, title: impl Into<String>, body: Option<String>) {
        let id = self.next_toast.get_untracked();
        self.next_toast.set(id + 1);
        self.toasts.update(|t| {
            t.push(Toast { id, kind, title: title.into(), body });
            if t.len() > 4 {
                t.remove(0);
            }
        });
        let toasts = self.toasts;
        set_timeout(
            move || toasts.update(|t| t.retain(|x| x.id != id)),
            Duration::from_millis(if kind == ToastKind::Error { 7000 } else { 4500 }),
        );
    }

    pub fn dismiss_toast(self, id: u32) {
        self.toasts.update(|t| t.retain(|x| x.id != id));
    }
}

pub fn use_app() -> AppState {
    expect_context::<AppState>()
}
