import { invokeCommand, listenEvent } from '@/infrastructure/tauri';

import type { BatchOperationAccepted, InteractionLog, RegistrationSnapshot } from './types';

/** 读取不落盘的注册运行时快照。 */
export function getRegistrationSnapshot(): Promise<RegistrationSnapshot> {
  return invokeCommand<RegistrationSnapshot>('get_registration_snapshot');
}

/** 发起当前全部设备的注册生命周期。 */
export function registerAllDevicesCommand(): Promise<BatchOperationAccepted> {
  return invokeCommand<BatchOperationAccepted>('register_all_devices');
}

/** 停止全部设备注册并发送 Expires 为 0 的 REGISTER。 */
export function stopAllDeviceRegistrationCommand(): Promise<BatchOperationAccepted> {
  return invokeCommand<BatchOperationAccepted>('stop_all_device_registration');
}

export function triggerAlarmCommand(
  deviceId: string,
  channelId: string,
  alarmType: string,
  description: string,
): Promise<void> {
  return invokeCommand('trigger_alarm', { deviceId, channelId, alarmType, description });
}

export function triggerMobilePositionCommand(
  deviceId: string,
  channelId: string,
  longitude: number,
  latitude: number,
): Promise<void> {
  return invokeCommand('trigger_mobile_position', {
    deviceId,
    channelId,
    longitude,
    latitude,
  });
}

/** 订阅降频后的注册状态快照。 */
export function listenRegistrationSnapshot(
  handler: (snapshot: RegistrationSnapshot) => void,
): Promise<() => void> {
  return listenEvent('registration-snapshot', handler);
}

/** 订阅批量原始 SIP 交互日志。 */
export function listenInteractionLogs(
  handler: (logs: Array<Omit<InteractionLog, 'id'> & { sequence: number }>) => void,
): Promise<() => void> {
  return listenEvent('sip-interaction-logs', handler);
}
