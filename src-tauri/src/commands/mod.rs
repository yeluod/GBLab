use std::sync::Arc;

use tauri::State;

use crate::{
    app_state::AppState,
    dto::{
        AppInfoDto, BatchDeviceDraftDto, BatchOperationAcceptedDto, CommandErrorDto,
        DeviceSnapshotDto, DeviceUpdateDraftDto, SimulatedChannelDto, SipServiceConfigurationDto,
    },
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
    let _operation = state
        .try_operation()
        .ok_or_else(CommandErrorDto::operation_busy)?;
    if state.registration.is_active() {
        return Err(CommandErrorDto::registration_active());
    }
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

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri command 参数提取器要求按值接收 State"
)]
pub fn get_device_snapshot(
    state: State<'_, AppState>,
) -> Result<DeviceSnapshotDto, CommandErrorDto> {
    let core = state
        .core
        .read()
        .map_err(|_| CommandErrorDto::state_unavailable())?;
    let snapshot = core.device_snapshot();
    drop(core);
    Ok(DeviceSnapshotDto::from_core(snapshot))
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri command 参数提取器要求按值接收 State"
)]
pub fn get_device_channels(
    device_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<SimulatedChannelDto>, CommandErrorDto> {
    let core = state
        .core
        .read()
        .map_err(|_| CommandErrorDto::state_unavailable())?;
    let channels = core
        .device_channels(&device_id)
        .map_err(|error| CommandErrorDto::from_core(&error))?;
    drop(core);
    Ok(channels.into_iter().map(Into::into).collect())
}

#[tauri::command]
pub async fn add_devices_in_batch(
    draft: BatchDeviceDraftDto,
    state: State<'_, AppState>,
) -> Result<DeviceSnapshotDto, CommandErrorDto> {
    let _operation = state
        .try_operation()
        .ok_or_else(CommandErrorDto::operation_busy)?;
    if state.registration.is_active() {
        return Err(CommandErrorDto::registration_active());
    }
    let core = Arc::clone(&state.core);
    let draft = draft.into_core();
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = core
            .write()
            .map_err(|_| CommandErrorDto::state_unavailable())?;
        let snapshot = core
            .add_devices_in_batch(draft)
            .map_err(|error| CommandErrorDto::from_core(&error))?;
        drop(core);
        Ok(DeviceSnapshotDto::from_core(snapshot))
    })
    .await
    .map_err(|_| CommandErrorDto::task_failed())?
}

#[tauri::command]
pub async fn clear_devices(
    state: State<'_, AppState>,
) -> Result<DeviceSnapshotDto, CommandErrorDto> {
    let _operation = state
        .try_operation()
        .ok_or_else(CommandErrorDto::operation_busy)?;
    if state.registration.is_active() {
        return Err(CommandErrorDto::registration_active());
    }
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = core
            .write()
            .map_err(|_| CommandErrorDto::state_unavailable())?;
        let snapshot = core
            .clear_devices()
            .map_err(|error| CommandErrorDto::from_core(&error))?;
        drop(core);
        Ok(DeviceSnapshotDto::from_core(snapshot))
    })
    .await
    .map_err(|_| CommandErrorDto::task_failed())?
}

#[tauri::command]
pub async fn update_device(
    device_id: String,
    draft: DeviceUpdateDraftDto,
    state: State<'_, AppState>,
) -> Result<DeviceSnapshotDto, CommandErrorDto> {
    let _operation = state
        .try_operation()
        .ok_or_else(CommandErrorDto::operation_busy)?;
    if state.registration.is_active() {
        return Err(CommandErrorDto::registration_active());
    }
    let core = Arc::clone(&state.core);
    let draft = draft.into_core();
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = core
            .write()
            .map_err(|_| CommandErrorDto::state_unavailable())?;
        let snapshot = core
            .update_device(&device_id, draft)
            .map_err(|error| CommandErrorDto::from_core(&error))?;
        drop(core);
        Ok(DeviceSnapshotDto::from_core(snapshot))
    })
    .await
    .map_err(|_| CommandErrorDto::task_failed())?
}

#[tauri::command]
pub async fn delete_device(
    device_id: String,
    state: State<'_, AppState>,
) -> Result<DeviceSnapshotDto, CommandErrorDto> {
    let _operation = state
        .try_operation()
        .ok_or_else(CommandErrorDto::operation_busy)?;
    if state.registration.is_active() {
        return Err(CommandErrorDto::registration_active());
    }
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = core
            .write()
            .map_err(|_| CommandErrorDto::state_unavailable())?;
        let snapshot = core
            .delete_device(&device_id)
            .map_err(|error| CommandErrorDto::from_core(&error))?;
        drop(core);
        Ok(DeviceSnapshotDto::from_core(snapshot))
    })
    .await
    .map_err(|_| CommandErrorDto::task_failed())?
}

#[tauri::command]
pub async fn register_all_devices(
    state: State<'_, AppState>,
) -> Result<BatchOperationAcceptedDto, CommandErrorDto> {
    let _operation = state
        .try_operation()
        .ok_or_else(CommandErrorDto::operation_busy)?;
    let (configuration, devices) = {
        let core = state
            .core
            .read()
            .map_err(|_| CommandErrorDto::state_unavailable())?;
        (
            core.sip_service_configuration(),
            core.device_snapshot().devices,
        )
    };
    let configuration = configuration
        .normalize_and_validate()
        .map_err(|error| CommandErrorDto::invalid_configuration(error.to_string()))?;
    if configuration.transport != gblab_core::SipTransport::Udp {
        return Err(CommandErrorDto::invalid_configuration(
            "当前真实设备注册仅支持 UDP 传输，请将 SIP 服务传输协议设置为 UDP。".to_owned(),
        ));
    }
    state
        .registration
        .register_all(
            configuration,
            devices,
            gblab_core::runtime::RuntimeLimits::default().device_start_concurrency,
        )
        .await
        .map(Into::into)
        .map_err(|error| CommandErrorDto::registration(&error))
}

#[tauri::command]
pub async fn stop_all_device_registration(
    state: State<'_, AppState>,
) -> Result<BatchOperationAcceptedDto, CommandErrorDto> {
    let _operation = state
        .try_operation()
        .ok_or_else(CommandErrorDto::operation_busy)?;
    state
        .registration
        .stop_all()
        .await
        .map(Into::into)
        .map_err(|error| CommandErrorDto::registration(&error))
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri command 参数提取器要求按值接收 State"
)]
pub fn get_registration_snapshot(
    state: State<'_, AppState>,
) -> gblab_core::runtime::RegistrationSnapshot {
    state.registration.snapshot()
}
