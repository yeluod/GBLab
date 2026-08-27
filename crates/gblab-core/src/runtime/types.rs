//! 注册运行时的稳定公开类型。

use serde::Serialize;
use thiserror::Error;

use super::platform::SubscriptionSnapshot;

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
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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
    /// 当前配置设备总数。
    pub total_devices: usize,
    /// 当前已注册设备数。
    pub registered_count: usize,
    /// 当前失败设备数。
    pub failed_count: usize,
    /// 当前有效订阅数。
    pub active_subscriptions: usize,
    /// 因队列满而丢弃的详细日志数量。
    pub dropped_logs: u64,
}

impl Default for RegistrationSnapshot {
    fn default() -> Self {
        Self {
            operation_status: RegistrationOperationStatus::Idle,
            operation_id: None,
            total_devices: 0,
            registered_count: 0,
            failed_count: 0,
            active_subscriptions: 0,
            dropped_logs: 0,
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
    /// 当前设备运行态列表，独立于轻量聚合快照传递。
    DeviceStates(Vec<DeviceRegistrationSnapshot>),
    /// 当前订阅运行态列表，独立于轻量聚合快照传递。
    Subscriptions(Vec<SubscriptionSnapshot>),
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
    /// 平台尚未建立当前业务需要的有效订阅。
    #[error("平台尚未建立有效的 {0} 订阅")]
    MissingActiveSubscription(&'static str),
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
    pub(super) const fn as_xml(self) -> &'static str {
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
    pub(super) const fn as_xml(self) -> &'static str {
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
