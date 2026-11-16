//! One module per sidebar section.

pub mod apps;
pub mod battery;
pub mod cleanup;
pub mod cloud;
pub mod fans;
pub mod memory;
pub mod optimize;
pub mod panel;
pub mod processes;
pub mod settings;
pub mod storage;

use iclear_protocol::{DiskInfo, SystemSnapshot};

/// The boot volume ("/"), falling back to the first disk.
pub fn root_disk(s: &SystemSnapshot) -> Option<&DiskInfo> {
    s.disks.iter().find(|d| d.mount_point == "/").or(s.disks.first())
}

/// Page heading with optional description, shown above the content.
#[leptos::component]
pub fn PageIntro(
    #[prop(into)] text: String,
    #[prop(optional)] children: Option<leptos::children::Children>,
) -> impl leptos::IntoView {
    use leptos::prelude::*;
    view! {
        <div class="page-intro">
            <p class="page-desc">{text}</p>
            <div class="page-intro-actions">{children.map(|c| c())}</div>
        </div>
    }
}
