#![expect(
    clippy::needless_pass_by_value,
    reason = "Tauri command extractors require owned values for IPC arguments"
)]

use std::sync::Arc;

use serde::Deserialize;
use tauri::{State, ipc::Response};

use crate::{
    app_state::AppState,
    dto::{
        AppInfoDto, BatchDeviceDraftDto, BatchOperationAcceptedDto, CommandErrorDto, DevicePageDto,
        DeviceSnapshotDto, DeviceUpdateDraftDto, MediaRuntimeStatusDto, Mp4ProbeResultDto,
        SimulatedChannelDto, SipServiceConfigurationDto, VideoCaptureCapabilitiesDto,
        VideoEncoderCapabilitiesDto,
    },
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmTriggerDto {
    device_id: String,
    channel_id: String,
    alarm_priority: String,
    alarm_method: String,
    alarm_type: String,
    alarm_status: String,
    description: String,
    longitude: f64,
    latitude: f64,
}

impl AlarmTriggerDto {
    fn into_core(self) -> gblab_core::runtime::AlarmTrigger {
        gblab_core::runtime::AlarmTrigger {
            device_id: self.device_id,
            channel_id: self.channel_id,
            alarm_priority: self.alarm_priority,
            alarm_method: self.alarm_method,
            alarm_type: self.alarm_type,
            alarm_status: self.alarm_status,
            description: self.description,
            longitude: self.longitude,
            latitude: self.latitude,
        }
    }
}

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
pub fn probe_mp4(file_path: String) -> Result<Mp4ProbeResultDto, CommandErrorDto> {
    gblab_core::probe_mp4(std::path::Path::new(&file_path))
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

#[tauri::command]
pub fn open_mp4(
    file_path: String,
    looping: bool,
    state: State<'_, AppState>,
) -> Result<MediaRuntimeStatusDto, CommandErrorDto> {
    state
        .media
        .handle()
        .open_mp4(std::path::PathBuf::from(file_path), looping)
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

#[tauri::command]
pub fn open_camera(
    configuration: gblab_core::CameraCaptureSettings,
    state: State<'_, AppState>,
) -> Result<MediaRuntimeStatusDto, CommandErrorDto> {
    state
        .media
        .handle()
        .open_camera(configuration)
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

/// 返回当前平台由 `FFmpeg` 原生设备层枚举出的摄像头和麦克风。
#[tauri::command]
pub fn list_capture_devices() -> Result<crate::dto::CaptureDeviceListsDto, CommandErrorDto> {
    gblab_core::list_capture_devices()
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

/// 返回指定摄像头的原生分辨率和帧率，不打开采集会话。
#[tauri::command]
pub fn get_video_capture_capabilities(
    device_id: String,
) -> Result<VideoCaptureCapabilitiesDto, CommandErrorDto> {
    gblab_core::video_capture_capabilities(&device_id)
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

/// 返回当前 `FFmpeg` Native Libraries 实际提供的视频编码器。
#[tauri::command]
pub fn get_video_encoder_capabilities() -> VideoEncoderCapabilitiesDto {
    gblab_core::video_encoder_capabilities().into()
}

#[tauri::command]
pub fn play_media(state: State<'_, AppState>) -> Result<MediaRuntimeStatusDto, CommandErrorDto> {
    state
        .media
        .handle()
        .play()
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

#[tauri::command]
pub fn attach_media_preview(
    state: State<'_, AppState>,
) -> Result<MediaRuntimeStatusDto, CommandErrorDto> {
    state
        .media
        .handle()
        .attach_preview()
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

#[tauri::command]
pub fn detach_media_preview(
    state: State<'_, AppState>,
) -> Result<MediaRuntimeStatusDto, CommandErrorDto> {
    state
        .media
        .handle()
        .detach_preview()
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

#[tauri::command]
pub fn pause_media(state: State<'_, AppState>) -> Result<MediaRuntimeStatusDto, CommandErrorDto> {
    state
        .media
        .handle()
        .pause()
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

#[tauri::command]
pub fn stop_media(state: State<'_, AppState>) -> Result<MediaRuntimeStatusDto, CommandErrorDto> {
    state
        .media
        .handle()
        .stop()
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

#[tauri::command]
pub fn reset_media(state: State<'_, AppState>) -> Result<MediaRuntimeStatusDto, CommandErrorDto> {
    state
        .media
        .handle()
        .reset()
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

#[tauri::command]
pub fn seek_media(
    position_seconds: f64,
    state: State<'_, AppState>,
) -> Result<MediaRuntimeStatusDto, CommandErrorDto> {
    state
        .media
        .handle()
        .seek(position_seconds)
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

#[tauri::command]
pub fn set_media_playback_rate(
    rate: f64,
    state: State<'_, AppState>,
) -> Result<MediaRuntimeStatusDto, CommandErrorDto> {
    state
        .media
        .handle()
        .set_playback_rate(rate)
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

#[tauri::command]
pub fn set_media_audio_control(
    muted: bool,
    volume: f64,
    state: State<'_, AppState>,
) -> Result<MediaRuntimeStatusDto, CommandErrorDto> {
    state
        .media
        .handle()
        .set_audio_control(muted, volume)
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

#[tauri::command]
pub fn step_media_frame(state: State<'_, AppState>) -> Result<Response, CommandErrorDto> {
    state
        .media
        .handle()
        .step_frame()
        .map(|frame| Response::new(frame.map_or_else(Vec::new, encode_preview_frame)))
        .map_err(|error| CommandErrorDto::media(&error))
}

#[tauri::command]
pub fn get_media_runtime_status(state: State<'_, AppState>) -> MediaRuntimeStatusDto {
    state.media.handle().status().into()
}

#[tauri::command]
pub fn set_media_audio_monitoring(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<MediaRuntimeStatusDto, CommandErrorDto> {
    state
        .media
        .handle()
        .set_audio_monitoring(enabled)
        .map(Into::into)
        .map_err(|error| CommandErrorDto::media(&error))
}

/// Reads one bounded preview frame as raw binary, never as a JSON number array.
#[tauri::command]
pub fn read_media_frame(state: State<'_, AppState>) -> Result<Response, CommandErrorDto> {
    let handle = state.media.handle();
    let status = handle.status();
    if let Some(error) = status.last_error {
        return Err(CommandErrorDto::media(&gblab_core::MediaError::Playback(
            error,
        )));
    }
    if matches!(
        status.source_status,
        gblab_core::media::MediaSourceStatus::Stopped
            | gblab_core::media::MediaSourceStatus::Unconfigured
    ) {
        // Natural MP4 EOF is a normal lifecycle state.  Return an empty
        // payload so the frontend can observe the stopped status without
        // converting it into a synthetic playback error.
        return Ok(Response::new(Vec::new()));
    }
    handle
        .try_preview_frame()
        .map(|frame| Response::new(frame.map_or_else(Vec::new, encode_preview_frame)))
        .map_err(|error| CommandErrorDto::media(&error))
}

fn encode_preview_frame(frame: gblab_core::MediaVideoFrame) -> Vec<u8> {
    const HEADER_SIZE: usize = 21;
    let mut bytes = Vec::with_capacity(HEADER_SIZE.saturating_add(frame.rgba.len()));
    bytes.extend_from_slice(b"GBPF");
    bytes.push(1);
    bytes.extend_from_slice(&frame.width.to_le_bytes());
    bytes.extend_from_slice(&frame.height.to_le_bytes());
    bytes.extend_from_slice(&frame.position_seconds.to_le_bytes());
    bytes.extend_from_slice(&frame.rgba);
    bytes
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
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri command 参数提取器要求按值接收 State"
)]
pub fn get_media_configuration(
    state: State<'_, AppState>,
) -> Result<gblab_core::MediaConfiguration, CommandErrorDto> {
    let core = state
        .core
        .read()
        .map_err(|_| CommandErrorDto::state_unavailable())?;
    Ok(core.media_configuration())
}

#[tauri::command]
pub async fn save_media_configuration(
    configuration: gblab_core::MediaConfiguration,
    state: State<'_, AppState>,
) -> Result<gblab_core::MediaConfiguration, CommandErrorDto> {
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
        core.save_media_configuration(configuration)
            .map_err(|error| CommandErrorDto::from_core(&error))
    })
    .await
    .map_err(|_| CommandErrorDto::task_failed())?
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

/// 按条件分页查询设备配置，避免把完整设备集合绑定到 UI 查询模型。
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri command 参数提取器要求按值接收 State"
)]
pub fn get_device_page(
    offset: usize,
    limit: usize,
    filter: Option<String>,
    sort: Option<String>,
    state: State<'_, AppState>,
) -> Result<DevicePageDto, CommandErrorDto> {
    let core = state
        .core
        .read()
        .map_err(|_| CommandErrorDto::state_unavailable())?;
    Ok(DevicePageDto::from_page(core.device_page(
        offset,
        limit,
        filter.as_deref(),
        sort.as_deref(),
    )))
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
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        let mut core = core
            .write()
            .map_err(|_| CommandErrorDto::state_unavailable())?;
        let snapshot = core
            .add_devices_in_batch(draft)
            .map_err(|error| CommandErrorDto::from_core(&error))?;
        drop(core);
        Ok::<_, CommandErrorDto>(snapshot)
    })
    .await
    .map_err(|_| CommandErrorDto::task_failed())??;
    Ok(DeviceSnapshotDto::from_core(snapshot))
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
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        let mut core = core
            .write()
            .map_err(|_| CommandErrorDto::state_unavailable())?;
        let snapshot = core
            .clear_devices()
            .map_err(|error| CommandErrorDto::from_core(&error))?;
        drop(core);
        Ok::<_, CommandErrorDto>(snapshot)
    })
    .await
    .map_err(|_| CommandErrorDto::task_failed())??;
    Ok(DeviceSnapshotDto::from_core(snapshot))
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
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        let mut core = core
            .write()
            .map_err(|_| CommandErrorDto::state_unavailable())?;
        let snapshot = core
            .update_device(&device_id, draft)
            .map_err(|error| CommandErrorDto::from_core(&error))?;
        drop(core);
        Ok::<_, CommandErrorDto>(snapshot)
    })
    .await
    .map_err(|_| CommandErrorDto::task_failed())??;
    Ok(DeviceSnapshotDto::from_core(snapshot))
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
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        let mut core = core
            .write()
            .map_err(|_| CommandErrorDto::state_unavailable())?;
        let snapshot = core
            .delete_device(&device_id)
            .map_err(|error| CommandErrorDto::from_core(&error))?;
        drop(core);
        Ok::<_, CommandErrorDto>(snapshot)
    })
    .await
    .map_err(|_| CommandErrorDto::task_failed())??;
    Ok(DeviceSnapshotDto::from_core(snapshot))
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
pub async fn trigger_alarm(
    alarm: AlarmTriggerDto,
    state: State<'_, AppState>,
) -> Result<(), CommandErrorDto> {
    state
        .registration
        .trigger_alarm(alarm.into_core())
        .await
        .map_err(|error| CommandErrorDto::registration(&error))
}

#[tauri::command]
pub async fn trigger_mobile_position(
    device_id: String,
    channel_id: String,
    longitude: f64,
    latitude: f64,
    state: State<'_, AppState>,
) -> Result<(), CommandErrorDto> {
    state
        .registration
        .trigger_mobile_position(device_id, channel_id, longitude, latitude)
        .await
        .map_err(|error| CommandErrorDto::registration(&error))
}

#[tauri::command]
pub async fn control_device(
    device_id: String,
    action: String,
    state: State<'_, AppState>,
) -> Result<(), CommandErrorDto> {
    let action = match action.as_str() {
        "restart" => gblab_core::runtime::DeviceControlAction::Restart,
        "guard" => gblab_core::runtime::DeviceControlAction::Guard,
        "unguard" => gblab_core::runtime::DeviceControlAction::Unguard,
        "alarm-reset" => gblab_core::runtime::DeviceControlAction::AlarmReset,
        _ => {
            return Err(CommandErrorDto::invalid_configuration(
                "不支持的设备控制动作".to_owned(),
            ));
        }
    };
    state
        .registration
        .control_device(device_id, action)
        .await
        .map_err(|error| CommandErrorDto::registration(&error))
}

#[tauri::command]
pub async fn control_ptz(
    device_id: String,
    channel_id: String,
    action: String,
    state: State<'_, AppState>,
) -> Result<(), CommandErrorDto> {
    let action = match action.as_str() {
        "up" => gblab_core::runtime::PtzAction::Up,
        "down" => gblab_core::runtime::PtzAction::Down,
        "left" => gblab_core::runtime::PtzAction::Left,
        "right" => gblab_core::runtime::PtzAction::Right,
        "zoom-in" => gblab_core::runtime::PtzAction::ZoomIn,
        "zoom-out" => gblab_core::runtime::PtzAction::ZoomOut,
        "stop" => gblab_core::runtime::PtzAction::Stop,
        _ => {
            return Err(CommandErrorDto::invalid_configuration(
                "不支持的 PTZ 动作".to_owned(),
            ));
        }
    };
    state
        .registration
        .control_ptz(device_id, channel_id, action)
        .await
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

/// 查询当前设备运行态，不把设备列表塞入聚合快照。
#[tauri::command]
pub async fn get_registration_device_states(
    state: State<'_, AppState>,
) -> Result<Vec<gblab_core::runtime::DeviceRegistrationSnapshot>, CommandErrorDto> {
    state
        .registration
        .device_states()
        .await
        .map_err(|error| CommandErrorDto::registration(&error))
}
