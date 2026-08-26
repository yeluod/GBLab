/** 模拟设备支持的最小类型集合。 */
export type DeviceType = '摄像机' | '球机' | 'NVR' | '门禁设备';

/** 设备与平台之间的运行时注册状态，不写入配置文件。 */
export type RegistrationStatus =
  'unregistered' | 'queued' | 'registering' | 'registered' | 'unregistering' | 'failed';

/** 全量设备注册生命周期状态。 */
export type RegistrationOperationStatus = 'idle' | 'registering' | 'running' | 'stopping';

/** 内存中的模拟设备。 */
export interface SimulatedDevice {
  id: string;
  name: string;
  type: DeviceType;
  manufacturer: string;
  model: string;
  firmwareVersion: string;
  channelCount: number;
  registrationStatus: RegistrationStatus;
  online?: boolean;
  lastHeartbeatAt?: number | null;
  lastPlatformRequestAt?: number | null;
  heartbeatFailures?: number;
  lastControlAction?: string | null;
  ptzAction?: string | null;
  guarded?: boolean;
  alarmActive?: boolean;
  createdAt: number;
}

/** 平台可能订阅的 GB28181 内容类型。 */
export type SubscriptionKind = 'catalog' | 'alarm' | 'mobile-position';

/** 服务端订阅在前端的展示状态。 */
export type SubscriptionStatus = 'active' | 'inactive';

/** 单条订阅记录与目录预览。 */
export interface DeviceSubscription {
  id: string;
  deviceId: string;
  kind: SubscriptionKind;
  status: SubscriptionStatus;
  expiresAt: string | null;
  lastNotifiedAt: string | null;
  catalogPreview: string[];
}

/** 设备下的模拟通道及平台已订阅的业务项；通道 ID 为 20 位数字。 */
export interface SimulatedChannel {
  id: string;
  deviceId: string;
  name: string;
  index: number;
  platformSubscriptions: SubscriptionKind[];
}

/** 与平台发生的单条模拟交互记录；设备与通道 ID 均为 20 位数字。 */
export interface InteractionLog {
  id: string;
  timestamp: number;
  deviceId: string;
  channelId: string | null;
  direction: 'send' | 'receive';
  message: string;
}

/** Rust 侧交互日志分页查询结果。 */
export interface InteractionLogPage {
  items: InteractionLog[];
  total: number;
  offset: number;
  limit: number;
}

/** Rust 注册运行时返回的单设备状态。 */
export interface DeviceRegistrationSnapshot {
  deviceId: string;
  status: RegistrationStatus;
  lastError: string | null;
  expiresAt: number | null;
  lastPlatformRequestAt?: number | null;
  lastHeartbeatAt?: number | null;
  online?: boolean;
  heartbeatFailures?: number;
  lastControlAction?: string | null;
  ptzAction?: string | null;
  guarded?: boolean;
  alarmActive?: boolean;
}

/** Rust 注册运行时返回的完整内存快照。 */
export interface RegistrationSnapshot {
  operationStatus: RegistrationOperationStatus;
  operationId: string | null;
  devices: DeviceRegistrationSnapshot[];
  interactionLogs: Array<Omit<InteractionLog, 'id'> & { sequence: number }>;
  subscriptions?: SubscriptionSnapshot[];
}

export type SubscriptionRuntimeStatus =
  'pending' | 'active' | 'refreshing' | 'cancelled' | 'expired' | 'failed';

export interface SubscriptionSnapshot {
  deviceId: string;
  channelId: string | null;
  commandType:
    | 'catalog'
    | 'deviceInfo'
    | 'deviceStatus'
    | 'deviceControl'
    | 'recordInfo'
    | 'alarm'
    | 'mobilePosition'
    | 'keepalive'
    | 'unknown';
  callId: string | null;
  status: SubscriptionRuntimeStatus;
  expiresAt: number | null;
  lastNotifiedAt: number | null;
  lastError: string | null;
}

/** 后端已接收的异步全量操作。 */
export interface BatchOperationAccepted {
  operationId: string;
  total: number;
}

/** 单设备编辑表单；设备创建仅支持批量操作。 */
export interface DeviceUpdateDraft {
  name: string;
  type: DeviceType;
  manufacturer: string;
  model: string;
  firmwareVersion: string;
  channelCount: number;
}

/** 批量新增表单。 */
export interface BatchDeviceDraft {
  count: number;
  startDeviceId: string;
  nameTemplate: string;
  type: DeviceType;
  manufacturer: string;
  model: string;
  firmwareVersion: string;
  channelCount: number;
}

/** Rust 核心返回的持久化设备与运行时派生通道快照。 */
export interface DeviceSnapshot {
  devices: SimulatedDevice[];
  hasCompletedBatchAdd: boolean;
}

/** Rust 侧设备分页查询结果。 */
export interface DevicePage {
  devices: SimulatedDevice[];
  total: number;
  offset: number;
  limit: number;
  hasCompletedBatchAdd: boolean;
}

/** 前端演示操作的可见结果。 */
export type OperationResult = { ok: true } | { ok: false; message: string };
