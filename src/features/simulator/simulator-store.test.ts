import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

import type { DeviceSnapshot, SubscriptionSnapshot } from './types';

const deviceApiMocks = vi.hoisted(() => ({
  getDeviceSnapshot: vi.fn(),
  getDeviceChannels: vi.fn(),
  addDevicesInBatchCommand: vi.fn(),
  updateDeviceCommand: vi.fn(),
  deleteDeviceCommand: vi.fn(),
}));

const registrationApiMocks = vi.hoisted(() => ({
  getRegistrationSnapshot: vi.fn(),
  getRegistrationDeviceStates: vi.fn(),
  listenRegistrationSnapshot: vi.fn(),
  listenRegistrationDeviceStates: vi.fn(),
  listenRegistrationSubscriptions: vi.fn(),
  listenInteractionLogs: vi.fn(),
  registerAllDevicesCommand: vi.fn(),
  stopAllDeviceRegistrationCommand: vi.fn(),
  triggerAlarmCommand: vi.fn(),
  triggerMobilePositionCommand: vi.fn(),
  controlDeviceCommand: vi.fn(),
  controlPtzCommand: vi.fn(),
}));

vi.mock('./device-api', () => deviceApiMocks);
vi.mock('./registration-api', () => registrationApiMocks);

import { useSimulatorStore } from './simulator-store';

let subscriptionListener: ((subscriptions: SubscriptionSnapshot[]) => void) | null = null;

function initialSnapshot(): DeviceSnapshot {
  return {
    devices: [
      {
        id: '34020000001320000001',
        name: '模拟摄像机-001',
        type: '摄像机',
        manufacturer: 'GBLab',
        model: 'SIM-CAM-100',
        firmwareVersion: 'V1.0.0',
        channelCount: 2,
        registrationStatus: 'unregistered',
        createdAt: 1_777_777_777_000,
      },
    ],
    hasCompletedBatchAdd: false,
  };
}

function initialChannels() {
  return [1, 2].map((index) => ({
    id: `34020000001320001${String(index).padStart(3, '0')}`,
    deviceId: '34020000001320000001',
    name: `模拟摄像机-001 · 通道 ${String(index).padStart(2, '0')}`,
    index,
    platformSubscriptions: [],
  }));
}

describe('useSimulatorStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    subscriptionListener = null;
    deviceApiMocks.getDeviceSnapshot.mockResolvedValue(initialSnapshot());
    deviceApiMocks.getDeviceChannels.mockResolvedValue(initialChannels());
    registrationApiMocks.getRegistrationSnapshot.mockResolvedValue({
      operationStatus: 'idle',
      operationId: null,
      totalDevices: 0,
      registeredCount: 0,
      failedCount: 0,
      activeSubscriptions: 0,
      droppedLogs: 0,
    });
    registrationApiMocks.getRegistrationDeviceStates.mockResolvedValue([]);
    registrationApiMocks.listenRegistrationSnapshot.mockResolvedValue(() => undefined);
    registrationApiMocks.listenRegistrationDeviceStates.mockResolvedValue(() => undefined);
    registrationApiMocks.listenRegistrationSubscriptions.mockImplementation(async (listener) => {
      subscriptionListener = listener;
      return () => undefined;
    });
    registrationApiMocks.listenInteractionLogs.mockResolvedValue(() => undefined);
    registrationApiMocks.registerAllDevicesCommand.mockResolvedValue({
      operationId: '1',
      total: 1,
    });
    registrationApiMocks.stopAllDeviceRegistrationCommand.mockResolvedValue({
      operationId: '1',
      total: 1,
    });
  });

  it('应从桌面后端加载设备及派生通道', async () => {
    const store = useSimulatorStore();

    expect(await store.loadDevices()).toEqual({ ok: true });
    expect(await store.loadDeviceChannels('34020000001320000001')).toEqual({ ok: true });
    expect(store.devices).toHaveLength(1);
    expect(store.channels.map((channel) => channel.id)).toEqual([
      '34020000001320001001',
      '34020000001320001002',
    ]);
    expect(store.devices[0]?.registrationStatus).toBe('unregistered');
  });

  it('批量新增应采用后端快照并保持默认未注册', async () => {
    const store = useSimulatorStore();
    await store.loadDevices();
    const next = initialSnapshot();
    next.hasCompletedBatchAdd = true;
    next.devices.push({
      id: '34020000001320000100',
      name: '批量设备-001',
      type: '摄像机',
      manufacturer: 'GBLab',
      model: 'SIM-CAM-100',
      firmwareVersion: 'V1.0.0',
      channelCount: 3,
      registrationStatus: 'unregistered',
      createdAt: 1_777_777_778_000,
    });
    deviceApiMocks.getDeviceChannels.mockResolvedValue(
      [1, 2, 3].map((index) => ({
        id: `34020000001320100${String(index).padStart(3, '0')}`,
        deviceId: '34020000001320000100',
        name: `批量设备-001 · 通道 ${String(index).padStart(2, '0')}`,
        index,
        platformSubscriptions: [] as [],
      })),
    );
    deviceApiMocks.addDevicesInBatchCommand.mockResolvedValue(next);
    const draft = {
      count: 1,
      startDeviceId: '34020000001320000100',
      nameTemplate: '批量设备-{序号}',
      type: '摄像机',
      manufacturer: 'GBLab',
      model: 'SIM-CAM-100',
      firmwareVersion: 'V1.0.0',
      channelCount: 3,
    } as const;

    expect(await store.addDevicesInBatch(draft)).toEqual({ ok: true });
    expect(await store.loadDeviceChannels(draft.startDeviceId)).toEqual({ ok: true });
    expect(store.devices.at(-1)?.registrationStatus).toBe('unregistered');
    expect(
      store.channels.filter((channel) => channel.deviceId === draft.startDeviceId),
    ).toHaveLength(3);
    expect(store.hasCompletedBatchAdd).toBe(true);
    expect(await store.addDevicesInBatch(draft)).toEqual({
      ok: false,
      message: '设备仅允许批量添加一次。',
    });
  });

  it('编辑持久化设备时应保留运行时注册状态', async () => {
    const store = useSimulatorStore();
    await store.loadDevices();
    const next = initialSnapshot();
    const device = next.devices[0];
    if (device === undefined) throw new Error('测试设备未初始化');
    Object.assign(device, { name: '重命名设备', type: '球机', channelCount: 4 });
    deviceApiMocks.getDeviceChannels.mockResolvedValue(
      [1, 2, 3, 4].map((index) => ({
        id: `34020000001320001${String(index).padStart(3, '0')}`,
        deviceId: device.id,
        name: `重命名设备 · 通道 ${String(index).padStart(2, '0')}`,
        index,
        platformSubscriptions: [],
      })),
    );
    deviceApiMocks.updateDeviceCommand.mockResolvedValue(next);

    expect(
      await store.updateDevice(device.id, {
        name: '重命名设备',
        type: '球机',
        manufacturer: 'GBLab',
        model: 'SIM-PTZ-200',
        firmwareVersion: 'V2.0.0',
        channelCount: 4,
      }),
    ).toEqual({ ok: true });
    await store.loadDeviceChannels(device.id);
    expect(store.devices[0]).toMatchObject({
      name: '重命名设备',
      channelCount: 4,
      registrationStatus: 'unregistered',
    });
    expect(store.channels).toHaveLength(4);
  });

  it('删除设备时应采用后端快照', async () => {
    const store = useSimulatorStore();
    await store.loadDevices();
    deviceApiMocks.deleteDeviceCommand.mockResolvedValue({
      devices: [],
      hasCompletedBatchAdd: false,
    });

    expect(await store.deleteDevice('34020000001320000001')).toEqual({ ok: true });
    expect(store.devices).toHaveLength(0);
    expect(store.channels).toHaveLength(0);
  });

  it('应全量注册和停止注册，并为所有设备记录运行时日志', async () => {
    const store = useSimulatorStore();
    await store.loadDevices();
    await store.loadDeviceChannels('34020000001320000001');

    expect(await store.registerAllDevices()).toEqual({ ok: true });
    expect(store.devices.every((device) => device.registrationStatus === 'queued')).toBe(true);
    expect(store.interactionLogs).toHaveLength(0);

    expect(await store.stopAllDeviceRegistration()).toEqual({ ok: true });
    expect(store.registrationOperationStatus).toBe('stopping');
  });

  it('订阅变化应实时同步设备级和通道级业务能力', async () => {
    const store = useSimulatorStore();
    await store.loadDevices();
    await store.loadDeviceChannels('34020000001320000001');
    const listener = subscriptionListener;
    if (listener === null) throw new Error('订阅监听器未初始化');

    listener([
      {
        deviceId: '34020000001320000001',
        channelId: null,
        commandType: 'alarm',
        callId: 'alarm-call',
        status: 'active',
        expiresAt: Date.now() + 60_000,
        lastNotifiedAt: null,
        lastError: null,
      },
      {
        deviceId: '34020000001320000001',
        channelId: '34020000001320001001',
        commandType: 'mobilePosition',
        callId: 'position-call',
        status: 'active',
        expiresAt: Date.now() + 60_000,
        lastNotifiedAt: null,
        lastError: null,
      },
    ]);

    expect(store.channels[0]?.platformSubscriptions).toEqual(['alarm', 'mobile-position']);
    expect(store.channels[1]?.platformSubscriptions).toEqual(['alarm']);
  });

  it('触发报警应提交平台要求的完整字段', async () => {
    registrationApiMocks.getRegistrationSnapshot.mockResolvedValue({
      operationStatus: 'running',
      operationId: 'registration-1',
      totalDevices: 1,
      registeredCount: 1,
      failedCount: 0,
      activeSubscriptions: 1,
      droppedLogs: 0,
    });
    registrationApiMocks.getRegistrationDeviceStates.mockResolvedValue([
      {
        deviceId: '34020000001320000001',
        status: 'registered',
        lastError: null,
        expiresAt: Date.now() + 60_000,
      },
    ]);
    registrationApiMocks.triggerAlarmCommand.mockResolvedValue(undefined);
    const store = useSimulatorStore();

    await store.loadDevices();

    expect(
      await store.triggerAlarm(
        '34020000001320000001',
        '34020000001320001001',
        '1',
        '2',
        '3',
        'Occur',
        '模拟报警',
        116.397,
        39.908,
      ),
    ).toEqual({ ok: true });
    expect(registrationApiMocks.triggerAlarmCommand).toHaveBeenCalledWith(
      '34020000001320000001',
      '34020000001320001001',
      '1',
      '2',
      '3',
      'Occur',
      '模拟报警',
      116.397,
      39.908,
    );
  });
});
