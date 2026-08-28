//! `GBLab` 的 Tauri 桌面壳。

#![deny(missing_docs)]

mod app_state;
mod commands;
mod dto;

use app_state::AppState;
use gblab_core::{CoreService, runtime::RegistrationHandle};
use tauri::{Emitter, Manager};

const REGISTRATION_SNAPSHOT_EVENT: &str = "registration-snapshot";
const REGISTRATION_DEVICE_STATES_EVENT: &str = "registration-device-states";
const REGISTRATION_SUBSCRIPTIONS_EVENT: &str = "registration-subscriptions";
const INTERACTION_LOGS_EVENT: &str = "sip-interaction-logs";

/// 启动 `GBLab` 桌面应用。
///
/// # Errors
///
/// Tauri 运行时无法初始化或桌面事件循环异常退出时返回错误。
pub fn run() -> Result<(), tauri::Error> {
    let app = tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let configuration_path = app_data_dir.join("gblab.config.json");
            let core = CoreService::open(&configuration_path)?;
            let (registration, supervisor) = RegistrationHandle::prepare();
            tauri::async_runtime::spawn(supervisor);
            let state = AppState::new(core, registration);
            let mut registration_events = state.registration.subscribe();
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match registration_events.recv().await {
                        Ok(gblab_core::runtime::RegistrationEvent::Snapshot(snapshot)) => {
                            let _ = app_handle.emit(REGISTRATION_SNAPSHOT_EVENT, snapshot);
                        }
                        Ok(gblab_core::runtime::RegistrationEvent::DeviceStates(states)) => {
                            let _ = app_handle.emit(REGISTRATION_DEVICE_STATES_EVENT, states);
                        }
                        Ok(gblab_core::runtime::RegistrationEvent::Subscriptions(
                            subscriptions,
                        )) => {
                            let _ =
                                app_handle.emit(REGISTRATION_SUBSCRIPTIONS_EVENT, subscriptions);
                        }
                        Ok(gblab_core::runtime::RegistrationEvent::InteractionLogs(logs)) => {
                            let _ = app_handle.emit(INTERACTION_LOGS_EVENT, logs);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            app.manage(state);
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::probe_mp4,
            commands::open_mp4,
            commands::open_camera,
            commands::probe_camera,
            commands::play_media,
            commands::pause_media,
            commands::stop_media,
            commands::reset_media,
            commands::get_media_runtime_status,
            commands::read_media_packet,
            commands::get_sip_service_configuration,
            commands::save_sip_service_configuration,
            commands::get_media_configuration,
            commands::save_media_configuration,
            commands::get_device_snapshot,
            commands::get_device_page,
            commands::get_device_channels,
            commands::add_devices_in_batch,
            commands::clear_devices,
            commands::update_device,
            commands::delete_device,
            commands::register_all_devices,
            commands::stop_all_device_registration,
            commands::trigger_alarm,
            commands::trigger_mobile_position,
            commands::control_device,
            commands::control_ptz,
            commands::get_registration_snapshot,
            commands::get_registration_device_states
        ])
        .build(tauri::generate_context!())?;
    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            let state = app_handle.state::<AppState>();
            if state.registration.is_active() && state.begin_shutdown() {
                api.prevent_exit();
                let registration = state.registration.clone();
                let app_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = registration.stop_all().await;
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                        while registration.is_active() {
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        }
                    })
                    .await;
                    app_handle.exit(0);
                });
            }
        }
    });
    Ok(())
}
