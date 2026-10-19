//! Tauri shell: owns the proxy, forwards flow events to the UI in batches and
//! exposes commands for inspection, export and system integration.

mod commands;
mod state;

use std::time::Duration;

use proxyzip_core::FlowEvent;
use tauri::{Emitter, Manager, RunEvent};
use tokio::sync::broadcast::error::RecvError;

use crate::state::AppState;

pub const EVENTS: &str = "proxyzip://events";
pub const RESYNC: &str = "proxyzip://resync";

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_env("PROXYZIP_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = tauri::async_runtime::block_on(AppState::init())?;
            let mut rx = state.store.subscribe();
            app.manage(state);
            let handle = app.handle().clone();
            // Batch events: emitting one IPC message per flow update would choke
            // the webview at a few thousand requests per second.
            tauri::async_runtime::spawn(async move {
                let mut batch: Vec<FlowEvent> = Vec::new();
                let mut tick = tokio::time::interval(Duration::from_millis(60));
                loop {
                    tokio::select! {
                        ev = rx.recv() => match ev {
                            Ok(e) => batch.push(e),
                            Err(RecvError::Lagged(_)) => {
                                batch.clear();
                                let _ = handle.emit(RESYNC, ());
                            }
                            Err(RecvError::Closed) => break,
                        },
                        _ = tick.tick() => {
                            if !batch.is_empty() {
                                let _ = handle.emit(EVENTS, std::mem::take(&mut batch));
                            }
                        }
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_flows,
            commands::get_flow_detail,
            commands::clear_flows,
            commands::remove_flows,
            commands::set_pinned,
            commands::set_comment,
            commands::get_status,
            commands::set_recording,
            commands::restart_proxy,
            commands::set_system_proxy,
            commands::install_ca,
            commands::uninstall_ca,
            commands::regenerate_ca,
            commands::install_ca_ios_simulator,
            commands::get_settings,
            commands::update_settings,
            commands::copy_as_curl,
            commands::export_har,
            commands::import_har,
            commands::terminal_env,
            commands::repeat_flow,
            commands::send_request,
            commands::parse_curl,
            commands::get_rules,
            commands::save_rules,
            commands::throttle_presets,
            commands::resolve_breakpoint,
            commands::release_breakpoints,
            commands::list_protos,
            commands::import_proto,
            commands::remove_proto,
            commands::code_languages,
            commands::generate_code,
            commands::query_body,
            commands::save_session,
            commands::open_session,
            commands::recent_sessions,
            commands::forget_recent_session,
            commands::export_csv,
            commands::export_postman,
            commands::test_script,
            commands::ssl_proxying_toggle_host,
        ])
        .build(tauri::generate_context!())
        .expect("error while building ProxyZip");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            // Never leave the machine pointing at a proxy that no longer exists.
            if let Some(state) = handle.try_state::<AppState>() {
                state.restore_system_proxy();
                state.save_last_session();
            }
        }
    });
}
