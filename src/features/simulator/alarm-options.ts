export interface AlarmOption {
  label: string;
  value: string;
  [key: string]: unknown;
}

export const alarmPriorityOptions: AlarmOption[] = [
  { label: '一级警情（1）', value: '1' },
  { label: '二级警情（2）', value: '2' },
  { label: '三级警情（3）', value: '3' },
  { label: '四级警情（4）', value: '4' },
];

export const alarmMethodOptions: AlarmOption[] = [
  { label: '电话报警（1）', value: '1' },
  { label: '设备报警（2）', value: '2' },
  { label: '短信报警（3）', value: '3' },
  { label: 'GPS 报警（4）', value: '4' },
  { label: '视频报警（5）', value: '5' },
  { label: '设备故障报警（6）', value: '6' },
  { label: '其他报警（7）', value: '7' },
];

const deviceAlarmTypeOptions: AlarmOption[] = [
  { label: '默认设备报警（不发送 AlarmType）', value: '' },
  { label: '视频丢失报警（1）', value: '1' },
  { label: '设备防拆报警（2）', value: '2' },
  { label: '存储设备磁盘满报警（3）', value: '3' },
  { label: '设备高温报警（4）', value: '4' },
  { label: '设备低温报警（5）', value: '5' },
];

const videoAlarmTypeOptions: AlarmOption[] = [
  { label: '人工视频报警（1）', value: '1' },
  { label: '运动目标检测报警（2）', value: '2' },
  { label: '遗留物检测报警（3）', value: '3' },
  { label: '物体移除检测报警（4）', value: '4' },
  { label: '绊线检测报警（5）', value: '5' },
  { label: '入侵检测报警（6）', value: '6' },
  { label: '逆行检测报警（7）', value: '7' },
  { label: '徘徊检测报警（8）', value: '8' },
  { label: '流量统计报警（9）', value: '9' },
  { label: '密度检测报警（10）', value: '10' },
  { label: '视频异常检测报警（11）', value: '11' },
  { label: '快速移动报警（12）', value: '12' },
  { label: '图像遮挡报警（13，GB/T 28181-2022）', value: '13' },
];

const deviceFaultAlarmTypeOptions: AlarmOption[] = [
  { label: '存储设备磁盘故障报警（1）', value: '1' },
  { label: '存储设备风扇故障报警（2）', value: '2' },
];

export function getAlarmTypeOptions(alarmMethod: string): AlarmOption[] {
  switch (alarmMethod) {
    case '2':
      return deviceAlarmTypeOptions;
    case '5':
      return videoAlarmTypeOptions;
    case '6':
      return deviceFaultAlarmTypeOptions;
    default:
      return [];
  }
}

export function isValidAlarmSelection(
  alarmPriority: string,
  alarmMethod: string,
  alarmType: string,
): boolean {
  if (!alarmPriorityOptions.some((option) => option.value === alarmPriority)) return false;
  if (!alarmMethodOptions.some((option) => option.value === alarmMethod)) return false;
  const typeOptions = getAlarmTypeOptions(alarmMethod);
  return typeOptions.length === 0
    ? alarmType === ''
    : typeOptions.some((option) => option.value === alarmType);
}
