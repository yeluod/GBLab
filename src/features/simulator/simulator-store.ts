import { computed, ref } from 'vue';
import { defineStore } from 'pinia';

import type {
  BatchDeviceDraft,
  DeviceDraft,
  DeviceSubscription,
  OperationResult,
  SimulatedDevice,
  SipServiceConfig,
} from './types';

const DEVICE_ID_PATTERN = /^\d{20}$/;
const MAX_BATCH_DEVICE_COUNT = 1_000;
const DEFAULT_CREATED_AT = '2026-08-25 14:20:00';

function createInitialDevices(): SimulatedDevice[] {
  return [
    { id: '34020000001320000001', name: '模拟摄像机-001', type: '摄像机', isEnabled: false, createdAt: DEFAULT_CREATED_AT },
    { id: '34020000001320000002', name: '模拟摄像机-002', type: '摄像机', isEnabled: false, createdAt: DEFAULT_CREATED_AT },
    { id: '34020000001320000003', name: '园区球机-001', type: '球机', isEnabled: true, createdAt: DEFAULT_CREATED_AT },
    { id: '34020000001320000004', name: '仓库 NVR-001', type: 'NVR', isEnabled: true, createdAt: DEFAULT_CREATED_AT },
    { id: '34020000001320000005', name: '门禁设备-001', type: '门禁设备', isEnabled: false, createdAt: DEFAULT_CREATED_AT },
  ];
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
      catalogPreview: ['34020000001320000011 · 主码流', '34020000001320000012 · 子码流'],
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
      catalogPreview: ['34020000001320000031 · 园区入口'],
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

function normalizeDeviceDraft(draft: DeviceDraft): DeviceDraft {
  return {
    ...draft,
    id: draft.id.trim(),
    name: draft.name.trim(),
  };
}

function validateDeviceDraft(draft: DeviceDraft, isDuplicate: boolean): OperationResult {
  if (!DEVICE_ID_PATTERN.test(draft.id)) {
    return { ok: false, message: '设备 ID 必须为 20 位数字。' };
  }
  if (draft.name.length === 0) {
    return { ok: false, message: '请输入设备名称。' };
  }
  if (isDuplicate) {
    return { ok: false, message: '设备 ID 已存在。' };
  }
  return { ok: true };
}

/**
 * 静态前端演示数据源。后续接入桌面核心时，以同名操作替换为类型化 IPC 调用。
 */
export const useSimulatorStore = defineStore('simulator', () => {
  const sipService = ref<SipServiceConfig>({
    uri: 'sip:192.168.1.100:5060',
    transport: 'UDP',
    platformId: '34020000002000000001',
    domain: '3402000000',
    registerExpires: 3_600,
    keepaliveInterval: 60,
  });
  const devices = ref<SimulatedDevice[]>(createInitialDevices());
  const subscriptions = ref<DeviceSubscription[]>(createInitialSubscriptions());

  const enabledDeviceCount = computed(() => devices.value.filter((device) => device.isEnabled).length);
  const activeSubscriptionCount = computed(
    () => subscriptions.value.filter((subscription) => subscription.status === 'active').length,
  );

  function updateSipService(config: SipServiceConfig): OperationResult {
    if (!config.uri.trim().startsWith('sip:')) {
      return { ok: false, message: 'SIP 地址必须以 sip: 开头。' };
    }
    if (!DEVICE_ID_PATTERN.test(config.platformId)) {
      return { ok: false, message: '平台 ID 必须为 20 位数字。' };
    }
    if (config.domain.trim().length === 0 || config.registerExpires <= 0 || config.keepaliveInterval <= 0) {
      return { ok: false, message: '请填写有效的服务配置。' };
    }

    sipService.value = { ...config, uri: config.uri.trim(), domain: config.domain.trim() };
    return { ok: true };
  }

  function addDevice(draft: DeviceDraft): OperationResult {
    const normalizedDraft = normalizeDeviceDraft(draft);
    const validation = validateDeviceDraft(
      normalizedDraft,
      devices.value.some((device) => device.id === normalizedDraft.id),
    );
    if (!validation.ok) {
      return validation;
    }

    devices.value.push({ ...normalizedDraft, createdAt: DEFAULT_CREATED_AT });
    return { ok: true };
  }

  function updateDevice(deviceId: string, draft: Omit<DeviceDraft, 'id'>): OperationResult {
    const device = devices.value.find((item) => item.id === deviceId);
    if (device === undefined) {
      return { ok: false, message: '设备不存在或已被删除。' };
    }
    if (draft.name.trim().length === 0) {
      return { ok: false, message: '请输入设备名称。' };
    }

    Object.assign(device, { ...draft, name: draft.name.trim() });
    return { ok: true };
  }

  function addDevicesInBatch(draft: BatchDeviceDraft): OperationResult {
    if (!Number.isInteger(draft.count) || draft.count < 1 || draft.count > MAX_BATCH_DEVICE_COUNT) {
      return { ok: false, message: `设备数量必须介于 1 到 ${MAX_BATCH_DEVICE_COUNT}。` };
    }
    if (!DEVICE_ID_PATTERN.test(draft.startDeviceId.trim())) {
      return { ok: false, message: '起始设备 ID 必须为 20 位数字。' };
    }
    if (draft.nameTemplate.trim().length === 0) {
      return { ok: false, message: '请输入设备名称模板。' };
    }

    const startDeviceId = BigInt(draft.startDeviceId.trim());
    const generatedDevices = Array.from({ length: draft.count }, (_, index) => {
      const id = (startDeviceId + BigInt(index)).toString();
      return {
        id,
        name: draft.nameTemplate.replace('{序号}', String(index + 1).padStart(3, '0')),
        type: draft.type,
        isEnabled: draft.isEnabled,
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
    return { ok: true };
  }

  function deleteDevice(deviceId: string): OperationResult {
    const deviceIndex = devices.value.findIndex((device) => device.id === deviceId);
    if (deviceIndex === -1) {
      return { ok: false, message: '设备不存在或已被删除。' };
    }

    devices.value.splice(deviceIndex, 1);
    subscriptions.value = subscriptions.value.filter((subscription) => subscription.deviceId !== deviceId);
    return { ok: true };
  }

  function toggleDevice(deviceId: string): OperationResult {
    const device = devices.value.find((item) => item.id === deviceId);
    if (device === undefined) {
      return { ok: false, message: '设备不存在或已被删除。' };
    }

    device.isEnabled = !device.isEnabled;
    return { ok: true };
  }

  return {
    sipService,
    devices,
    subscriptions,
    enabledDeviceCount,
    activeSubscriptionCount,
    updateSipService,
    addDevice,
    updateDevice,
    addDevicesInBatch,
    deleteDevice,
    toggleDevice,
  };
});
