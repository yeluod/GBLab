import { invokeCommand } from '@/infrastructure/tauri';

import type {
  BatchDeviceDraft,
  DeviceSnapshot,
  DeviceUpdateDraft,
  SimulatedChannel,
} from './types';

/** 读取持久化设备及其即时派生通道。 */
export function getDeviceSnapshot(): Promise<DeviceSnapshot> {
  return invokeCommand<DeviceSnapshot>('get_device_snapshot');
}

/** 按需读取单台设备的运行时派生通道。 */
export function getDeviceChannels(deviceId: string): Promise<SimulatedChannel[]> {
  return invokeCommand<SimulatedChannel[]>('get_device_channels', { deviceId });
}

/** 执行唯一一次批量添加并持久化设备。 */
export function addDevicesInBatchCommand(draft: BatchDeviceDraft): Promise<DeviceSnapshot> {
  return invokeCommand<DeviceSnapshot>('add_devices_in_batch', { draft });
}

/** 清空全部设备配置并重新开放一次批量添加。 */
export function clearDevicesCommand(): Promise<DeviceSnapshot> {
  return invokeCommand<DeviceSnapshot>('clear_devices');
}

/** 编辑并持久化指定设备。 */
export function updateDeviceCommand(
  deviceId: string,
  draft: DeviceUpdateDraft,
): Promise<DeviceSnapshot> {
  return invokeCommand<DeviceSnapshot>('update_device', { deviceId, draft });
}

/** 删除并持久化指定设备。 */
export function deleteDeviceCommand(deviceId: string): Promise<DeviceSnapshot> {
  return invokeCommand<DeviceSnapshot>('delete_device', { deviceId });
}
