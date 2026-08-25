import { beforeEach, describe, expect, it } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';

import { useSimulatorStore } from './simulator-store';

describe('useSimulatorStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it('应仅更新一份 SIP 服务配置', () => {
    const store = useSimulatorStore();
    const result = store.updateSipService({
      ...store.sipService,
      uri: 'sip:10.10.0.8:5060',
      transport: 'TCP',
    });

    expect(result).toEqual({ ok: true });
    expect(store.sipService).toMatchObject({ uri: 'sip:10.10.0.8:5060', transport: 'TCP' });
  });

  it('批量新增应默认创建未注册设备', () => {
    const store = useSimulatorStore();
    const draft = {
      count: 2,
      startDeviceId: '34020000001320000100',
      nameTemplate: '批量设备-{序号}',
      type: '摄像机',
      manufacturer: 'GBLab',
      model: 'SIM-CAM-100',
      firmwareVersion: 'V1.0.0',
      channelCount: 3,
    } as const;
    const result = store.addDevicesInBatch(draft);

    expect(result).toEqual({ ok: true });
    expect(store.devices.slice(-2).map((device) => device.registrationStatus)).toEqual([
      'unregistered',
      'unregistered',
    ]);
    expect(store.channels.filter((channel) => channel.deviceId.endsWith('100'))).toHaveLength(3);
    expect(store.hasCompletedBatchAdd).toBe(true);
    expect(store.addDevicesInBatch(draft)).toEqual({
      ok: false,
      message: '设备仅允许批量添加一次。',
    });
  });

  it('编辑设备时应更新可编辑字段', () => {
    const store = useSimulatorStore();
    const target = store.devices[0];
    if (target === undefined) {
      throw new Error('演示设备未初始化');
    }

    const result = store.updateDevice(target.id, {
      name: '重命名设备',
      type: '球机',
      manufacturer: 'GBLab',
      model: 'SIM-PTZ-200',
      firmwareVersion: 'V2.0.0',
      channelCount: 4,
    });

    expect(result).toEqual({ ok: true });
    expect(store.devices[0]).toMatchObject({
      name: '重命名设备',
      type: '球机',
      manufacturer: 'GBLab',
      model: 'SIM-PTZ-200',
      firmwareVersion: 'V2.0.0',
      channelCount: 4,
      registrationStatus: 'unregistered',
    });
    expect(store.channels.filter((channel) => channel.deviceId === target.id)).toHaveLength(4);
  });

  it('删除设备时应同步删除关联订阅', () => {
    const store = useSimulatorStore();
    const deviceId = '34020000001320000001';

    const result = store.deleteDevice(deviceId);

    expect(result).toEqual({ ok: true });
    expect(store.devices.some((device) => device.id === deviceId)).toBe(false);
    expect(store.subscriptions.some((subscription) => subscription.deviceId === deviceId)).toBe(
      false,
    );
    expect(store.channels.some((channel) => channel.deviceId === deviceId)).toBe(false);
  });

  it('模拟通道 ID 应为 20 位数字', () => {
    const store = useSimulatorStore();

    expect(store.channels.every((channel) => /^\d{20}$/.test(channel.id))).toBe(true);
  });

  it('应全量注册和停止注册，并为所有设备记录交互日志', () => {
    const store = useSimulatorStore();
    const deviceCount = store.devices.length;
    const initialLogCount = store.interactionLogs.length;
    const result = store.registerAllDevices();

    expect(result).toEqual({ ok: true });
    expect(store.devices.every((device) => device.registrationStatus === 'registered')).toBe(true);
    expect(store.interactionLogs).toHaveLength(initialLogCount + deviceCount);
    expect(store.interactionLogs.at(-1)?.message).toContain('设备已请求注册');
    expect(store.interactionLogs.at(-1)?.channelId).toMatch(/^\d{20}$/);

    const stopResult = store.stopAllDeviceRegistration();

    expect(stopResult).toEqual({ ok: true });
    expect(store.devices.every((device) => device.registrationStatus === 'unregistered')).toBe(
      true,
    );
    expect(store.interactionLogs).toHaveLength(initialLogCount + deviceCount * 2);
    expect(store.interactionLogs.at(-1)?.message).toContain('Expires: 0');
  });
});
