import { computed, ref } from 'vue';
import { defineStore } from 'pinia';

import {
  getSipServiceConfiguration,
  saveSipServiceConfiguration,
  type ConfigurationCommandError,
  type SipServiceConfig,
} from '@/features/settings';

import type {
  BatchDeviceDraft,
  DeviceUpdateDraft,
  DeviceSubscription,
  InteractionLog,
  OperationResult,
  SimulatedChannel,
  SimulatedDevice,
  SubscriptionKind,
} from './types';

const DEVICE_ID_PATTERN = /^\d{20}$/;
const MAX_BATCH_DEVICE_COUNT = 1_000;
const MAX_CHANNEL_COUNT = 128;
const MAX_INTERACTION_LOG_COUNT = 500;
const DEFAULT_CREATED_AT = '2026-08-25 14:20:00';

function getConfigurationErrorMessage(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const commandError = error as ConfigurationCommandError;
    if (typeof commandError.message === 'string' && commandError.message.length > 0) {
      return commandError.message;
    }
  }
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return '桌面后端暂时不可用，请重试。';
}

function createInitialDevices(): SimulatedDevice[] {
  return [
    {
      id: '34020000001320000001',
      name: '模拟摄像机-001',
      type: '摄像机',
      manufacturer: '海康威视',
      model: 'DS-2CD2146G2-I',
      firmwareVersion: 'V5.7.11',
      channelCount: 2,
      registrationStatus: 'unregistered',
      createdAt: DEFAULT_CREATED_AT,
    },
    {
      id: '34020000001320000002',
      name: '模拟摄像机-002',
      type: '摄像机',
      manufacturer: '大华',
      model: 'DH-IPC-HFW5442',
      firmwareVersion: '2.840.G.2',
      channelCount: 1,
      registrationStatus: 'unregistered',
      createdAt: DEFAULT_CREATED_AT,
    },
    {
      id: '34020000001320000003',
      name: '园区球机-001',
      type: '球机',
      manufacturer: '海康威视',
      model: 'DS-2DE7A245IX-AE',
      firmwareVersion: 'V5.7.19',
      channelCount: 1,
      registrationStatus: 'registered',
      createdAt: DEFAULT_CREATED_AT,
    },
    {
      id: '34020000001320000004',
      name: '仓库 NVR-001',
      type: 'NVR',
      manufacturer: '大华',
      model: 'NVR608-128-4KS2',
      firmwareVersion: '4.004.0000004.1',
      channelCount: 8,
      registrationStatus: 'registered',
      createdAt: DEFAULT_CREATED_AT,
    },
    {
      id: '34020000001320000005',
      name: '门禁设备-001',
      type: '门禁设备',
      manufacturer: 'GBLab',
      model: 'ACS-SIM-100',
      firmwareVersion: 'V1.0.0',
      channelCount: 1,
      registrationStatus: 'unregistered',
      createdAt: DEFAULT_CREATED_AT,
    },
  ];
}

/**
 * 静态演示阶段使用的 20 位数字通道 ID。接入桌面核心后，直接采用后端返回的通道 ID。
 */
function createMockChannelId(deviceId: string, channelIndex: number): string {
  return `${deviceId.slice(0, 14)}${deviceId.slice(-3)}${String(channelIndex).padStart(3, '0')}`;
}

function createChannelsForDevice(
  device: Pick<SimulatedDevice, 'id' | 'name' | 'channelCount'>,
  subscriptions: DeviceSubscription[],
): SimulatedChannel[] {
  const platformSubscriptions = subscriptions
    .filter(
      (subscription) => subscription.deviceId === device.id && subscription.status === 'active',
    )
    .map((subscription) => subscription.kind);

  return Array.from({ length: device.channelCount }, (_, index) => ({
    id: createMockChannelId(device.id, index + 1),
    deviceId: device.id,
    name: `${device.name} · 通道 ${String(index + 1).padStart(2, '0')}`,
    index: index + 1,
    platformSubscriptions: [...new Set(platformSubscriptions)] as SubscriptionKind[],
  }));
}

function createInitialSubscriptions(): DeviceSubscription[] {
  return [
    {
      id: 'subscription-001',
      deviceId: '34020000001320000001',
      kind: 'catalog',
      status: 'active',
      expiresAt: '2026-08-25 15:18:00',
      lastNotifiedAt: '2026-08-25 14:18:12',
      catalogPreview: [
        `${createMockChannelId('34020000001320000001', 1)} · 主码流`,
        `${createMockChannelId('34020000001320000001', 2)} · 子码流`,
      ],
    },
    {
      id: 'subscription-002',
      deviceId: '34020000001320000001',
      kind: 'alarm',
      status: 'active',
      expiresAt: '2026-08-25 15:18:00',
      lastNotifiedAt: null,
      catalogPreview: [],
    },
    {
      id: 'subscription-003',
      deviceId: '34020000001320000003',
      kind: 'catalog',
      status: 'active',
      expiresAt: '2026-08-25 14:48:00',
      lastNotifiedAt: '2026-08-25 14:15:32',
      catalogPreview: [`${createMockChannelId('34020000001320000003', 1)} · 园区入口`],
    },
    {
      id: 'subscription-004',
      deviceId: '34020000001320000003',
      kind: 'mobile-position',
      status: 'inactive',
      expiresAt: null,
      lastNotifiedAt: null,
      catalogPreview: [],
    },
    {
      id: 'subscription-005',
      deviceId: '34020000001320000004',
      kind: 'alarm',
      status: 'active',
      expiresAt: '2026-08-25 15:30:00',
      lastNotifiedAt: '2026-08-25 14:14:05',
      catalogPreview: [],
    },
  ];
}

function createInitialInteractionLogs(): InteractionLog[] {
  return [
    {
      id: 'interaction-001',
      timestamp: '2026-08-25 14:20:00',
      deviceId: '34020000001320000001',
      channelId: createMockChannelId('34020000001320000001', 1),
      message: '← SUBSCRIBE Catalog · 平台已订阅设备目录。',
    },
    {
      id: 'interaction-002',
      timestamp: '2026-08-25 14:20:04',
      deviceId: '34020000001320000004',
      channelId: createMockChannelId('34020000001320000004', 1),
      message: '→ REGISTER sip:192.168.1.100:5060 · Contact 已发送。',
    },
    {
      id: 'interaction-003',
      timestamp: '2026-08-25 14:20:08',
      deviceId: '34020000001320000003',
      channelId: createMockChannelId('34020000001320000003', 1),
      message: '← SIP/2.0 200 OK · REGISTER 注册成功，Expires: 3600。',
    },
    {
      id: 'interaction-004',
      timestamp: '2026-08-25 14:20:12',
      deviceId: '34020000001320000003',
      channelId: createMockChannelId('34020000001320000003', 1),
      message: '→ NOTIFY Catalog · 园区入口通道目录已上报至平台。',
    },
    {
      id: 'interaction-005',
      timestamp: '2026-08-25 14:20:15',
      deviceId: '34020000001320000004',
      channelId: createMockChannelId('34020000001320000004', 1),
      message: '← SIP/2.0 200 OK · MESSAGE 报警订阅已确认。',
    },
  ];
}

function formatCurrentTimestamp(): string {
  const now = new Date();
  const pad = (value: number): string => String(value).padStart(2, '0');
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())} ${pad(
    now.getHours(),
  )}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`;
}

function normalizeDeviceDraft(draft: DeviceUpdateDraft): DeviceUpdateDraft {
  return {
    ...draft,
    name: draft.name.trim(),
    manufacturer: draft.manufacturer.trim(),
    model: draft.model.trim(),
    firmwareVersion: draft.firmwareVersion.trim(),
  };
}

function validateDeviceDraft(draft: DeviceUpdateDraft): OperationResult {
  if (draft.name.length === 0) {
    return { ok: false, message: '请输入设备名称。' };
  }
  if (
    draft.manufacturer.length === 0 ||
    draft.model.length === 0 ||
    draft.firmwareVersion.length === 0
  ) {
    return { ok: false, message: '请填写制造商、设备型号和固件版本。' };
  }
  if (
    !Number.isInteger(draft.channelCount) ||
    draft.channelCount < 1 ||
    draft.channelCount > MAX_CHANNEL_COUNT
  ) {
    return { ok: false, message: `通道数量必须介于 1 到 ${MAX_CHANNEL_COUNT}。` };
  }
  return { ok: true };
}

function validateSipServiceConfig(config: SipServiceConfig): OperationResult {
  if (!config.uri.trim().startsWith('sip:')) {
    return { ok: false, message: 'SIP 地址必须以 sip: 开头。' };
  }
  if (!DEVICE_ID_PATTERN.test(config.platformId.trim())) {
    return { ok: false, message: '平台 ID 必须为 20 位数字。' };
  }
  if (config.password.length === 0 || config.password.length > 128) {
    return { ok: false, message: '密码不能为空且长度不能超过 128 个字符。' };
  }
  if (/\p{Cc}/u.test(config.password)) {
    return { ok: false, message: '密码不能包含控制字符。' };
  }
  if (
    config.domain.trim().length === 0 ||
    config.registerExpires <= 0 ||
    config.keepaliveInterval <= 0
  ) {
    return { ok: false, message: '请填写有效的服务配置。' };
  }

  return { ok: true };
}

function normalizeSipServiceConfig(config: SipServiceConfig): SipServiceConfig {
  return {
    ...config,
    uri: config.uri.trim(),
    platformId: config.platformId.trim(),
    domain: config.domain.trim(),
  };
}

/** 设备与运行状态演示数据源；SIP 服务配置通过类型化 IPC 读写桌面核心。 */
export const useSimulatorStore = defineStore('simulator', () => {
  const sipService = ref<SipServiceConfig>({
    uri: 'sip:192.168.1.100:5060',
    transport: 'UDP',
    platformId: '34020000002000000001',
    domain: '3402000000',
    password: '',
    registerExpires: 3_600,
    keepaliveInterval: 60,
  });
  const isSipServiceLoading = ref(false);
  const isSipServiceSaving = ref(false);
  const devices = ref<SimulatedDevice[]>(createInitialDevices());
  const subscriptions = ref<DeviceSubscription[]>(createInitialSubscriptions());
  const channels = ref<SimulatedChannel[]>(
    devices.value.flatMap((device) => createChannelsForDevice(device, subscriptions.value)),
  );
  const interactionLogs = ref<InteractionLog[]>(createInitialInteractionLogs());
  const hasCompletedBatchAdd = ref(false);

  const registeredDeviceCount = computed(
    () => devices.value.filter((device) => device.registrationStatus === 'registered').length,
  );
  const activeSubscriptionCount = computed(
    () => subscriptions.value.filter((subscription) => subscription.status === 'active').length,
  );

  function appendInteractionLogs(logs: InteractionLog[]): void {
    interactionLogs.value.push(...logs);
    if (interactionLogs.value.length > MAX_INTERACTION_LOG_COUNT) {
      interactionLogs.value.splice(0, interactionLogs.value.length - MAX_INTERACTION_LOG_COUNT);
    }
  }

  function appendRegistrationLogs(targetDevices: SimulatedDevice[], isRegistering: boolean): void {
    const timestamp = formatCurrentTimestamp();
    const firstChannelIdByDevice = new Map<string, string>();
    channels.value.forEach((channel) => {
      if (!firstChannelIdByDevice.has(channel.deviceId)) {
        firstChannelIdByDevice.set(channel.deviceId, channel.id);
      }
    });
    appendInteractionLogs(
      targetDevices.map((device) => ({
        id: `interaction-${crypto.randomUUID()}`,
        timestamp,
        deviceId: device.id,
        channelId: firstChannelIdByDevice.get(device.id) ?? createMockChannelId(device.id, 1),
        message: isRegistering
          ? `→ REGISTER ${sipService.value.uri} · 设备已请求注册，Expires: ${sipService.value.registerExpires}。`
          : `→ REGISTER ${sipService.value.uri} · 设备已请求注销，Expires: 0。`,
      })),
    );
  }

  function updateSipService(config: SipServiceConfig): OperationResult {
    const normalized = normalizeSipServiceConfig(config);
    const validation = validateSipServiceConfig(normalized);
    if (!validation.ok) {
      return validation;
    }

    sipService.value = normalized;
    return { ok: true };
  }

  async function loadSipService(): Promise<OperationResult> {
    if (isSipServiceLoading.value) {
      return { ok: false, message: 'SIP 服务配置正在加载。' };
    }

    isSipServiceLoading.value = true;
    try {
      sipService.value = await getSipServiceConfiguration();
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    } finally {
      isSipServiceLoading.value = false;
    }
  }

  async function saveSipService(config: SipServiceConfig): Promise<OperationResult> {
    if (isSipServiceSaving.value) {
      return { ok: false, message: 'SIP 服务配置正在保存。' };
    }
    const normalized = normalizeSipServiceConfig(config);
    const validation = validateSipServiceConfig(normalized);
    if (!validation.ok) {
      return validation;
    }

    isSipServiceSaving.value = true;
    try {
      sipService.value = await saveSipServiceConfiguration(normalized);
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    } finally {
      isSipServiceSaving.value = false;
    }
  }

  function updateDevice(deviceId: string, draft: DeviceUpdateDraft): OperationResult {
    const device = devices.value.find((item) => item.id === deviceId);
    if (device === undefined) {
      return { ok: false, message: '设备不存在或已被删除。' };
    }
    const normalizedDraft = normalizeDeviceDraft(draft);
    const validation = validateDeviceDraft(normalizedDraft);
    if (!validation.ok) {
      return validation;
    }

    Object.assign(device, normalizedDraft);
    channels.value = channels.value.filter((channel) => channel.deviceId !== deviceId);
    channels.value.push(...createChannelsForDevice(device, subscriptions.value));
    return { ok: true };
  }

  function addDevicesInBatch(draft: BatchDeviceDraft): OperationResult {
    if (hasCompletedBatchAdd.value) {
      return { ok: false, message: '设备仅允许批量添加一次。' };
    }
    if (!Number.isInteger(draft.count) || draft.count < 1 || draft.count > MAX_BATCH_DEVICE_COUNT) {
      return { ok: false, message: `设备数量必须介于 1 到 ${MAX_BATCH_DEVICE_COUNT}。` };
    }
    if (!DEVICE_ID_PATTERN.test(draft.startDeviceId.trim())) {
      return { ok: false, message: '起始设备 ID 必须为 20 位数字。' };
    }
    const normalizedDraft = {
      ...draft,
      startDeviceId: draft.startDeviceId.trim(),
      nameTemplate: draft.nameTemplate.trim(),
      manufacturer: draft.manufacturer.trim(),
      model: draft.model.trim(),
      firmwareVersion: draft.firmwareVersion.trim(),
    };
    if (normalizedDraft.nameTemplate.length === 0) {
      return { ok: false, message: '请输入设备名称模板。' };
    }
    if (
      normalizedDraft.manufacturer.length === 0 ||
      normalizedDraft.model.length === 0 ||
      normalizedDraft.firmwareVersion.length === 0
    ) {
      return { ok: false, message: '请填写制造商、设备型号和固件版本。' };
    }
    if (
      !Number.isInteger(normalizedDraft.channelCount) ||
      normalizedDraft.channelCount < 1 ||
      normalizedDraft.channelCount > MAX_CHANNEL_COUNT
    ) {
      return { ok: false, message: `通道数量必须介于 1 到 ${MAX_CHANNEL_COUNT}。` };
    }

    const startDeviceId = BigInt(normalizedDraft.startDeviceId);
    const generatedDevices = Array.from({ length: normalizedDraft.count }, (_, index) => {
      const id = (startDeviceId + BigInt(index)).toString();
      return {
        id,
        name: normalizedDraft.nameTemplate.replace('{序号}', String(index + 1).padStart(3, '0')),
        type: normalizedDraft.type,
        manufacturer: normalizedDraft.manufacturer,
        model: normalizedDraft.model,
        firmwareVersion: normalizedDraft.firmwareVersion,
        channelCount: normalizedDraft.channelCount,
        registrationStatus: 'unregistered',
        createdAt: DEFAULT_CREATED_AT,
      } satisfies SimulatedDevice;
    });

    if (generatedDevices.some((device) => !DEVICE_ID_PATTERN.test(device.id))) {
      return { ok: false, message: '批量生成的设备 ID 超出 20 位数字范围。' };
    }
    const existingIds = new Set(devices.value.map((device) => device.id));
    if (generatedDevices.some((device) => existingIds.has(device.id))) {
      return { ok: false, message: '批量生成的设备 ID 与现有设备重复。' };
    }

    devices.value.push(...generatedDevices);
    channels.value.push(
      ...generatedDevices.flatMap((device) => createChannelsForDevice(device, subscriptions.value)),
    );
    hasCompletedBatchAdd.value = true;
    return { ok: true };
  }

  function deleteDevice(deviceId: string): OperationResult {
    const deviceIndex = devices.value.findIndex((device) => device.id === deviceId);
    if (deviceIndex === -1) {
      return { ok: false, message: '设备不存在或已被删除。' };
    }

    devices.value.splice(deviceIndex, 1);
    subscriptions.value = subscriptions.value.filter(
      (subscription) => subscription.deviceId !== deviceId,
    );
    channels.value = channels.value.filter((channel) => channel.deviceId !== deviceId);
    return { ok: true };
  }

  function registerAllDevices(): OperationResult {
    if (devices.value.length === 0) {
      return { ok: false, message: '当前没有可注册的设备。' };
    }
    devices.value.forEach((device) => {
      device.registrationStatus = 'registered';
    });
    appendRegistrationLogs(devices.value, true);
    return { ok: true };
  }

  function stopAllDeviceRegistration(): OperationResult {
    if (devices.value.length === 0) {
      return { ok: false, message: '当前没有可停止注册的设备。' };
    }
    devices.value.forEach((device) => {
      device.registrationStatus = 'unregistered';
    });
    appendRegistrationLogs(devices.value, false);
    return { ok: true };
  }

  return {
    sipService,
    isSipServiceLoading,
    isSipServiceSaving,
    devices,
    subscriptions,
    channels,
    interactionLogs,
    hasCompletedBatchAdd,
    registeredDeviceCount,
    activeSubscriptionCount,
    updateSipService,
    loadSipService,
    saveSipService,
    updateDevice,
    addDevicesInBatch,
    deleteDevice,
    registerAllDevices,
    stopAllDeviceRegistration,
  };
});
