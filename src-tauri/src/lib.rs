//! `GBLab` 的 Tauri 桌面壳。

#![deny(missing_docs)]

mod app_state;
mod commands;
mod dto;

use std::fs;

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
            fs::create_dir_all(&app_data_dir)?;
            let database_path = app_data_dir.join("gblab.db");
            let core = tauri::async_runtime::block_on(CoreService::open(&database_path))?;
            app.manage(AppState::new(core));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::get_app_info])
        .run(tauri::generate_context!())
}
