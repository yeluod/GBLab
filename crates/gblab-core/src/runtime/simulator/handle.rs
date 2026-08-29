//! 面向应用层的本地模拟器运行时句柄。

#![expect(
    clippy::missing_errors_doc,
    reason = "所有句柄方法统一返回 SimulatorRuntimeError，具体错误已由枚举表达"
)]

use std::future::Future;

use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::SimulatedDevice;

use super::types::{
    AlarmCommand, DeviceControlCommand, ExecutionMode, FaultProfile, OperationRecord,
    PositionCommand, PtzCommand, QueryRequest, QueryResult, RecordingCommand, RecordingEntry,
    RuntimeEventRecord, ScenarioDefinition, ScenarioId, ScenarioRuntimeState, ScenarioStatus,
    SimulatorRuntimeSnapshot, SubscriptionCommand, TransactionRecord,
};
use super::{
    SimulatorRuntimeError,
    actor::{COMMAND_CAPACITY, SimulatorCommand},
};

/// 本地模拟器运行时的克隆句柄。
#[derive(Clone)]
pub struct SimulatorRuntimeHandle {
    command_tx: mpsc::Sender<SimulatorCommand>,
    snapshot_rx: watch::Receiver<SimulatorRuntimeSnapshot>,
    cancellation: CancellationToken,
}

impl SimulatorRuntimeHandle {
    /// 创建本地模拟器句柄及其单所有者 Actor。
    pub fn prepare(
        devices: Vec<SimulatedDevice>,
    ) -> (Self, impl Future<Output = ()> + Send + 'static) {
        let (command_tx, command_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (snapshot_tx, snapshot_rx) = watch::channel(SimulatorRuntimeSnapshot::default());
        let cancellation = CancellationToken::new();
        let actor = super::actor::run(devices, command_rx, snapshot_tx, cancellation.clone());
        (
            Self {
                command_tx,
                snapshot_rx,
                cancellation,
            },
            actor,
        )
    }

    /// 返回最近快照，不发生异步等待。
    #[must_use]
    pub fn snapshot(&self) -> SimulatorRuntimeSnapshot {
        self.snapshot_rx.borrow().clone()
    }

    /// 同步持久化设备配置到本地运行时并按需保留已有通道状态。
    pub async fn sync_devices(
        &self,
        devices: Vec<SimulatedDevice>,
    ) -> Result<(), SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::SyncDevices { devices, reply })
            .await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)
    }

    /// 返回最近操作记录。
    pub async fn operations(&self) -> Result<Vec<OperationRecord>, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::GetOperations { reply }).await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)
    }

    /// 返回最近运行事件。
    pub async fn events(&self) -> Result<Vec<RuntimeEventRecord>, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::GetEvents { reply }).await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)
    }

    /// 返回最近查询结果。
    pub async fn queries(&self) -> Result<Vec<QueryResult>, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::GetQueries { reply }).await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)
    }

    /// 返回 SIP 事务投影。
    pub async fn transactions(&self) -> Result<Vec<TransactionRecord>, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::GetTransactions { reply })
            .await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)
    }

    /// 返回模拟录像索引。
    pub async fn recordings(&self) -> Result<Vec<RecordingEntry>, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::GetRecordings { reply }).await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)
    }

    /// 返回场景运行列表。
    pub async fn scenarios(&self) -> Result<Vec<ScenarioRuntimeState>, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::GetScenarios { reply }).await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)
    }

    /// 更新故障注入配置。
    pub async fn set_fault_profile(
        &self,
        profile: FaultProfile,
    ) -> Result<(), SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::SetFaultProfile { profile, reply })
            .await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)?
    }

    /// 执行设备控制。
    pub async fn control_device(
        &self,
        device_id: String,
        command: DeviceControlCommand,
        mode: ExecutionMode,
    ) -> Result<OperationRecord, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::DeviceControl {
            device_id,
            command,
            mode,
            reply,
        })
        .await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)?
    }

    /// 执行 PTZ 控制。
    pub async fn control_ptz(
        &self,
        device_id: String,
        channel_id: String,
        command: PtzCommand,
        mode: ExecutionMode,
    ) -> Result<OperationRecord, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::PtzControl {
            device_id,
            channel_id,
            command,
            mode,
            reply,
        })
        .await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)?
    }

    /// 更新报警状态或周期计划。
    pub async fn update_alarm(
        &self,
        device_id: String,
        channel_id: String,
        command: AlarmCommand,
        mode: ExecutionMode,
    ) -> Result<OperationRecord, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::Alarm {
            device_id,
            channel_id,
            command,
            mode,
            reply,
        })
        .await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)?
    }

    /// 更新移动位置及周期计划。
    pub async fn update_position(
        &self,
        device_id: String,
        channel_id: String,
        command: PositionCommand,
        mode: ExecutionMode,
    ) -> Result<OperationRecord, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::Position {
            device_id,
            channel_id,
            command,
            mode,
            reply,
        })
        .await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)?
    }

    /// 控制通道本地模拟录像状态。
    pub async fn control_recording(
        &self,
        device_id: String,
        channel_id: String,
        command: RecordingCommand,
        mode: ExecutionMode,
    ) -> Result<OperationRecord, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::Recording {
            device_id,
            channel_id,
            command,
            mode,
            reply,
        })
        .await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)?
    }

    /// 控制通道本地订阅生命周期。
    pub async fn control_subscription(
        &self,
        device_id: String,
        channel_id: String,
        command: SubscriptionCommand,
        mode: ExecutionMode,
    ) -> Result<OperationRecord, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::Subscription {
            device_id,
            channel_id,
            command,
            mode,
            reply,
        })
        .await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)?
    }

    /// 执行统一查询。
    pub async fn query(&self, request: QueryRequest) -> Result<QueryResult, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::Query { request, reply })
            .await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)?
    }

    /// 新建或更新场景定义。
    pub async fn save_scenario(
        &self,
        definition: ScenarioDefinition,
    ) -> Result<ScenarioRuntimeState, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::SaveScenario { definition, reply })
            .await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)?
    }

    /// 从第一步启动场景。
    pub async fn start_scenario(
        &self,
        id: ScenarioId,
    ) -> Result<ScenarioRuntimeState, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::StartScenario { id, reply })
            .await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)?
    }

    /// 暂停、继续或停止场景。
    pub async fn set_scenario_status(
        &self,
        id: ScenarioId,
        status: ScenarioStatus,
    ) -> Result<ScenarioRuntimeState, SimulatorRuntimeError> {
        let (reply, response) = oneshot::channel();
        self.send(SimulatorCommand::SetScenarioStatus { id, status, reply })
            .await?;
        response
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)?
    }

    async fn send(&self, command: SimulatorCommand) -> Result<(), SimulatorRuntimeError> {
        self.command_tx
            .send(command)
            .await
            .map_err(|_| SimulatorRuntimeError::Unavailable)
    }
}

impl Drop for SimulatorRuntimeHandle {
    fn drop(&mut self) {
        if self.command_tx.strong_count() == 1 {
            self.cancellation.cancel();
        }
    }
}
