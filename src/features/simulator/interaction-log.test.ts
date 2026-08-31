import { describe, expect, it } from 'vitest';

import {
  classifyInteractionMessage,
  formatLogsAsTsv,
  type InteractionMessageType,
} from './interaction-log';

describe('交互日志展示辅助函数', () => {
  it('识别 SIP 请求方法和 GB28181 命令类型', () => {
    expect(
      classifyInteractionMessage('REGISTER sip:platform SIP/2.0'),
    ).toEqual<InteractionMessageType>({
      kind: 'sip-request',
      method: 'REGISTER',
      label: 'SIP 请求 · REGISTER',
    });
    expect(
      classifyInteractionMessage('<Notify><CmdType>Catalog</CmdType></Notify>'),
    ).toEqual<InteractionMessageType>({
      kind: 'gb-command',
      commandType: 'Catalog',
      label: 'GB28181 · Catalog',
    });
    expect(
      classifyInteractionMessage('MESSAGE sip:platform SIP/2.0\r\n\r\n<CmdType>Alarm</CmdType>'),
    ).toEqual<InteractionMessageType>({
      kind: 'sip-request',
      method: 'MESSAGE',
      label: 'MESSAGE · Alarm',
    });
  });

  it('识别 SIP 响应和未知消息', () => {
    expect(classifyInteractionMessage('SIP/2.0 400 Bad Request')).toEqual<InteractionMessageType>({
      kind: 'sip-response',
      status: 400,
      label: 'SIP 响应 · 400',
    });
    expect(classifyInteractionMessage('not a SIP message')).toEqual<InteractionMessageType>({
      kind: 'other',
      label: '其他',
    });
  });

  it('复制格式保留完整多行消息并加入消息类型', () => {
    const message = 'SIP/2.0 200 OK\nContent-Length: 0';
    const result = formatLogsAsTsv([
      {
        id: 'log-1',
        timestamp: 0,
        deviceId: 'device-1',
        channelId: null,
        direction: 'receive',
        message,
      },
    ]);

    expect(result).toContain('消息类型');
    expect(result).toContain('SIP 响应 · 200');
    expect(result).toContain(message);
  });
});
