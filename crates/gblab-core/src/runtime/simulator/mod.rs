//! 独立于平台连接的本地设备、通道、查询、故障和场景模拟运行时。

mod actor;
mod handle;
mod state;
mod types;

use thiserror::Error;

pub use handle::SimulatorRuntimeHandle;
pub use types::{
    AlarmCommand, AlarmRuntimeState, ChannelRuntimeState, ChannelSubscriptionState,
    ConnectivityState, DeviceControlCommand, DeviceRuntimeState, ExecutionMode, FaultProfile,
    OperationId, OperationRecord, OperationStatus, OperationTarget, PositionCommand,
    PositionRuntimeState, PositionSimulationMode, PtzCommand, PtzMotion, PtzPreset,
    PtzRuntimeState, QueryId, QueryKind, QueryRequest, QueryResult, RecordingCommand,
    RecordingEntry, RecordingRuntimeState, RecordingRuntimeStatus, RuntimeEventLevel,
    RuntimeEventRecord, ScenarioAction, ScenarioDefinition, ScenarioId, ScenarioRuntimeState,
    ScenarioStatus, ScenarioStep, SimulatorRuntimeSnapshot, SubscriptionCommand, TransactionRecord,
};

/// 本地模拟器运行时错误。
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SimulatorRuntimeError {
    /// Actor 已停止。
    #[error("本地模拟运行时不可用")]
    Unavailable,
    /// 设备不存在。
    #[error("设备不存在: {0}")]
    DeviceNotFound(String),
    /// 通道不存在。
    #[error("通道不存在: {0}")]
    ChannelNotFound(String),
    /// 设备离线。
    #[error("设备当前离线: {0}")]
    DeviceOffline(String),
    /// 预置位不存在。
    #[error("PTZ 预置位不存在: {0}")]
    PresetNotFound(u16),
    /// 场景不存在。
    #[error("场景不存在: {0}")]
    ScenarioNotFound(String),
    /// 参数不合法。
    #[error("模拟参数无效: {0}")]
    InvalidInput(String),
    /// 平台模式尚未绑定适配器。
    #[error("平台执行模式尚未绑定 SIP 适配器")]
    PlatformAdapterUnavailable,
    /// 故障注入超时。
    #[error("故障注入强制操作超时")]
    ForcedTimeout,
    /// 故障注入拒绝。
    #[error("故障注入强制返回 {0}")]
    ForcedRejection(u16),
    /// 故障注入消息丢失。
    #[error("故障注入模拟消息丢失")]
    SimulatedPacketLoss,
}
