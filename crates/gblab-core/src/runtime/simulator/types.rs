//! 本地模拟运行时的稳定状态、命令参数与可观测性契约。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 模拟命令的执行目标。
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionMode {
    /// 只改变本地模拟状态，不依赖注册或平台订阅。
    #[default]
    LocalSimulation,
    /// 将领域命令交给 SIP 平台适配器执行。
    Platform,
}

/// 一次运行时操作的唯一编号。
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct OperationId(pub String);

/// 一个场景定义的唯一编号。
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ScenarioId(pub String);

/// 一次查询的唯一编号。
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct QueryId(pub String);

/// 可观察操作状态。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationStatus {
    /// 已接受但尚未执行。
    Pending,
    /// 正在执行。
    Running,
    /// 已成功完成。
    Succeeded,
    /// 业务或适配器执行失败。
    Failed,
    /// 超过操作期限。
    Timeout,
    /// 被用户或生命周期取消。
    Cancelled,
}

/// 操作作用的资源。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationTarget {
    /// 设备国标编号。
    pub device_id: Option<String>,
    /// 通道国标编号。
    pub channel_id: Option<String>,
}

/// 一次本地或平台操作的统一记录。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRecord {
    /// 操作编号。
    pub id: OperationId,
    /// 稳定的操作类型名称。
    pub kind: String,
    /// 本地模拟或平台执行模式。
    pub mode: ExecutionMode,
    /// 操作目标。
    pub target: OperationTarget,
    /// 当前状态。
    pub status: OperationStatus,
    /// 开始时间，Unix 毫秒。
    pub started_at: u64,
    /// 完成时间，Unix 毫秒。
    pub completed_at: Option<u64>,
    /// 操作耗时，毫秒。
    pub duration_millis: Option<u64>,
    /// 失败码。
    pub error_code: Option<String>,
    /// 失败原因。
    pub error_message: Option<String>,
    /// 关联 SIP 事务编号；本地操作为空。
    pub transaction_id: Option<String>,
}

/// 设备连通状态。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectivityState {
    /// 本地模拟设备在线。
    Online,
    /// 本地模拟设备离线。
    Offline,
    /// 正在模拟设备重启。
    Restarting,
}

/// 报警运行状态。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmRuntimeState {
    /// 是否存在活动报警。
    pub active: bool,
    /// 报警优先级。
    pub priority: Option<String>,
    /// 报警方式。
    pub method: Option<String>,
    /// 报警类型。
    pub alarm_type: Option<String>,
    /// 报警描述。
    pub description: Option<String>,
    /// 报警发生时间。
    pub occurred_at: Option<u64>,
    /// 最近恢复时间。
    pub restored_at: Option<u64>,
    /// 周期触发间隔；为空表示未启用周期报警。
    pub interval_seconds: Option<u32>,
    /// 下一次周期触发时间。
    pub next_trigger_at: Option<u64>,
}

/// 移动位置模拟模式。
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PositionSimulationMode {
    /// 仅保存当前固定坐标。
    #[default]
    Fixed,
    /// 每次上报按速度和方向推进。
    Route,
    /// 每次上报增加确定性小幅扰动。
    RandomWalk,
}

/// 移动位置运行状态。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionRuntimeState {
    /// 经度。
    pub longitude: f64,
    /// 纬度。
    pub latitude: f64,
    /// 速度，千米/小时。
    pub speed: f64,
    /// 方向角，0 至 360 度。
    pub direction: f64,
    /// 海拔，米。
    pub altitude: f64,
    /// 最近更新时间。
    pub updated_at: Option<u64>,
    /// 当前模拟模式。
    pub mode: PositionSimulationMode,
    /// 是否正在周期模拟。
    pub running: bool,
    /// 周期间隔。
    pub interval_seconds: Option<u32>,
    /// 下一次上报时间。
    pub next_report_at: Option<u64>,
}

impl Default for PositionRuntimeState {
    fn default() -> Self {
        Self {
            longitude: 116.397,
            latitude: 39.908,
            speed: 0.0,
            direction: 0.0,
            altitude: 0.0,
            updated_at: None,
            mode: PositionSimulationMode::Fixed,
            running: false,
            interval_seconds: None,
            next_report_at: None,
        }
    }
}

/// PTZ 连续运动方向。
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PtzMotion {
    /// 当前停止。
    #[default]
    Stop,
    /// 向上。
    Up,
    /// 向下。
    Down,
    /// 向左。
    Left,
    /// 向右。
    Right,
    /// 放大。
    ZoomIn,
    /// 缩小。
    ZoomOut,
    /// 聚焦增加。
    FocusNear,
    /// 聚焦减小。
    FocusFar,
    /// 光圈增大。
    IrisOpen,
    /// 光圈减小。
    IrisClose,
}

/// PTZ 预置位。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PtzPreset {
    /// 预置位编号。
    pub id: u16,
    /// 预置位名称。
    pub name: String,
    /// 水平位置。
    pub pan: i16,
    /// 垂直位置。
    pub tilt: i16,
    /// 变倍位置。
    pub zoom: u16,
}

/// PTZ 当前状态。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PtzRuntimeState {
    /// 当前运动。
    pub motion: PtzMotion,
    /// 当前速度，1 至 255。
    pub speed: u8,
    /// 水平模拟坐标。
    pub pan: i16,
    /// 垂直模拟坐标。
    pub tilt: i16,
    /// 变倍模拟坐标。
    pub zoom: u16,
    /// 聚焦模拟坐标。
    pub focus: u16,
    /// 光圈模拟坐标。
    pub iris: u16,
    /// 当前调用的预置位。
    pub active_preset: Option<u16>,
    /// 已配置预置位。
    pub presets: Vec<PtzPreset>,
    /// 最近动作时间。
    pub updated_at: Option<u64>,
}

/// 录像运行状态。
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordingRuntimeStatus {
    /// 未录像。
    #[default]
    Idle,
    /// 正在录像。
    Recording,
    /// 已暂停。
    Paused,
    /// 最近一次录像失败。
    Failed,
}

/// 通道录像运行状态。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingRuntimeState {
    /// 当前状态。
    pub status: RecordingRuntimeStatus,
    /// 当前文件。
    pub current_file: Option<String>,
    /// 开始时间。
    pub started_at: Option<u64>,
    /// 已录制时长。
    pub duration_millis: u64,
    /// 最近错误。
    pub last_error: Option<String>,
}

/// 本地模拟录像控制命令。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RecordingCommand {
    /// 开始一段逻辑录像。
    Start {
        /// 录像显示名称。
        name: String,
    },
    /// 暂停当前录像。
    Pause,
    /// 继续当前录像。
    Resume,
    /// 停止录像并写入运行时录像索引。
    Stop,
}

/// 本地模拟录像索引项。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingEntry {
    /// 录像编号。
    pub id: String,
    /// 设备编号。
    pub device_id: String,
    /// 通道编号。
    pub channel_id: String,
    /// 文件名或逻辑名称。
    pub name: String,
    /// 开始时间。
    pub started_at: u64,
    /// 结束时间。
    pub ended_at: u64,
    /// 录像类型。
    pub record_type: String,
    /// 文件大小；纯模拟录像可以为 0。
    pub size_bytes: u64,
    /// 实际文件路径；纯模拟录像为空。
    pub file_path: Option<String>,
}

/// 通道订阅运行状态。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSubscriptionState {
    /// 订阅类型。
    pub kind: String,
    /// 订阅状态。
    pub status: String,
    /// 到期时间。
    pub expires_at: Option<u64>,
    /// 最近通知时间。
    pub last_notified_at: Option<u64>,
    /// 最近错误。
    pub last_error: Option<String>,
}

/// 本地订阅生命周期命令。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SubscriptionCommand {
    /// 建立或刷新订阅。
    Upsert {
        /// `Catalog`、`Alarm` 或 `MobilePosition`。
        subscription_kind: String,
        /// 有效期秒数。
        expires_seconds: u32,
    },
    /// 主动取消订阅。
    Cancel {
        /// 订阅类型。
        subscription_kind: String,
    },
    /// 注入订阅失败状态。
    Fail {
        /// 订阅类型。
        subscription_kind: String,
        /// 失败原因。
        error: String,
    },
}

/// 单通道完整运行状态。
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRuntimeState {
    /// 通道编号。
    pub channel_id: String,
    /// 通道名称。
    pub name: String,
    /// 是否在线。
    pub online: bool,
    /// 报警状态。
    pub alarm: AlarmRuntimeState,
    /// 移动位置状态。
    pub position: PositionRuntimeState,
    /// PTZ 状态。
    pub ptz: PtzRuntimeState,
    /// 录像状态。
    pub recording: RecordingRuntimeState,
    /// 平台订阅投影。
    pub subscriptions: Vec<ChannelSubscriptionState>,
    /// 最近操作编号。
    pub last_operation_id: Option<OperationId>,
}

/// 单设备完整运行状态。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRuntimeState {
    /// 设备编号。
    pub device_id: String,
    /// 设备名称。
    pub name: String,
    /// 本地连通状态。
    pub connectivity: ConnectivityState,
    /// 是否布防。
    pub guarded: bool,
    /// 模拟设备时钟相对系统时间的偏移毫秒。
    pub clock_offset_millis: i64,
    /// 最近平台请求时间。
    pub last_platform_request_at: Option<u64>,
    /// 最近操作编号。
    pub last_operation_id: Option<OperationId>,
    /// 派生通道运行状态。
    pub channels: Vec<ChannelRuntimeState>,
}

/// 本地模拟运行时快照。
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulatorRuntimeSnapshot {
    /// 状态修订号。
    pub revision: u64,
    /// 当前设备状态。
    pub devices: Vec<DeviceRuntimeState>,
    /// 当前运行中的场景。
    pub active_scenarios: usize,
    /// 当前故障配置。
    pub fault_profile: FaultProfile,
}

/// 设备控制命令。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DeviceControlCommand {
    /// 模拟重启指定秒数。
    Restart {
        /// 模拟重启持续秒数。
        duration_seconds: u32,
    },
    /// 布防。
    Guard,
    /// 撤防。
    Unguard,
    /// 清除所有通道报警。
    AlarmReset,
    /// 调整设备时间偏移。
    SetTime {
        /// 相对系统时间的偏移毫秒。
        offset_millis: i64,
    },
    /// 设为在线。
    SetOnline,
    /// 设为离线。
    SetOffline,
}

/// PTZ 命令。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PtzCommand {
    /// 开始连续运动。
    Move {
        /// 连续运动类型。
        motion: PtzMotion,
        /// 模拟速度。
        speed: u8,
    },
    /// 停止连续运动。
    Stop,
    /// 保存当前坐标为预置位。
    SetPreset {
        /// 预置位编号。
        id: u16,
        /// 预置位名称。
        name: String,
    },
    /// 调用预置位。
    CallPreset {
        /// 预置位编号。
        id: u16,
    },
    /// 删除预置位。
    DeletePreset {
        /// 预置位编号。
        id: u16,
    },
}

/// 报警状态变更参数。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmCommand {
    /// 是否发生报警；false 表示恢复。
    pub active: bool,
    /// 报警优先级。
    pub priority: String,
    /// 报警方式。
    pub method: String,
    /// 报警类型。
    pub alarm_type: Option<String>,
    /// 描述。
    pub description: String,
    /// 周期间隔；仅在 active=true 时生效。
    pub interval_seconds: Option<u32>,
}

/// 移动位置状态变更参数。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionCommand {
    /// 经度。
    pub longitude: f64,
    /// 纬度。
    pub latitude: f64,
    /// 速度。
    pub speed: f64,
    /// 方向角。
    pub direction: f64,
    /// 海拔。
    pub altitude: f64,
    /// 模拟模式。
    pub mode: PositionSimulationMode,
    /// 是否开始周期模拟。
    pub running: bool,
    /// 周期间隔。
    pub interval_seconds: Option<u32>,
}

/// 查询类型。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum QueryKind {
    /// 目录。
    Catalog,
    /// 设备信息。
    DeviceInfo,
    /// 设备状态。
    DeviceStatus,
    /// 设备能力。
    DeviceCapability,
    /// 设备时间。
    DeviceTime,
    /// 设备参数。
    DeviceParameter,
    /// 配置下载。
    ConfigDownload,
    /// 报警状态。
    AlarmStatus,
    /// 移动位置。
    MobilePosition,
    /// 预置位。
    PresetQuery,
    /// 录像信息。
    RecordInfo,
}

/// 统一查询请求。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    /// 设备编号。
    pub device_id: String,
    /// 可选通道编号。
    pub channel_id: Option<String>,
    /// 查询类型。
    pub kind: QueryKind,
    /// 类型化查询以外的扩展参数。
    #[serde(default)]
    pub parameters: BTreeMap<String, serde_json::Value>,
    /// 执行模式。
    #[serde(default)]
    pub mode: ExecutionMode,
}

/// 统一查询结果。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    /// 查询编号。
    pub id: QueryId,
    /// 原查询。
    pub request: QueryRequest,
    /// 查询状态。
    pub status: OperationStatus,
    /// 结构化响应。
    pub response: Option<serde_json::Value>,
    /// 失败原因。
    pub error: Option<String>,
    /// 开始时间。
    pub started_at: u64,
    /// 完成时间。
    pub completed_at: u64,
    /// 耗时。
    pub duration_millis: u64,
    /// 关联操作编号。
    pub operation_id: OperationId,
}

/// 故障注入配置。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FaultProfile {
    /// 为所有本地操作增加的确定性延迟。
    pub delay_millis: u64,
    /// 是否让操作直接超时。
    pub force_timeout: bool,
    /// 按 0 至 100 的确定性序列模拟丢弃比例。
    pub packet_loss_percent: u8,
    /// 强制业务失败码。
    pub reject_status: Option<u16>,
    /// 强制设备离线。
    pub force_device_offline: bool,
}

/// 场景步骤动作。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ScenarioAction {
    /// 等待指定时长。
    Delay {
        /// 等待毫秒数。
        duration_millis: u64,
    },
    /// 设备控制。
    DeviceControl {
        /// 设备控制命令。
        command: DeviceControlCommand,
    },
    /// PTZ 控制。
    Ptz {
        /// PTZ 命令。
        command: PtzCommand,
    },
    /// 报警状态变更。
    Alarm {
        /// 报警命令。
        command: AlarmCommand,
    },
    /// 位置状态变更。
    Position {
        /// 位置命令。
        command: PositionCommand,
    },
    /// 录像控制。
    Recording {
        /// 录像命令。
        command: RecordingCommand,
    },
    /// 订阅生命周期变更。
    Subscription {
        /// 订阅命令。
        command: SubscriptionCommand,
    },
    /// 执行查询。
    Query {
        /// 查询请求。
        request: QueryRequest,
    },
}

/// 场景步骤。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioStep {
    /// 步骤名称。
    pub name: String,
    /// 目标设备。
    pub device_id: String,
    /// 目标通道。
    pub channel_id: Option<String>,
    /// 步骤动作。
    pub action: ScenarioAction,
}

/// 场景定义。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioDefinition {
    /// 场景编号；为空时由运行时分配。
    pub id: Option<ScenarioId>,
    /// 场景名称。
    pub name: String,
    /// 步骤列表。
    pub steps: Vec<ScenarioStep>,
    /// 完成后是否从第一步重新运行。
    pub repeat: bool,
}

/// 场景执行状态。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScenarioStatus {
    /// 未启动。
    Idle,
    /// 运行中。
    Running,
    /// 已暂停。
    Paused,
    /// 已完成。
    Completed,
    /// 已停止。
    Stopped,
    /// 执行失败。
    Failed,
}

/// 场景运行快照。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioRuntimeState {
    /// 场景编号。
    pub id: ScenarioId,
    /// 名称。
    pub name: String,
    /// 状态。
    pub status: ScenarioStatus,
    /// 当前步骤，从 0 开始。
    pub current_step: usize,
    /// 步骤总数。
    pub total_steps: usize,
    /// 下一步执行时间。
    pub next_step_at: Option<u64>,
    /// 最近错误。
    pub last_error: Option<String>,
}

/// 可观察运行事件级别。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeEventLevel {
    /// 普通状态变化。
    Info,
    /// 可恢复异常。
    Warning,
    /// 失败。
    Error,
}

/// 统一运行事件。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEventRecord {
    /// 单次运行内递增编号。
    pub id: u64,
    /// 时间。
    pub timestamp: u64,
    /// 稳定事件类型。
    pub kind: String,
    /// 级别。
    pub level: RuntimeEventLevel,
    /// 设备编号。
    pub device_id: Option<String>,
    /// 通道编号。
    pub channel_id: Option<String>,
    /// 关联操作。
    pub operation_id: Option<OperationId>,
    /// 人类可读消息。
    pub message: String,
}

/// SIP 事务的可观察摘要。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionRecord {
    /// 内部事务编号。
    pub id: String,
    /// SIP Call-ID。
    pub call_id: String,
    /// `CSeq` 序号。
    pub cseq: u32,
    /// SIP 方法。
    pub method: String,
    /// Via branch。
    pub via_branch: String,
    /// 状态。
    pub status: String,
    /// SIP 响应码。
    pub response_status: Option<u16>,
    /// 传输或协议错误。
    pub error: Option<String>,
    /// 创建时间。
    pub started_at: u64,
    /// 完成时间。
    pub completed_at: Option<u64>,
}
