//! `GBLab` 的 Tauri 桌面壳。

#![deny(missing_docs)]

mod app_state;
mod commands;
mod dto;

use app_state::AppState;
use gblab_core::CoreService;
use tauri::Manager;

/// 启动 `GBLab` 桌面应用。
///
/// # Errors
///
/// Tauri 运行时无法初始化或桌面事件循环异常退出时返回错误。
pub fn run() -> Result<(), tauri::Error> {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let configuration_path = app_data_dir.join("gblab.config.json");
            let core = CoreService::open(&configuration_path)?;
            app.manage(AppState::new(core));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_sip_service_configuration,
            commands::save_sip_service_configuration,
            commands::get_device_snapshot,
            commands::get_device_channels,
            commands::add_devices_in_batch,
            commands::update_device,
            commands::delete_device
        ])
        .run(tauri::generate_context!())
}
