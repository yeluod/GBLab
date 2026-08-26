import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeCommand = vi.hoisted(() => vi.fn());
vi.mock('@/infrastructure/tauri', () => ({ invokeCommand }));

import {
  addDevicesInBatchCommand,
  deleteDeviceCommand,
  getDeviceChannels,
  getDevicePage,
  getDeviceSnapshot,
  updateDeviceCommand,
} from './device-api';

describe('device-api', () => {
  beforeEach(() => invokeCommand.mockReset());

  it('应使用类型化命令读写设备配置', async () => {
    invokeCommand.mockResolvedValue({ devices: [], channels: [], hasCompletedBatchAdd: false });
    const batch = {
      count: 1,
      startDeviceId: '34020000001320000100',
      nameTemplate: '设备-{序号}',
      type: '摄像机',
      manufacturer: 'GBLab',
      model: 'SIM-100',
      firmwareVersion: 'V1.0.0',
      channelCount: 1,
    } as const;
    const update = {
      name: '设备-001',
      type: '摄像机',
      manufacturer: 'GBLab',
      model: 'SIM-100',
      firmwareVersion: 'V1.0.0',
      channelCount: 2,
    } as const;

    await getDeviceSnapshot();
    await getDevicePage({ offset: 0, limit: 10, filter: '摄像机', sort: 'id-asc' });
    await getDeviceChannels(batch.startDeviceId);
    await addDevicesInBatchCommand(batch);
    await updateDeviceCommand(batch.startDeviceId, update);
    await deleteDeviceCommand(batch.startDeviceId);

    expect(invokeCommand).toHaveBeenNthCalledWith(1, 'get_device_snapshot');
    expect(invokeCommand).toHaveBeenNthCalledWith(2, 'get_device_page', {
      offset: 0,
      limit: 10,
      filter: '摄像机',
      sort: 'id-asc',
    });
    expect(invokeCommand).toHaveBeenNthCalledWith(3, 'get_device_channels', {
      deviceId: batch.startDeviceId,
    });
    expect(invokeCommand).toHaveBeenNthCalledWith(4, 'add_devices_in_batch', { draft: batch });
    expect(invokeCommand).toHaveBeenNthCalledWith(5, 'update_device', {
      deviceId: batch.startDeviceId,
      draft: update,
    });
    expect(invokeCommand).toHaveBeenNthCalledWith(6, 'delete_device', {
      deviceId: batch.startDeviceId,
    });
  });
});
