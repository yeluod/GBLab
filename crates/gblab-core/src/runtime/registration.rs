use std::{
    collections::{BTreeMap, VecDeque},
    fmt::Write,
    future::Future,
    sync::Arc,
    time::{Duration, SystemTime},
};

use serde::Serialize;
use thiserror::Error;
use tokio::{
    sync::{Semaphore, broadcast, mpsc, oneshot, watch},
    task::JoinSet,
    time::{interval, sleep},
};
use tokio_util::sync::CancellationToken;

use crate::{
    SimulatedDevice, SipServiceConfiguration,
    runtime::scheduler::{Scheduler, SchedulerTick},
    runtime::{
        PlatformCommandType, PlatformRequest, PlatformRequestMethod, SubscriptionManager,
        SubscriptionSnapshot,
    },
    sip::{
        DeviceSipSession, SipLogDirection, SipRegistrationClient, SipRegistrationError,
        SipTransportEvent,
    },
};

const COMMAND_QUEUE_CAPACITY: usize = 32;
const INTERNAL_EVENT_QUEUE_CAPACITY: usize = 4_096;
const EVENT_BROADCAST_CAPACITY: usize = 64;
const MAX_INTERACTION_LOGS: usize = 10_000;
const EVENT_FLUSH_INTERVAL: Duration = Duration::from_millis(50);
const REGISTRATION_ATTEMPTS: u8 = 3;
const RETRY_CYCLE_DELAY: Duration = Duration::from_secs(30);

/// 单台设备当前的注册状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceRegistrationStatus {
    /// 尚未发起注册。
    Unregistered,
    /// 已进入有界注册队列。
    Queued,
    /// 正在完成 REGISTER 事务或刷新。
    Registering,
    /// 平台已经返回成功响应。
    Registered,
    /// 正在发送 Expires 为 0 的 REGISTER。
    Unregistering,
    /// 最近一次注册或注销失败。
    Failed,
}

/// 全量注册运行时的操作状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RegistrationOperationStatus {
    /// 没有运行中的注册资源。
    Idle,
    /// 正在完成首轮全量注册。
    Registering,
    /// 首轮注册完成，正在维持注册与自动刷新。
    Running,
    /// 正在全量注销并释放资源。
    Stopping,
}

/// 单台设备注册状态快照。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRegistrationSnapshot {
    /// 设备国标编号。
    pub device_id: String,
    /// 当前注册状态。
    pub status: DeviceRegistrationStatus,
    /// 最近一次失败原因。
    pub last_error: Option<String>,
    /// 注册有效时间点，Unix 毫秒。
    pub expires_at: Option<u64>,
    /// 最近一次收到平台请求的时间，Unix 毫秒。
    pub last_platform_request_at: Option<u64>,
    /// 最近一次收到 Keepalive 的时间，Unix 毫秒。
    pub last_heartbeat_at: Option<u64>,
    /// 当前是否被判定为在线。
    pub online: bool,
    /// 连续心跳失败次数。
    pub heartbeat_failures: u32,
    /// 最近一次设备控制动作。
    pub last_control_action: Option<String>,
    /// 当前 PTZ 动作。
    pub ptz_action: Option<String>,
    /// 当前是否处于布防状态。
    pub guarded: bool,
    /// 当前是否处于报警状态。
    pub alarm_active: bool,
}

/// SIP 交互方向。
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InteractionDirection {
    /// 模拟器发送到平台。
    Send,
    /// 模拟器从平台接收。
    Receive,
}

/// 内存中的原始 SIP 交互日志。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionLog {
    /// 单次运行内递增的日志序号。
    pub sequence: u64,
    /// 发生时间，Unix 毫秒。
    pub timestamp: u64,
    /// 设备国标编号。
    pub device_id: String,
    /// 通道编号；注册事务不属于具体通道。
    pub channel_id: Option<String>,
    /// 消息方向。
    pub direction: InteractionDirection,
    /// 完整原始 SIP 报文。
    pub message: String,
}

/// 注册运行时完整内存快照。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationSnapshot {
    /// 当前全量操作状态。
    pub operation_status: RegistrationOperationStatus,
    /// 当前操作编号。
    pub operation_id: Option<String>,
    /// 各设备当前注册状态。
    pub devices: Vec<DeviceRegistrationSnapshot>,
    /// 当前内存日志窗口。
    pub interaction_logs: Vec<InteractionLog>,
    /// 当前平台订阅运行时快照。
    pub subscriptions: Vec<SubscriptionSnapshot>,
}

impl Default for RegistrationSnapshot {
    fn default() -> Self {
        Self {
            operation_status: RegistrationOperationStatus::Idle,
            operation_id: None,
            devices: Vec::new(),
            interaction_logs: Vec::new(),
            subscriptions: Vec::new(),
        }
    }
}

/// 已接收的全量操作。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchOperationAccepted {
    /// 操作编号。
    pub operation_id: String,
    /// 本次操作涉及的设备数。
    pub total: usize,
}

/// 注册运行时向桌面层发布的批量事件。
#[derive(Clone, Debug)]
pub enum RegistrationEvent {
    /// 降频后的注册快照。
    Snapshot(RegistrationSnapshot),
    /// 一批完整原始 SIP 日志。
    InteractionLogs(Vec<InteractionLog>),
}

/// 注册运行时命令失败。
#[derive(Clone, Debug, Error)]
pub enum RegistrationRuntimeError {
    /// 没有可注册的设备。
    #[error("当前没有可注册的设备")]
    NoDevices,
    /// 已经存在注册生命周期。
    #[error("全量注册生命周期已经在运行")]
    AlreadyRunning,
    /// 当前没有可停止的注册生命周期。
    #[error("当前没有运行中的注册生命周期")]
    NotRunning,
    /// 命令队列已经关闭。
    #[error("注册运行时不可用")]
    Unavailable,
    /// 业务触发时设备会话不存在或运行时不可用。
    #[error("设备未注册或业务运行时不可用")]
    BusinessUnavailable,
    /// 业务 SIP 事务已完成，但平台返回了失败状态或传输失败。
    #[error("业务 SIP 事务失败: {0}")]
    BusinessFailed(String),
}

/// 可模拟的设备控制动作。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceControlAction {
    /// 远程重启。
    Restart,
    /// 布防。
    Guard,
    /// 撤防。
    Unguard,
    /// 报警复位。
    AlarmReset,
}

impl DeviceControlAction {
    const fn as_xml(self) -> &'static str {
        match self {
            Self::Restart => "DeviceRestart",
            Self::Guard => "Guard",
            Self::Unguard => "ResetGuard",
            Self::AlarmReset => "AlarmReset",
        }
    }
}

/// 可模拟的 PTZ 动作。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PtzAction {
    /// 向上移动。
    Up,
    /// 向下移动。
    Down,
    /// 向左移动。
    Left,
    /// 向右移动。
    Right,
    /// 放大。
    ZoomIn,
    /// 缩小。
    ZoomOut,
    /// 停止。
    Stop,
}

impl PtzAction {
    const fn as_xml(self) -> &'static str {
        match self {
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::ZoomIn => "ZoomIn",
            Self::ZoomOut => "ZoomOut",
            Self::Stop => "Stop",
        }
    }
}

/// 注册运行时的克隆句柄。
#[derive(Clone)]
pub struct RegistrationHandle {
    command_tx: mpsc::Sender<RegistrationCommand>,
    snapshot_rx: watch::Receiver<RegistrationSnapshot>,
    event_tx: broadcast::Sender<RegistrationEvent>,
    shutdown: CancellationToken,
}

impl RegistrationHandle {
    /// 创建注册句柄和待调度的单所有者监督器。
    ///
    /// 调用方必须将返回的 `Future` 提交给自身的异步运行时执行。
    pub fn prepare() -> (Self, impl Future<Output = ()> + Send + 'static) {
        let (command_tx, command_rx) = mpsc::channel(COMMAND_QUEUE_CAPACITY);
        let (internal_tx, internal_rx) = mpsc::channel(INTERNAL_EVENT_QUEUE_CAPACITY);
        let (snapshot_tx, snapshot_rx) = watch::channel(RegistrationSnapshot::default());
        let (event_tx, _) = broadcast::channel(EVENT_BROADCAST_CAPACITY);
        let shutdown = CancellationToken::new();
        let supervisor = run_supervisor(
            command_rx,
            internal_rx,
            internal_tx,
            snapshot_tx,
            event_tx.clone(),
            shutdown.clone(),
        );
        (
            Self {
                command_tx,
                snapshot_rx,
                event_tx,
                shutdown,
            },
            supervisor,
        )
    }

    /// 发起全量注册并立即返回操作回执。
    ///
    /// # Errors
    ///
    /// 没有设备、已有运行中操作或运行时不可用时返回错误。
    pub async fn register_all(
        &self,
        configuration: SipServiceConfiguration,
        devices: Vec<SimulatedDevice>,
        concurrency: usize,
    ) -> Result<BatchOperationAccepted, RegistrationRuntimeError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(RegistrationCommand::RegisterAll {
                configuration,
                devices,
                concurrency: concurrency.max(1),
                reply: reply_tx,
            })
            .await
            .map_err(|_| RegistrationRuntimeError::Unavailable)?;
        reply_rx
            .await
            .map_err(|_| RegistrationRuntimeError::Unavailable)?
    }

    /// 发起全量停止注册并立即返回操作回执。
    ///
    /// # Errors
    ///
    /// 当前没有运行中的注册生命周期或运行时不可用时返回错误。
    pub async fn stop_all(&self) -> Result<BatchOperationAccepted, RegistrationRuntimeError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(RegistrationCommand::StopAll { reply: reply_tx })
            .await
            .map_err(|_| RegistrationRuntimeError::Unavailable)?;
        reply_rx
            .await
            .map_err(|_| RegistrationRuntimeError::Unavailable)?
    }

    /// 向指定设备通道发送一次 Alarm 通知。
    ///
    /// # Errors
    ///
    /// 设备未注册或业务运行时不可用时返回错误。
    pub async fn trigger_alarm(
        &self,
        device_id: String,
        channel_id: String,
        alarm_type: String,
        description: String,
    ) -> Result<(), RegistrationRuntimeError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(RegistrationCommand::TriggerAlarm {
                device_id,
                channel_id,
                alarm_type,
                description,
                reply: reply_tx,
            })
            .await
            .map_err(|_| RegistrationRuntimeError::Unavailable)?;
        reply_rx
            .await
            .map_err(|_| RegistrationRuntimeError::Unavailable)?
    }

    /// 向指定设备通道发送一次移动位置通知。
    ///
    /// # Errors
    ///
    /// 设备未注册或业务运行时不可用时返回错误。
    pub async fn trigger_mobile_position(
        &self,
        device_id: String,
        channel_id: String,
        longitude: f64,
        latitude: f64,
    ) -> Result<(), RegistrationRuntimeError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(RegistrationCommand::TriggerMobilePosition {
                device_id,
                channel_id,
                longitude,
                latitude,
                reply: reply_tx,
            })
            .await
            .map_err(|_| RegistrationRuntimeError::Unavailable)?;
        reply_rx
            .await
            .map_err(|_| RegistrationRuntimeError::Unavailable)?
    }

    /// 向指定设备发送一次设备控制命令。
    ///
    /// # Errors
    ///
    /// 设备未注册或业务运行时不可用时返回错误。
    pub async fn control_device(
        &self,
        device_id: String,
        action: DeviceControlAction,
    ) -> Result<(), RegistrationRuntimeError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(RegistrationCommand::DeviceControl {
                device_id,
                action,
                reply: reply_tx,
            })
            .await
            .map_err(|_| RegistrationRuntimeError::Unavailable)?;
        reply_rx
            .await
            .map_err(|_| RegistrationRuntimeError::Unavailable)?
    }

    /// 向指定通道发送一次 PTZ 控制命令。
    ///
    /// # Errors
    ///
    /// 设备未注册或业务运行时不可用时返回错误。
    pub async fn control_ptz(
        &self,
        device_id: String,
        channel_id: String,
        action: PtzAction,
    ) -> Result<(), RegistrationRuntimeError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(RegistrationCommand::PtzControl {
                device_id,
                channel_id,
                action,
                reply: reply_tx,
            })
            .await
            .map_err(|_| RegistrationRuntimeError::Unavailable)?;
        reply_rx
            .await
            .map_err(|_| RegistrationRuntimeError::Unavailable)?
    }

    /// 返回当前内存快照。
    #[must_use]
    pub fn snapshot(&self) -> RegistrationSnapshot {
        self.snapshot_rx.borrow().clone()
    }

    /// 返回运行时是否占用设备与 SIP 配置。
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.snapshot().operation_status != RegistrationOperationStatus::Idle
    }

    /// 订阅降频快照和批量日志事件。
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<RegistrationEvent> {
        self.event_tx.subscribe()
    }
}

impl Drop for RegistrationHandle {
    fn drop(&mut self) {
        if self.command_tx.strong_count() == 1 {
            self.shutdown.cancel();
        }
    }
}

enum RegistrationCommand {
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
        device_id: String,
        channel_id: String,
        alarm_type: String,
        description: String,
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
}

enum InternalEvent {
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
}

enum BusinessCommand {
    Alarm {
        device_id: String,
        channel_id: String,
        alarm_type: String,
        description: String,
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

struct SupervisorState {
    snapshot: RegistrationSnapshot,
    devices: BTreeMap<String, DeviceRegistrationSnapshot>,
    logs: VecDeque<InteractionLog>,
    operation_cancellation: Option<CancellationToken>,
    operation_total: usize,
    initial_settled: usize,
    next_operation_id: u64,
    next_log_sequence: u64,
    snapshot_dirty: bool,
    pending_logs: Vec<InteractionLog>,
    business_tx: Option<mpsc::Sender<BusinessCommand>>,
    subscriptions: SubscriptionManager,
}

impl SupervisorState {
    fn new() -> Self {
        Self {
            snapshot: RegistrationSnapshot::default(),
            devices: BTreeMap::new(),
            logs: VecDeque::with_capacity(MAX_INTERACTION_LOGS),
            operation_cancellation: None,
            operation_total: 0,
            initial_settled: 0,
            next_operation_id: 1,
            next_log_sequence: 1,
            snapshot_dirty: false,
            pending_logs: Vec::new(),
            business_tx: None,
            subscriptions: SubscriptionManager::default(),
        }
    }

    fn build_snapshot(&self) -> RegistrationSnapshot {
        RegistrationSnapshot {
            operation_status: self.snapshot.operation_status,
            operation_id: self.snapshot.operation_id.clone(),
            devices: self.devices.values().cloned().collect(),
            interaction_logs: self.logs.iter().cloned().collect(),
            subscriptions: self.subscriptions.snapshots(),
        }
    }
}

async fn run_supervisor(
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
        RegistrationCommand::TriggerAlarm {
            device_id,
            channel_id,
            alarm_type,
            description,
            reply,
        } => {
            let Some(tx) = state.business_tx.clone() else {
                let _ = reply.send(Err(RegistrationRuntimeError::BusinessUnavailable));
                return;
            };
            let Some(subscription) = state.subscriptions.next_notify(
                &device_id,
                Some(&channel_id),
                PlatformCommandType::Alarm,
                now_millis(),
            ) else {
                let _ = reply.send(Err(RegistrationRuntimeError::BusinessUnavailable));
                return;
            };
            let command = BusinessCommand::Alarm {
                device_id,
                channel_id,
                alarm_type,
                description,
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
                let _ = reply.send(Err(RegistrationRuntimeError::BusinessUnavailable));
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
                        Some("RecordInfo") => PlatformCommandType::RecordInfo,
                        Some("Alarm" | "alarm") => PlatformCommandType::Alarm,
                        Some("MobilePosition" | "mobile-position") => {
                            PlatformCommandType::MobilePosition
                        }
                        Some("Keepalive") => PlatformCommandType::Keepalive,
                        _ => PlatformCommandType::Unknown,
                    },
                    device_id: (!event.device_id.is_empty()).then_some(event.device_id.clone()),
                    channel_id: event.channel_id.clone(),
                    sn: extract_xml_value(&event.message, "SN"),
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
    }
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
        let event_snapshot = RegistrationSnapshot {
            interaction_logs: Vec::new(),
            ..snapshot
        };
        let _ = event_tx.send(RegistrationEvent::Snapshot(event_snapshot));
        state.snapshot_dirty = false;
    }
    if !state.pending_logs.is_empty() {
        let logs = std::mem::take(&mut state.pending_logs);
        let _ = event_tx.send(RegistrationEvent::InteractionLogs(logs));
    }
}

async fn run_registration_operation(
    configuration: SipServiceConfiguration,
    devices: Vec<SimulatedDevice>,
    concurrency: usize,
    cancellation: CancellationToken,
    internal_tx: mpsc::Sender<InternalEvent>,
) {
    let (transport_event_tx, mut transport_event_rx) = mpsc::channel(INTERNAL_EVENT_QUEUE_CAPACITY);
    let client = match SipRegistrationClient::connect(&configuration, transport_event_tx).await {
        Ok(client) => client,
        Err(error) => {
            for device in devices {
                send_device_state(
                    &internal_tx,
                    device.id.as_str(),
                    DeviceRegistrationStatus::Failed,
                    Some(error.to_string()),
                    None,
                )
                .await;
                let _ = internal_tx.send(InternalEvent::InitialSettled).await;
            }
            let _ = internal_tx.send(InternalEvent::OperationFinished).await;
            return;
        }
    };

    let transport_cancellation = CancellationToken::new();
    let scheduler = Scheduler::start(cancellation.clone());
    let catalog_devices = Arc::new(
        devices
            .iter()
            .map(|device| (device.id.to_string(), device.clone()))
            .collect::<std::collections::HashMap<_, _>>(),
    );
    let receiver_task = tokio::spawn(
        Arc::clone(&client)
            .receive_loop(transport_cancellation.clone(), Arc::clone(&catalog_devices)),
    );
    let transport_forward_tx = internal_tx.clone();
    let transport_forward_task = tokio::spawn(async move {
        while let Some(event) = transport_event_rx.recv().await {
            if transport_forward_tx
                .send(InternalEvent::Sip(event))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let (business_tx, business_rx) = mpsc::channel(32);
    let _ = internal_tx
        .send(InternalEvent::BusinessChannel(business_tx))
        .await;

    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut sessions = JoinSet::new();
    let session_map = Arc::new(tokio::sync::Mutex::new(BTreeMap::new()));
    for device in devices {
        let session_map = Arc::clone(&session_map);
        sessions.spawn(run_device_lifecycle(
            device.id.to_string(),
            configuration.clone(),
            Arc::clone(&client),
            Arc::clone(&semaphore),
            cancellation.clone(),
            internal_tx.clone(),
            session_map,
            scheduler.subscribe(),
        ));
    }
    let business_task = tokio::spawn(run_business_commands(
        business_rx,
        Arc::clone(&session_map),
        Arc::clone(&client),
        Arc::clone(&catalog_devices),
        configuration.clone(),
        cancellation.clone(),
        internal_tx.clone(),
    ));
    while sessions.join_next().await.is_some() {}

    transport_cancellation.cancel();
    let _ = receiver_task.await;
    drop(client);
    let _ = transport_forward_task.await;
    scheduler.join().await;
    business_task.abort();
    let _ = internal_tx.send(InternalEvent::OperationFinished).await;
}

#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "注册生命周期需要共享运行时资源与状态通道"
)]
async fn run_device_lifecycle(
    device_id: String,
    configuration: SipServiceConfiguration,
    client: Arc<SipRegistrationClient>,
    semaphore: Arc<Semaphore>,
    cancellation: CancellationToken,
    internal_tx: mpsc::Sender<InternalEvent>,
    session_map: SessionMap,
    mut scheduler_rx: broadcast::Receiver<SchedulerTick>,
) {
    let session = match DeviceSipSession::new(device_id.clone(), &configuration, &client) {
        Ok(session) => session,
        Err(error) => {
            send_device_state(
                &internal_tx,
                &device_id,
                DeviceRegistrationStatus::Failed,
                Some(error.to_string()),
                None,
            )
            .await;
            let _ = internal_tx.send(InternalEvent::InitialSettled).await;
            return;
        }
    };
    let mut initial_settled = false;
    let session = Arc::new(session);
    session_map
        .lock()
        .await
        .insert(device_id.clone(), Arc::clone(&session));
    loop {
        send_device_state(
            &internal_tx,
            &device_id,
            DeviceRegistrationStatus::Registering,
            None,
            None,
        )
        .await;
        let result =
            register_with_retry(&session, &client, &configuration, &semaphore, &cancellation).await;
        if !initial_settled {
            initial_settled = true;
            let _ = internal_tx.send(InternalEvent::InitialSettled).await;
        }
        match result {
            Ok(expires) => {
                let refresh_after = refresh_delay(expires);
                send_device_state(
                    &internal_tx,
                    &device_id,
                    DeviceRegistrationStatus::Registered,
                    None,
                    Some(now_millis().saturating_add(duration_millis(expires))),
                )
                .await;
                let refresh_at =
                    now_millis().saturating_add(duration_millis_u64(refresh_after.as_secs()));
                let keepalive_interval_millis =
                    duration_millis(configuration.keepalive_interval.max(1));
                let mut next_keepalive_at = now_millis().saturating_add(keepalive_interval_millis);
                loop {
                    let tick = tokio::select! {
                        () = cancellation.cancelled() => break,
                        tick = scheduler_rx.recv() => match tick {
                            Ok(tick) => tick,
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break,
                        },
                    };
                    if tick.now_millis >= refresh_at {
                        break;
                    }
                    if tick.now_millis >= next_keepalive_at {
                        let body = format!(
                            "<Notify><CmdType>Keepalive</CmdType><SN>1</SN><DeviceID>{device_id}</DeviceID><Status>OK</Status><Info>OK</Info></Notify>"
                        );
                        let success = session
                            .send_message(&client, body, &cancellation, None)
                            .await
                            .is_ok();
                        let _ = internal_tx
                            .send(InternalEvent::Heartbeat {
                                device_id: device_id.clone(),
                                success,
                                timestamp: tick.now_millis,
                            })
                            .await;
                        next_keepalive_at =
                            tick.now_millis.saturating_add(keepalive_interval_millis);
                    }
                }
            }
            Err(SipRegistrationError::Cancelled) => break,
            Err(error) => {
                send_device_state(
                    &internal_tx,
                    &device_id,
                    DeviceRegistrationStatus::Failed,
                    Some(error.to_string()),
                    None,
                )
                .await;
                tokio::select! {
                    () = cancellation.cancelled() => break,
                    () = sleep(RETRY_CYCLE_DELAY) => {}
                }
            }
        }
    }

    send_device_state(
        &internal_tx,
        &device_id,
        DeviceRegistrationStatus::Unregistering,
        None,
        None,
    )
    .await;
    let unregister_cancellation = CancellationToken::new();
    let unregister_result = if let Ok(permit) = semaphore.acquire().await {
        let result = session
            .unregister(&client, &configuration, &unregister_cancellation)
            .await;
        drop(permit);
        result
    } else {
        Err(SipRegistrationError::Cancelled)
    };
    match unregister_result {
        Ok(()) => {
            send_device_state(
                &internal_tx,
                &device_id,
                DeviceRegistrationStatus::Unregistered,
                None,
                None,
            )
            .await;
        }
        Err(error) => {
            send_device_state(
                &internal_tx,
                &device_id,
                DeviceRegistrationStatus::Failed,
                Some(format!("注销失败: {error}")),
                None,
            )
            .await;
        }
    }
    session_map.lock().await.remove(&device_id);
}

#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "设备业务命令需要统一串行发送并投影运行时状态"
)]
async fn run_business_commands(
    mut rx: mpsc::Receiver<BusinessCommand>,
    sessions: SessionMap,
    client: Arc<SipRegistrationClient>,
    catalog_devices: Arc<std::collections::HashMap<String, SimulatedDevice>>,
    configuration: SipServiceConfiguration,
    cancellation: CancellationToken,
    internal_tx: mpsc::Sender<InternalEvent>,
) {
    while let Some(command) = tokio::select! {
        command = rx.recv() => command,
        () = cancellation.cancelled() => None,
    } {
        match command {
            BusinessCommand::Alarm {
                device_id,
                channel_id,
                alarm_type,
                description,
                subscription,
                reply,
            } => {
                let result = send_business_notify(&sessions, &client, cancellation.clone(), &device_id, Some(&channel_id), &subscription,
                    format!("<Notify><CmdType>Alarm</CmdType><SN>1</SN><DeviceID>{channel_id}</DeviceID><AlarmMethod>1</AlarmMethod><AlarmType>{alarm_type}</AlarmType><AlarmDescription>{description}</AlarmDescription></Notify>")).await;
                let notification_error = result.as_ref().err().map(ToString::to_string);
                let _ = internal_tx
                    .send(InternalEvent::SubscriptionNotification {
                        device_id: device_id.clone(),
                        channel_id: Some(channel_id.clone()),
                        command_type: PlatformCommandType::Alarm,
                        success: result.is_ok(),
                        error: notification_error,
                        timestamp: now_millis(),
                    })
                    .await;
                if result.is_ok() {
                    let _ = internal_tx
                        .send(InternalEvent::ControlState {
                            device_id: device_id.clone(),
                            action: None,
                            ptz_action: None,
                            guarded: None,
                            alarm_active: Some(true),
                        })
                        .await;
                }
                let _ = reply.send(result);
            }
            BusinessCommand::MobilePosition {
                device_id,
                channel_id,
                longitude,
                latitude,
                subscription,
                reply,
            } => {
                let result = send_business_notify(&sessions, &client, cancellation.clone(), &device_id, Some(&channel_id), &subscription,
                    format!("<Notify><CmdType>MobilePosition</CmdType><SN>1</SN><DeviceID>{channel_id}</DeviceID><Longitude>{longitude}</Longitude><Latitude>{latitude}</Latitude></Notify>")).await;
                let notification_error = result.as_ref().err().map(ToString::to_string);
                let _ = internal_tx
                    .send(InternalEvent::SubscriptionNotification {
                        device_id: device_id.clone(),
                        channel_id: Some(channel_id.clone()),
                        command_type: PlatformCommandType::MobilePosition,
                        success: result.is_ok(),
                        error: notification_error,
                        timestamp: now_millis(),
                    })
                    .await;
                let _ = reply.send(result);
            }
            BusinessCommand::DeviceControl {
                device_id,
                action,
                reply,
            } => {
                let result = send_business_message(
                    &sessions,
                    &client,
                    &configuration,
                    cancellation.clone(),
                    &device_id,
                    None,
                    format!(
                        "<Control><CmdType>DeviceControl</CmdType><SN>1</SN><DeviceID>{device_id}</DeviceID><Type>{}</Type></Control>",
                        action.as_xml()
                    ),
                )
                .await;
                if result.is_ok() {
                    let _ = internal_tx
                        .send(InternalEvent::ControlState {
                            device_id: device_id.clone(),
                            action: Some(action.as_xml().to_owned()),
                            ptz_action: None,
                            guarded: match action {
                                DeviceControlAction::Guard => Some(true),
                                DeviceControlAction::Unguard | DeviceControlAction::AlarmReset => {
                                    Some(false)
                                }
                                DeviceControlAction::Restart => None,
                            },
                            alarm_active: matches!(action, DeviceControlAction::AlarmReset)
                                .then_some(false),
                        })
                        .await;
                }
                let _ = reply.send(result);
            }
            BusinessCommand::PtzControl {
                device_id,
                channel_id,
                action,
                reply,
            } => {
                let result = send_business_message(
                    &sessions,
                    &client,
                    &configuration,
                    cancellation.clone(),
                    &device_id,
                    Some(&channel_id),
                    format!(
                        "<Control><CmdType>DeviceControl</CmdType><SN>1</SN><DeviceID>{channel_id}</DeviceID><PTZCmd>{}</PTZCmd></Control>",
                        action.as_xml()
                    ),
                )
                .await;
                if result.is_ok() {
                    let _ = internal_tx
                        .send(InternalEvent::ControlState {
                            device_id: device_id.clone(),
                            action: None,
                            ptz_action: Some(action.as_xml().to_owned()),
                            guarded: None,
                            alarm_active: None,
                        })
                        .await;
                }
                let _ = reply.send(result);
            }
            BusinessCommand::SubscriptionNotify {
                device_id,
                channel_id,
                command_type,
                subscription,
                reply,
            } => {
                let result = send_subscription_notify(
                    &sessions,
                    &client,
                    &catalog_devices,
                    cancellation.clone(),
                    &device_id,
                    channel_id.as_deref(),
                    command_type,
                    &subscription,
                )
                .await;
                let notification_error = result.as_ref().err().map(ToString::to_string);
                let _ = internal_tx
                    .send(InternalEvent::SubscriptionNotification {
                        device_id: device_id.clone(),
                        channel_id: channel_id.clone(),
                        command_type,
                        success: result.is_ok(),
                        error: notification_error,
                        timestamp: now_millis(),
                    })
                    .await;
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            }
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "订阅通知发送需要完整的订阅、设备和传输上下文"
)]
async fn send_subscription_notify(
    sessions: &SessionMap,
    client: &Arc<SipRegistrationClient>,
    devices: &Arc<std::collections::HashMap<String, SimulatedDevice>>,
    cancellation: CancellationToken,
    device_id: &str,
    channel_id: Option<&str>,
    command_type: PlatformCommandType,
    subscription: &SubscriptionSnapshot,
) -> Result<(), RegistrationRuntimeError> {
    let body_device_id = channel_id.unwrap_or(device_id);
    let body = match command_type {
        PlatformCommandType::Catalog => build_catalog_notify_body(body_device_id, devices),
        PlatformCommandType::Alarm => format!(
            "<Notify><CmdType>Alarm</CmdType><SN>1</SN><DeviceID>{body_device_id}</DeviceID><AlarmMethod>1</AlarmMethod><AlarmType>0</AlarmType><AlarmDescription>订阅已建立</AlarmDescription></Notify>"
        ),
        PlatformCommandType::MobilePosition => format!(
            "<Notify><CmdType>MobilePosition</CmdType><SN>1</SN><DeviceID>{body_device_id}</DeviceID><Longitude>0</Longitude><Latitude>0</Latitude></Notify>"
        ),
        _ => return Err(RegistrationRuntimeError::BusinessUnavailable),
    };
    let session = sessions
        .lock()
        .await
        .get(device_id)
        .cloned()
        .ok_or(RegistrationRuntimeError::BusinessUnavailable)?;
    session
        .send_notify(
            client,
            body,
            &cancellation,
            channel_id.map(str::to_owned),
            subscription,
        )
        .await
        .map_err(|error| map_business_error(&error))
}

fn build_catalog_notify_body(
    device_id: &str,
    devices: &std::collections::HashMap<String, SimulatedDevice>,
) -> String {
    let Some(device) = devices.get(device_id) else {
        return format!(
            "<Notify><CmdType>Catalog</CmdType><SN>1</SN><DeviceID>{device_id}</DeviceID><SumNum>0</SumNum><DeviceList Num=\"0\"></DeviceList></Notify>"
        );
    };
    let channels = crate::domain::derive_channels_for_device(device).unwrap_or_default();
    let mut items = format!(
        "<Device><DeviceID>{}</DeviceID><Name>{}</Name><Manufacturer>{}</Manufacturer><Model>{}</Model><Status>ON</Status><ParentID>{}</ParentID></Device>",
        xml_escape(&device.id.to_string()),
        xml_escape(&device.name),
        xml_escape(&device.manufacturer),
        xml_escape(&device.model),
        xml_escape(&device.id.to_string())
    );
    for channel in channels {
        let _ = write!(
            items,
            "<Device><DeviceID>{}</DeviceID><Name>{}</Name><Manufacturer>{}</Manufacturer><Model>{}</Model><Status>ON</Status><ParentID>{}</ParentID></Device>",
            xml_escape(&channel.id.to_string()),
            xml_escape(&channel.name),
            xml_escape(&device.manufacturer),
            xml_escape(&device.model),
            xml_escape(&device.id.to_string())
        );
    }
    let count = device.channel_count as usize + 1;
    format!(
        "<Notify><CmdType>Catalog</CmdType><SN>1</SN><DeviceID>{device_id}</DeviceID><SumNum>{count}</SumNum><DeviceList Num=\"{count}\">{items}</DeviceList></Notify>"
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

type SessionMap = Arc<tokio::sync::Mutex<BTreeMap<String, Arc<DeviceSipSession>>>>;

#[expect(
    clippy::too_many_arguments,
    reason = "业务消息需要完整的设备、通道与传输上下文"
)]
async fn send_business_message(
    sessions: &SessionMap,
    client: &Arc<SipRegistrationClient>,
    _configuration: &SipServiceConfiguration,
    cancellation: CancellationToken,
    device_id: &str,
    channel_id: Option<&str>,
    body: String,
) -> Result<(), RegistrationRuntimeError> {
    let session = sessions
        .lock()
        .await
        .get(device_id)
        .cloned()
        .ok_or(RegistrationRuntimeError::BusinessUnavailable)?;
    session
        .send_message(client, body, &cancellation, channel_id.map(str::to_owned))
        .await
        .map_err(|error| map_business_error(&error))
}

#[expect(
    clippy::too_many_arguments,
    reason = "业务通知发送需要完整的设备、订阅和传输上下文"
)]
async fn send_business_notify(
    sessions: &SessionMap,
    client: &Arc<SipRegistrationClient>,
    cancellation: CancellationToken,
    device_id: &str,
    channel_id: Option<&str>,
    subscription: &SubscriptionSnapshot,
    body: String,
) -> Result<(), RegistrationRuntimeError> {
    let session = sessions
        .lock()
        .await
        .get(device_id)
        .cloned()
        .ok_or(RegistrationRuntimeError::BusinessUnavailable)?;
    session
        .send_notify(
            client,
            body,
            &cancellation,
            channel_id.map(str::to_owned),
            subscription,
        )
        .await
        .map_err(|error| map_business_error(&error))
}

fn map_business_error(error: &SipRegistrationError) -> RegistrationRuntimeError {
    RegistrationRuntimeError::BusinessFailed(error.to_string())
}

async fn register_with_retry(
    session: &DeviceSipSession,
    client: &SipRegistrationClient,
    configuration: &SipServiceConfiguration,
    semaphore: &Semaphore,
    cancellation: &CancellationToken,
) -> Result<u32, SipRegistrationError> {
    let mut backoff = Duration::from_secs(1);
    let mut last_error = SipRegistrationError::Timeout;
    for attempt in 0..REGISTRATION_ATTEMPTS {
        let permit = tokio::select! {
            () = cancellation.cancelled() => return Err(SipRegistrationError::Cancelled),
            permit = semaphore.acquire() => permit.map_err(|_| SipRegistrationError::Cancelled)?,
        };
        let result = session.register(client, configuration, cancellation).await;
        drop(permit);
        match result {
            Ok(expires) => return Ok(expires),
            Err(SipRegistrationError::Rejected { code, reason }) if code == 403 => {
                return Err(SipRegistrationError::Rejected { code, reason });
            }
            Err(SipRegistrationError::Cancelled) => {
                return Err(SipRegistrationError::Cancelled);
            }
            Err(error) => last_error = error,
        }
        if attempt + 1 < REGISTRATION_ATTEMPTS {
            tokio::select! {
                () = cancellation.cancelled() => return Err(SipRegistrationError::Cancelled),
                () = sleep(backoff) => {}
            }
            backoff = backoff.saturating_mul(2);
        }
    }
    Err(last_error)
}

async fn send_device_state(
    internal_tx: &mpsc::Sender<InternalEvent>,
    device_id: &str,
    status: DeviceRegistrationStatus,
    last_error: Option<String>,
    expires_at: Option<u64>,
) {
    let _ = internal_tx
        .send(InternalEvent::DeviceState {
            device_id: device_id.to_owned(),
            status,
            last_error,
            expires_at,
        })
        .await;
}

fn refresh_delay(expires: u32) -> Duration {
    Duration::from_secs(
        u64::from(expires)
            .saturating_mul(4)
            .saturating_div(5)
            .max(1),
    )
}

fn duration_millis(seconds: u32) -> u64 {
    u64::from(seconds).saturating_mul(1_000)
}

const fn duration_millis_u64(seconds: u64) -> u64 {
    seconds.saturating_mul(1_000)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

fn extract_xml_value(message: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = message.find(&open)? + open.len();
    let end = message[start..].find(&close)? + start;
    Some(message[start..end].trim().to_owned())
}
