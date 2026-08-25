/** 唯一 SIP 服务的前端演示配置。 */
export interface SipServiceConfig {
  uri: string;
  transport: 'UDP' | 'TCP';
  platformId: string;
  domain: string;
  registerExpires: number;
  keepaliveInterval: number;
}

/** 模拟设备支持的最小类型集合。 */
export type DeviceType = '摄像机' | '球机' | 'NVR' | '门禁设备';

/** 内存中的模拟设备。 */
export interface SimulatedDevice {
  id: string;
  name: string;
  type: DeviceType;
  isEnabled: boolean;
  createdAt: string;
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

/** 单设备新增或编辑表单。 */
export interface DeviceDraft {
  id: string;
  name: string;
  type: DeviceType;
  isEnabled: boolean;
}

/** 批量新增表单。 */
export interface BatchDeviceDraft {
  count: number;
  startDeviceId: string;
  nameTemplate: string;
  type: DeviceType;
  isEnabled: boolean;
}

/** 前端演示操作的可见结果。 */
export type OperationResult = { ok: true } | { ok: false; message: string };
