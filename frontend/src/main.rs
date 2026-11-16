//! iclear frontend: Leptos (CSR) UI compiled to WASM with Trunk.
//!
//! Talks to the Tauri backend through `api` (which falls back to mock data
//! when running in a plain browser via `trunk serve`).

mod api;
mod app;
mod components;
mod fmt;
mod mock;
mod mock_cloud;
mod pages;
mod state;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
