//! 面向应用层的注册运行时句柄。

use std::future::Future;

use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::{SimulatedDevice, SipServiceConfiguration};

use super::{
    registration::{
        COMMAND_QUEUE_CAPACITY, EVENT_BROADCAST_CAPACITY, INTERNAL_EVENT_QUEUE_CAPACITY,
        RegistrationCommand, run_supervisor,
    },
    types::{
        AlarmTrigger, BatchOperationAccepted, DeviceControlAction, DeviceRegistrationSnapshot,
        PtzAction, RegistrationEvent, RegistrationOperationStatus, RegistrationRuntimeError,
        RegistrationSnapshot,
    },
};

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
    pub async fn trigger_alarm(&self, alarm: AlarmTrigger) -> Result<(), RegistrationRuntimeError> {
        alarm.validate()?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(RegistrationCommand::TriggerAlarm {
                alarm,
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

    /// 返回当前设备运行态列表；列表独立于轻量聚合快照查询。
    ///
    /// # Errors
    ///
    /// 当运行时 owner 已停止或命令队列关闭时返回不可用错误。
    pub async fn device_states(
        &self,
    ) -> Result<Vec<DeviceRegistrationSnapshot>, RegistrationRuntimeError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(RegistrationCommand::GetDeviceStates { reply: reply_tx })
            .await
            .map_err(|_| RegistrationRuntimeError::Unavailable)?;
        reply_rx
            .await
            .map_err(|_| RegistrationRuntimeError::Unavailable)
    }
}

impl Drop for RegistrationHandle {
    fn drop(&mut self) {
        if self.command_tx.strong_count() == 1 {
            self.shutdown.cancel();
        }
    }
}
