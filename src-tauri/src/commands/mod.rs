use std::sync::Arc;

use tauri::State;

use crate::{
    app_state::AppState,
    dto::{AppInfoDto, CommandErrorDto, SipServiceConfigurationDto},
};

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri command 参数提取器要求按值接收 State"
)]
pub fn get_app_info(state: State<'_, AppState>) -> Result<AppInfoDto, CommandErrorDto> {
    let core = state
        .core
        .read()
        .map_err(|_| CommandErrorDto::state_unavailable())?;
    Ok(AppInfoDto::from_core(core.info()))
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri command 参数提取器要求按值接收 State"
)]
pub fn get_sip_service_configuration(
    state: State<'_, AppState>,
) -> Result<SipServiceConfigurationDto, CommandErrorDto> {
    let core = state
        .core
        .read()
        .map_err(|_| CommandErrorDto::state_unavailable())?;
    Ok(SipServiceConfigurationDto::from_core(
        core.sip_service_configuration(),
    ))
}

#[tauri::command]
pub async fn save_sip_service_configuration(
    configuration: SipServiceConfigurationDto,
    state: State<'_, AppState>,
) -> Result<SipServiceConfigurationDto, CommandErrorDto> {
    let core = Arc::clone(&state.core);
    let configuration = configuration.into_core();

    tauri::async_runtime::spawn_blocking(move || {
        let mut core = core
            .write()
            .map_err(|_| CommandErrorDto::state_unavailable())?;
        let saved = core
            .save_sip_service_configuration(configuration)
            .map_err(|error| CommandErrorDto::from_core(&error))?;
        drop(core);
        Ok(SipServiceConfigurationDto::from_core(saved))
    })
    .await
    .map_err(|_| CommandErrorDto::task_failed())?
}
