import { describe, expect, it } from 'vitest';

import { getAlarmTypeOptions, isValidAlarmSelection } from './alarm-options';

describe('GB28181 报警字典', () => {
  it('设备报警应支持默认类型和五种标准类型', () => {
    expect(getAlarmTypeOptions('2').map((option) => option.value)).toEqual([
      '',
      '1',
      '2',
      '3',
      '4',
      '5',
    ]);
  });

  it('视频报警应包含 2022 版图像遮挡报警', () => {
    expect(getAlarmTypeOptions('5').at(-1)).toEqual({
      label: '图像遮挡报警（13，GB/T 28181-2022）',
      value: '13',
    });
  });

  it('非设备视频故障报警不得携带报警类型', () => {
    expect(isValidAlarmSelection('1', '1', '')).toBe(true);
    expect(isValidAlarmSelection('1', '1', '1')).toBe(false);
  });
});
