use std::{
    collections::{BTreeMap, VecDeque},
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
}

impl Default for RegistrationSnapshot {
    fn default() -> Self {
        Self {
            operation_status: RegistrationOperationStatus::Idle,
            operation_id: None,
            devices: Vec::new(),
            interaction_logs: Vec::new(),
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
    /// 启动单所有者注册运行时。
    #[must_use]
    pub fn start() -> Self {
        let (command_tx, command_rx) = mpsc::channel(COMMAND_QUEUE_CAPACITY);
        let (internal_tx, internal_rx) = mpsc::channel(INTERNAL_EVENT_QUEUE_CAPACITY);
        let (snapshot_tx, snapshot_rx) = watch::channel(RegistrationSnapshot::default());
        let (event_tx, _) = broadcast::channel(EVENT_BROADCAST_CAPACITY);
        let shutdown = CancellationToken::new();
        tokio::spawn(run_supervisor(
            command_rx,
            internal_rx,
            internal_tx,
            snapshot_tx,
            event_tx.clone(),
            shutdown.clone(),
        ));
        Self {
            command_tx,
            snapshot_rx,
            event_tx,
            shutdown,
        }
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
}

enum InternalEvent {
    DeviceState {
        device_id: String,
        status: DeviceRegistrationStatus,
        last_error: Option<String>,
        expires_at: Option<u64>,
    },
    Sip(SipTransportEvent),
    InitialSettled,
    OperationFinished,
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
        }
    }

    fn build_snapshot(&self) -> RegistrationSnapshot {
        RegistrationSnapshot {
            operation_status: self.snapshot.operation_status,
            operation_id: self.snapshot.operation_id.clone(),
            devices: self.devices.values().cloned().collect(),
            interaction_logs: self.logs.iter().cloned().collect(),
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
    }
}

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
                state.snapshot_dirty = true;
            }
        }
        InternalEvent::Sip(event) => {
            let log = InteractionLog {
                sequence: state.next_log_sequence,
                timestamp: event.timestamp_millis,
                device_id: event.device_id,
                channel_id: None,
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
            state.snapshot_dirty = true;
        }
    }
}

fn flush_events(
    state: &mut SupervisorState,
    snapshot_tx: &watch::Sender<RegistrationSnapshot>,
    event_tx: &broadcast::Sender<RegistrationEvent>,
) {
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
    let receiver_task =
        tokio::spawn(Arc::clone(&client).receive_loop(transport_cancellation.clone()));
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

    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut sessions = JoinSet::new();
    for device in devices {
        sessions.spawn(run_device_lifecycle(
            device.id.to_string(),
            configuration.clone(),
            Arc::clone(&client),
            Arc::clone(&semaphore),
            cancellation.clone(),
            internal_tx.clone(),
        ));
    }
    while sessions.join_next().await.is_some() {}

    transport_cancellation.cancel();
    let _ = receiver_task.await;
    drop(client);
    let _ = transport_forward_task.await;
    let _ = internal_tx.send(InternalEvent::OperationFinished).await;
}

async fn run_device_lifecycle(
    device_id: String,
    configuration: SipServiceConfiguration,
    client: Arc<SipRegistrationClient>,
    semaphore: Arc<Semaphore>,
    cancellation: CancellationToken,
    internal_tx: mpsc::Sender<InternalEvent>,
) {
    let mut session = match DeviceSipSession::new(device_id.clone(), &configuration, &client) {
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
    loop {
        send_device_state(
            &internal_tx,
            &device_id,
            DeviceRegistrationStatus::Registering,
            None,
            None,
        )
        .await;
        let result = register_with_retry(
            &mut session,
            &client,
            &configuration,
            &semaphore,
            &cancellation,
        )
        .await;
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
                tokio::select! {
                    () = cancellation.cancelled() => break,
                    () = sleep(refresh_after) => {}
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
}

async fn register_with_retry(
    session: &mut DeviceSipSession,
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

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}
