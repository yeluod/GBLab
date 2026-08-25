use tauri::State;

use crate::{app_state::AppState, dto::AppInfoDto};

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri command 参数提取器要求按值接收 State"
)]
pub fn get_app_info(state: State<'_, AppState>) -> AppInfoDto {
    AppInfoDto::from_core(state.core.info())
}
