use std::time::Duration;

use siprs::siprs_gb28181_xml::Message as GbMessage;
use tokio::{
    sync::{broadcast, mpsc, oneshot, watch},
    time::interval,
};
use tokio_util::sync::CancellationToken;

use crate::{
    SimulatedDevice, SipServiceConfiguration,
    runtime::{PlatformCommandType, PlatformRequest, PlatformRequestMethod, SubscriptionSnapshot},
    sip::{SipLogDirection, SipTransportEvent, parse_gb_xml},
};

use super::types::{
    AlarmTrigger, BatchOperationAccepted, DeviceControlAction, DeviceRegistrationSnapshot,
    DeviceRegistrationStatus, InteractionDirection, InteractionLog, PtzAction, RegistrationEvent,
    RegistrationOperationStatus, RegistrationRuntimeError, RegistrationSnapshot,
};
use super::{
    operations::run_registration_operation,
    state::{MAX_INTERACTION_LOGS, SupervisorState},
    time::now_millis,
};

pub(super) const COMMAND_QUEUE_CAPACITY: usize = 32;
pub(super) const INTERNAL_EVENT_QUEUE_CAPACITY: usize = 4_096;
pub(super) const EVENT_BROADCAST_CAPACITY: usize = 64;
const EVENT_FLUSH_INTERVAL: Duration = Duration::from_millis(50);

pub(super) enum RegistrationCommand {
    RegisterAll {
        configuration: SipServiceConfiguration,
        devices: Vec<SimulatedDevice>,
        concurrency: usize,
        reply: oneshot::Sender<Result<BatchOperationAccepted, RegistrationRuntimeError>>,
    },
    StopAll {
        reply: oneshot::Sender<Result<BatchOperationAccepted, RegistrationRuntimeError>>,
    },
    TriggerAlarm {
        alarm: AlarmTrigger,
        reply: oneshot::Sender<Result<(), RegistrationRuntimeError>>,
    },
    TriggerMobilePosition {
        device_id: String,
        channel_id: String,
        longitude: f64,
        latitude: f64,
        reply: oneshot::Sender<Result<(), RegistrationRuntimeError>>,
    },
    DeviceControl {
        device_id: String,
        action: DeviceControlAction,
        reply: oneshot::Sender<Result<(), RegistrationRuntimeError>>,
    },
    PtzControl {
        device_id: String,
        channel_id: String,
        action: PtzAction,
        reply: oneshot::Sender<Result<(), RegistrationRuntimeError>>,
    },
    GetDeviceStates {
        reply: oneshot::Sender<Vec<DeviceRegistrationSnapshot>>,
    },
}

pub(super) enum InternalEvent {
    DeviceState {
        device_id: String,
        status: DeviceRegistrationStatus,
        last_error: Option<String>,
        expires_at: Option<u64>,
    },
    Sip(SipTransportEvent),
    Heartbeat {
        device_id: String,
        success: bool,
        timestamp: u64,
    },
    ControlState {
        device_id: String,
        action: Option<String>,
        ptz_action: Option<String>,
        guarded: Option<bool>,
        alarm_active: Option<bool>,
    },
    SubscriptionNotification {
        device_id: String,
        channel_id: Option<String>,
        command_type: PlatformCommandType,
        success: bool,
        error: Option<String>,
        timestamp: u64,
    },
    InitialSettled,
    OperationFinished,
    BusinessChannel(mpsc::Sender<BusinessCommand>),
    DroppedLog,
}

pub(super) enum BusinessCommand {
    Alarm {
        alarm: AlarmTrigger,
        subscription: SubscriptionSnapshot,
        reply: oneshot::Sender<Result<(), RegistrationRuntimeError>>,
    },
    MobilePosition {
        device_id: String,
        channel_id: String,
        longitude: f64,
        latitude: f64,
        subscription: SubscriptionSnapshot,
        reply: oneshot::Sender<Result<(), RegistrationRuntimeError>>,
    },
    DeviceControl {
        device_id: String,
        action: DeviceControlAction,
        reply: oneshot::Sender<Result<(), RegistrationRuntimeError>>,
    },
    PtzControl {
        device_id: String,
        channel_id: String,
        action: PtzAction,
        reply: oneshot::Sender<Result<(), RegistrationRuntimeError>>,
    },
    SubscriptionNotify {
        device_id: String,
        channel_id: Option<String>,
        command_type: PlatformCommandType,
        subscription: SubscriptionSnapshot,
        reply: Option<oneshot::Sender<Result<(), RegistrationRuntimeError>>>,
    },
}

pub(super) async fn run_supervisor(
    mut command_rx: mpsc::Receiver<RegistrationCommand>,
    mut internal_rx: mpsc::Receiver<InternalEvent>,
    internal_tx: mpsc::Sender<InternalEvent>,
    snapshot_tx: watch::Sender<RegistrationSnapshot>,
    event_tx: broadcast::Sender<RegistrationEvent>,
    shutdown: CancellationToken,
) {
    let mut state = SupervisorState::new();
    let mut flush_interval = interval(EVENT_FLUSH_INTERVAL);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => {
                if let Some(cancellation) = state.operation_cancellation.take() {
                    cancellation.cancel();
                }
                break;
            }
            command = command_rx.recv() => {
                let Some(command) = command else { break };
                handle_command(command, &mut state, &internal_tx);
            }
            event = internal_rx.recv() => {
                let Some(event) = event else { break };
                handle_internal_event(event, &mut state);
            }
            _ = flush_interval.tick() => {
                flush_events(&mut state, &snapshot_tx, &event_tx);
            }
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "注册和业务命令在同一所有者状态机内串行处理"
)]
fn handle_command(
    command: RegistrationCommand,
    state: &mut SupervisorState,
    internal_tx: &mpsc::Sender<InternalEvent>,
) {
    match command {
        RegistrationCommand::RegisterAll {
            configuration,
            devices,
            concurrency,
            reply,
        } => {
            if devices.is_empty() {
                let _ = reply.send(Err(RegistrationRuntimeError::NoDevices));
                return;
            }
            if state.snapshot.operation_status != RegistrationOperationStatus::Idle {
                let _ = reply.send(Err(RegistrationRuntimeError::AlreadyRunning));
                return;
            }
            let operation_id = state.next_operation_id.to_string();
            state.next_operation_id = state.next_operation_id.saturating_add(1);
            state.snapshot.operation_status = RegistrationOperationStatus::Registering;
            state.snapshot.operation_id = Some(operation_id.clone());
            state.operation_total = devices.len();
            state.initial_settled = 0;
            state.devices = devices
                .iter()
                .map(|device| {
                    let device_id = device.id.to_string();
                    (
                        device_id.clone(),
                        DeviceRegistrationSnapshot {
                            device_id,
                            status: DeviceRegistrationStatus::Queued,
                            last_error: None,
                            expires_at: None,
                            last_platform_request_at: None,
                            last_heartbeat_at: None,
                            online: false,
                            heartbeat_failures: 0,
                            last_control_action: None,
                            ptz_action: None,
                            guarded: false,
                            alarm_active: false,
                        },
                    )
                })
                .collect();
            state.snapshot_dirty = true;
            let cancellation = CancellationToken::new();
            state.operation_cancellation = Some(cancellation.clone());
            tokio::spawn(run_registration_operation(
                configuration,
                devices,
                concurrency,
                cancellation,
                internal_tx.clone(),
            ));
            let _ = reply.send(Ok(BatchOperationAccepted {
                operation_id,
                total: state.operation_total,
            }));
        }
        RegistrationCommand::StopAll { reply } => {
            if state.snapshot.operation_status == RegistrationOperationStatus::Idle {
                let _ = reply.send(Err(RegistrationRuntimeError::NotRunning));
                return;
            }
            state.snapshot.operation_status = RegistrationOperationStatus::Stopping;
            state.snapshot_dirty = true;
            if let Some(cancellation) = state.operation_cancellation.as_ref() {
                cancellation.cancel();
            }
            let _ = reply.send(Ok(BatchOperationAccepted {
                operation_id: state.snapshot.operation_id.clone().unwrap_or_default(),
                total: state.operation_total,
            }));
        }
        RegistrationCommand::TriggerAlarm { alarm, reply } => {
            let Some(tx) = state.business_tx.clone() else {
                let _ = reply.send(Err(RegistrationRuntimeError::BusinessUnavailable));
                return;
            };
            let Some(subscription) = state.subscriptions.next_notify(
                &alarm.device_id,
                Some(&alarm.channel_id),
                PlatformCommandType::Alarm,
                now_millis(),
            ) else {
                let _ = reply.send(Err(RegistrationRuntimeError::MissingActiveSubscription(
                    "Alarm",
                )));
                return;
            };
            let command = BusinessCommand::Alarm {
                alarm,
                subscription,
                reply,
            };
            if let Err(error) = tx.try_send(command) {
                reject_business_command(error.into_inner());
            }
        }
        RegistrationCommand::TriggerMobilePosition {
            device_id,
            channel_id,
            longitude,
            latitude,
            reply,
        } => {
            let Some(tx) = state.business_tx.clone() else {
                let _ = reply.send(Err(RegistrationRuntimeError::BusinessUnavailable));
                return;
            };
            let Some(subscription) = state.subscriptions.next_notify(
                &device_id,
                Some(&channel_id),
                PlatformCommandType::MobilePosition,
                now_millis(),
            ) else {
                let _ = reply.send(Err(RegistrationRuntimeError::MissingActiveSubscription(
                    "Mobile Position",
                )));
                return;
            };
            let command = BusinessCommand::MobilePosition {
                device_id,
                channel_id,
                longitude,
                latitude,
                subscription,
                reply,
            };
            if let Err(error) = tx.try_send(command) {
                reject_business_command(error.into_inner());
            }
        }
        RegistrationCommand::DeviceControl {
            device_id,
            action,
            reply,
        } => {
            let Some(tx) = state.business_tx.clone() else {
                let _ = reply.send(Err(RegistrationRuntimeError::BusinessUnavailable));
                return;
            };
            let command = BusinessCommand::DeviceControl {
                device_id,
                action,
                reply,
            };
            if let Err(error) = tx.try_send(command) {
                reject_business_command(error.into_inner());
            }
        }
        RegistrationCommand::PtzControl {
            device_id,
            channel_id,
            action,
            reply,
        } => {
            let Some(tx) = state.business_tx.clone() else {
                let _ = reply.send(Err(RegistrationRuntimeError::BusinessUnavailable));
                return;
            };
            let command = BusinessCommand::PtzControl {
                device_id,
                channel_id,
                action,
                reply,
            };
            if let Err(error) = tx.try_send(command) {
                reject_business_command(error.into_inner());
            }
        }
        RegistrationCommand::GetDeviceStates { reply } => {
            let _ = reply.send(state.devices.values().cloned().collect());
        }
    }
}

fn reject_business_command(command: BusinessCommand) {
    let reply = match command {
        BusinessCommand::Alarm { reply, .. }
        | BusinessCommand::MobilePosition { reply, .. }
        | BusinessCommand::DeviceControl { reply, .. }
        | BusinessCommand::PtzControl { reply, .. } => reply,
        BusinessCommand::SubscriptionNotify { reply, .. } => {
            if let Some(reply) = reply {
                let _ = reply.send(Err(RegistrationRuntimeError::Unavailable));
            }
            return;
        }
    };
    let _ = reply.send(Err(RegistrationRuntimeError::Unavailable));
}

#[expect(
    clippy::too_many_lines,
    reason = "平台请求、日志和运行时状态必须在同一所有者内顺序更新"
)]
fn handle_internal_event(event: InternalEvent, state: &mut SupervisorState) {
    match event {
        InternalEvent::DeviceState {
            device_id,
            status,
            last_error,
            expires_at,
        } => {
            if let Some(device) = state.devices.get_mut(&device_id) {
                device.status = status;
                device.last_error = last_error;
                device.expires_at = expires_at;
                match status {
                    DeviceRegistrationStatus::Registered => device.online = true,
                    DeviceRegistrationStatus::Unregistered | DeviceRegistrationStatus::Failed => {
                        device.online = false;
                    }
                    DeviceRegistrationStatus::Queued
                    | DeviceRegistrationStatus::Registering
                    | DeviceRegistrationStatus::Unregistering => {}
                }
                state.snapshot_dirty = true;
            }
        }
        InternalEvent::Sip(event) => {
            if event.is_request {
                let request = PlatformRequest {
                    method: match event.method.as_deref() {
                        Some("MESSAGE") => PlatformRequestMethod::Message,
                        Some("SUBSCRIBE") => PlatformRequestMethod::Subscribe,
                        Some("NOTIFY") => PlatformRequestMethod::Notify,
                        Some("OPTIONS") => PlatformRequestMethod::Options,
                        _ => PlatformRequestMethod::Unknown,
                    },
                    command_type: match event.command_type.as_deref().or(event.event.as_deref()) {
                        Some("Catalog" | "catalog" | "presence") => PlatformCommandType::Catalog,
                        Some("DeviceInfo") => PlatformCommandType::DeviceInfo,
                        Some("DeviceStatus") => PlatformCommandType::DeviceStatus,
                        Some("DeviceControl") => PlatformCommandType::DeviceControl,
                        Some("Alarm" | "alarm") => PlatformCommandType::Alarm,
                        Some("MobilePosition" | "mobile-position") => {
                            PlatformCommandType::MobilePosition
                        }
                        Some("Keepalive") => PlatformCommandType::Keepalive,
                        _ => PlatformCommandType::Unknown,
                    },
                    device_id: (!event.device_id.is_empty()).then_some(event.device_id.clone()),
                    channel_id: event.channel_id.clone(),
                    sn: xml_sn(&event.message),
                    call_id: event.call_id.clone(),
                    expires: event.expires,
                    from_tag: event.from_tag.clone(),
                    local_tag: event.local_tag.clone(),
                    event: event.event.clone(),
                    request_uri: event.request_uri.clone(),
                    response_body: None,
                    initial_notify_body: None,
                };
                if request.method == PlatformRequestMethod::Subscribe {
                    let now = now_millis();
                    if request.expires == Some(0) {
                        state.subscriptions.cancel(&request);
                    } else if state.subscriptions.subscribe(&request, now).is_some()
                        && should_send_initial_subscription_notify(request.command_type)
                        && let Some(subscription) = state.subscriptions.next_notify(
                            &event.device_id,
                            event.channel_id.as_deref(),
                            request.command_type,
                            now,
                        )
                        && let Some(tx) = state.business_tx.clone()
                    {
                        let command = BusinessCommand::SubscriptionNotify {
                            device_id: event.device_id.clone(),
                            channel_id: event.channel_id.clone(),
                            command_type: request.command_type,
                            subscription,
                            reply: None,
                        };
                        if let Err(error) = tx.try_send(command) {
                            reject_business_command(error.into_inner());
                        }
                    }
                    state.snapshot_dirty = true;
                }
                if let Some(device) = state.devices.get_mut(&event.device_id) {
                    device.last_platform_request_at = Some(event.timestamp_millis);
                    device.online = true;
                    if request.command_type == PlatformCommandType::Keepalive {
                        device.last_heartbeat_at = Some(event.timestamp_millis);
                        device.heartbeat_failures = 0;
                    }
                    state.snapshot_dirty = true;
                }
            }
            let log = InteractionLog {
                sequence: state.next_log_sequence,
                timestamp: event.timestamp_millis,
                device_id: event.device_id,
                channel_id: event.channel_id,
                direction: match event.direction {
                    SipLogDirection::Send => InteractionDirection::Send,
                    SipLogDirection::Receive => InteractionDirection::Receive,
                },
                message: event.message,
            };
            state.next_log_sequence = state.next_log_sequence.saturating_add(1);
            if state.logs.len() == MAX_INTERACTION_LOGS {
                state.logs.pop_front();
            }
            state.logs.push_back(log.clone());
            state.pending_logs.push(log);
        }
        InternalEvent::Heartbeat {
            device_id,
            success,
            timestamp,
        } => {
            if let Some(device) = state.devices.get_mut(&device_id) {
                if success {
                    device.last_heartbeat_at = Some(timestamp);
                    device.heartbeat_failures = 0;
                    device.online = true;
                } else {
                    device.heartbeat_failures = device.heartbeat_failures.saturating_add(1);
                    if device.heartbeat_failures >= 3 {
                        device.online = false;
                    }
                }
                state.snapshot_dirty = true;
            }
        }
        InternalEvent::ControlState {
            device_id,
            action,
            ptz_action,
            guarded,
            alarm_active,
        } => {
            if let Some(device) = state.devices.get_mut(&device_id) {
                if let Some(action) = action {
                    device.last_control_action = Some(action);
                }
                if ptz_action.is_some() {
                    device.ptz_action = ptz_action;
                }
                if guarded.is_some() {
                    device.guarded = guarded.unwrap_or(false);
                }
                if alarm_active.is_some() {
                    device.alarm_active = alarm_active.unwrap_or(false);
                }
                state.snapshot_dirty = true;
            }
        }
        InternalEvent::SubscriptionNotification {
            device_id,
            channel_id,
            command_type,
            success,
            error,
            timestamp,
        } => {
            if success {
                state.subscriptions.mark_notified(
                    &device_id,
                    channel_id.as_deref(),
                    command_type,
                    timestamp,
                );
            } else if let Some(error) = error {
                state.subscriptions.mark_failed(
                    &device_id,
                    channel_id.as_deref(),
                    command_type,
                    error,
                );
            }
            state.snapshot_dirty = true;
        }
        InternalEvent::InitialSettled => {
            state.initial_settled = state.initial_settled.saturating_add(1);
            if state.initial_settled >= state.operation_total
                && state.snapshot.operation_status == RegistrationOperationStatus::Registering
            {
                state.snapshot.operation_status = RegistrationOperationStatus::Running;
                state.snapshot_dirty = true;
            }
        }
        InternalEvent::OperationFinished => {
            state.snapshot.operation_status = RegistrationOperationStatus::Idle;
            state.snapshot.operation_id = None;
            state.operation_cancellation = None;
            state.operation_total = 0;
            state.initial_settled = 0;
            state.business_tx = None;
            state.subscriptions.clear();
            state.snapshot_dirty = true;
        }
        InternalEvent::BusinessChannel(tx) => state.business_tx = Some(tx),
        InternalEvent::DroppedLog => {
            state.dropped_logs = state.dropped_logs.saturating_add(1);
            state.snapshot_dirty = true;
        }
    }
}

const fn should_send_initial_subscription_notify(command_type: PlatformCommandType) -> bool {
    matches!(command_type, PlatformCommandType::Catalog)
}

fn flush_events(
    state: &mut SupervisorState,
    snapshot_tx: &watch::Sender<RegistrationSnapshot>,
    event_tx: &broadcast::Sender<RegistrationEvent>,
) {
    let before = state.subscriptions.snapshots();
    state.subscriptions.expire(now_millis());
    if before
        .iter()
        .zip(state.subscriptions.snapshots().iter())
        .any(|(previous, current)| previous.status != current.status)
    {
        state.snapshot_dirty = true;
    }
    if state.snapshot_dirty {
        let snapshot = state.build_snapshot();
        state.snapshot = snapshot.clone();
        snapshot_tx.send_replace(snapshot.clone());
        let _ = event_tx.send(RegistrationEvent::Snapshot(snapshot));
        let _ = event_tx.send(RegistrationEvent::DeviceStates(
            state.devices.values().cloned().collect(),
        ));
        let _ = event_tx.send(RegistrationEvent::Subscriptions(
            state.subscriptions.snapshots(),
        ));
        state.snapshot_dirty = false;
    }
    if !state.pending_logs.is_empty() {
        let logs = std::mem::take(&mut state.pending_logs);
        let _ = event_tx.send(RegistrationEvent::InteractionLogs(logs));
    }
}

fn xml_sn(message: &str) -> Option<String> {
    let body = message
        .split_once("\r\n\r\n")
        .or_else(|| message.split_once("\n\n"))
        .map_or(message, |(_, body)| body)
        .trim();
    let parsed = parse_gb_xml(body).ok()?;
    let sn = match parsed {
        GbMessage::Query(query) => query.sn,
        GbMessage::Response(response) => response.sn,
        GbMessage::Control(control) => control.sn,
        GbMessage::Notify(notify) => notify.sn,
        GbMessage::CascadingRegister(register) => register.sn,
    };
    Some(sn.to_string())
}

#[cfg(test)]
mod tests {
    use crate::runtime::PlatformCommandType;

    use super::should_send_initial_subscription_notify;

    #[test]
    fn only_catalog_subscription_should_send_an_initial_notify() {
        assert!(should_send_initial_subscription_notify(
            PlatformCommandType::Catalog
        ));
        assert!(!should_send_initial_subscription_notify(
            PlatformCommandType::Alarm
        ));
        assert!(!should_send_initial_subscription_notify(
            PlatformCommandType::MobilePosition
        ));
    }
}
