use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::*;
use crate::state::{use_app, ToastKind};
use crate::{api, fmt};

#[component]
pub fn SettingsPage() -> impl IntoView {
    let app = use_app();
    let confirm_uninstall = RwSignal::new(false);

    let status = move || {
        app.helper.with(|h| match h {
            None => ("neutral", "Comprobando…"),
            Some(h) if h.installed && h.reachable => ("accent", "Activo"),
            Some(h) if h.installed => ("warn", "Instalado, sin respuesta"),
            Some(_) => ("neutral", "No instalado"),
        })
    };
    let yes_no = |b: bool| if b { "Sí".to_string() } else { "No".to_string() };
    let installed = move || app.helper.with(|h| h.as_ref().is_some_and(|h| h.installed));
    let state_field = move |f: fn(&iclear_protocol::HelperState) -> String| {
        Signal::derive(move || app.helper_state().map(|s| f(&s)).unwrap_or_else(|| "—".into()))
    };
    let caps = move || {
        app.helper_state()
            .map(|s| {
                vec![
                    ("Límite de carga", s.supports_charge_limit),
                    ("Descarga forzada", s.supports_force_discharge),
                    ("Ventiladores", s.supports_fan_control),
                ]
            })
            .unwrap_or_default()
    };
    let machine = move |f: fn(&iclear_protocol::MachineInfo) -> String| {
        Signal::derive(move || app.machine.with(|m| m.as_ref().map(f).unwrap_or_else(|| "—".into())))
    };

    let open_fda = move |_| {
        spawn_local(async move {
            if let Err(e) = api::open_full_disk_access_settings().await {
                app.toast(ToastKind::Error, "No se pudo abrir Ajustes del Sistema", Some(e));
            }
        });
    };

    view! {
        <div class="grid grid-2">
            <section class="card">
                <header class="card-head">
                    <span class="card-icon"><Icon name="shield" size=16 /></span>
                    <div class="card-head-text">
                        <h3 class="card-title">"Asistente con privilegios"</h3>
                        <p class="card-sub">"Servicio launchd com.iclear.helper"</p>
                    </div>
                    <Badge tone=Signal::derive(move || status().0.to_string()) dot=true>{move || status().1}</Badge>
                </header>
                <p class="card-text">
                    "Controla ventiladores, límite de carga y tareas de mantenimiento. Solo acepta un conjunto cerrado de comandos validados a través de un socket local; no ejecuta comandos arbitrarios."
                </p>
                <div class="info-list">
                    <InfoRow label="Instalado" value=Signal::derive(move || yes_no(installed())) />
                    <InfoRow label="Conectado" value=Signal::derive(move || yes_no(app.helper_ready())) />
                    <InfoRow label="Versión" value=state_field(|s| s.helper_version.clone()) />
                    <InfoRow label="Protocolo" value=state_field(|s| format!("v{}", s.protocol_version)) />
                </div>
                <Show when=move || !caps().is_empty()>
                    <div class="caps">
                        <span class="eyebrow">"Capacidades en este Mac"</span>
                        <div class="chips">
                            {move || {
                                caps()
                                    .into_iter()
                                    .map(|(label, ok)| {
                                        view! {
                                            <Badge tone=if ok { "accent".to_string() } else { "neutral".to_string() } dot=true>
                                                {label}
                                                {if ok { "" } else { " · no disponible" }}
                                            </Badge>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </div>
                </Show>
                {move || {
                    app.helper.with(|h| h.as_ref().and_then(|h| h.error.clone())).filter(|_| installed()).map(|e| {
                        view! { <p class="footnote">{e}</p> }
                    })
                }}
                <div class="card-actions">
                    <Pill variant="ghost" icon="refresh" on_click=move |_| app.refresh_helper()>"Comprobar"</Pill>
                    <Show when=installed>
                        <Pill variant="ghost danger-text" icon="trash" disabled=app.helper_busy on_click=move |_| confirm_uninstall.set(true)>
                            "Desinstalar"
                        </Pill>
                    </Show>
                    <Pill variant="primary" icon="shield" busy=app.helper_busy on_click=move |_| app.install_helper()>
                        {move || if installed() { "Reinstalar" } else { "Instalar asistente" }}
                    </Pill>
                </div>
            </section>

            <section class="card">
                <header class="card-head">
                    <span class="card-icon"><Icon name="lock" size=16 /></span>
                    <div class="card-head-text">
                        <h3 class="card-title">"Acceso total al disco"</h3>
                        <p class="card-sub">"Privacidad y seguridad"</p>
                    </div>
                </header>
                <p class="card-text">
                    "Sin este permiso, macOS oculta carpetas como Mail, Mensajes, Safari o Contenedores, y el análisis de almacenamiento y limpieza queda incompleto. iclear nunca envía datos fuera de tu Mac."
                </p>
                <ol class="steps">
                    <li>"Abre Ajustes del Sistema › Privacidad y seguridad › Acceso total al disco."</li>
                    <li>"Activa iclear en la lista (o agrégala con el botón +)."</li>
                    <li>"Vuelve a abrir iclear para aplicar el cambio."</li>
                </ol>
                <div class="card-actions">
                    <Pill variant="primary" icon="external" on_click=open_fda>"Abrir Ajustes del Sistema"</Pill>
                </div>
            </section>

            <Card title="Este Mac" subtitle="Hardware y sistema" icon="info">
                <div class="info-list">
                    <InfoRow label="Nombre" value=machine(|m| m.hostname.clone()) />
                    <InfoRow label="Modelo" value=machine(|m| m.model.clone()) />
                    <InfoRow label="Chip" value=machine(|m| m.chip.clone()) />
                    <InfoRow label="Sistema" value=machine(|m| m.os_version.clone()) />
                    <InfoRow label="Núcleos" value=machine(|m| m.cpu_cores.to_string()) />
                    <InfoRow label="Memoria" value=machine(|m| fmt::mem(m.mem_total)) />
                </div>
            </Card>

            <section class="card about">
                <header class="card-head">
                    <Logo />
                    <div class="card-head-text">
                        <h3 class="card-title">"Acerca de iclear"</h3>
                        <p class="card-sub">{format!("Versión {}", env!("CARGO_PKG_VERSION"))}</p>
                    </div>
                </header>
                <p class="card-text">
                    "Monitor y optimizador para macOS en Apple Silicon. Todo se procesa localmente: no hay cuentas, telemetría ni conexiones a servidores externos."
                </p>
                <div class="info-list">
                    <InfoRow label="Registro de borrados" value="~/Library/Logs/iclear/deletions.log".to_string() />
                    <InfoRow label="Licencia" value="MIT".to_string() />
                    <InfoRow label="Interfaz" value="Leptos + Tauri 2".to_string() />
                </div>
            </section>
        </div>

        <ConfirmDialog
            open=confirm_uninstall
            title="¿Desinstalar el asistente?".to_string()
            confirm_label="Desinstalar".to_string()
            danger=true
            busy=app.helper_busy
            on_confirm=move |_| {
                confirm_uninstall.set(false);
                app.uninstall_helper();
            }
            on_cancel=move |_| confirm_uninstall.set(false)
        >
            <p class="dialog-text">
                "Los ventiladores vuelven a modo automático y se elimina el límite de carga. Podrás reinstalarlo cuando quieras; macOS pedirá tu contraseña."
            </p>
        </ConfirmDialog>
    }
}
