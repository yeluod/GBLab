import { describe, expect, it } from 'vitest';

import {
  formatInteractionMessage,
  MAX_FORMATTABLE_MESSAGE_LENGTH,
  xmlHighlighter,
} from './interaction-message';

describe('交互消息 XML 展示', () => {
  it('拆分 SIP Header 和 XML Body 并格式化高亮', () => {
    const result = formatInteractionMessage(
      'MESSAGE sip:device SIP/2.0\r\nContent-Type: Application/MANSCDP+xml\r\n\r\n<Notify><CmdType>Catalog</CmdType><!--note--></Notify>',
    );

    expect(result.isXml).toBe(true);
    expect(result.formatted).toBe(true);
    expect(result.headers).toContain('Content-Type: Application/MANSCDP+xml');
    expect(result.formattedBody).toContain('<CmdType>Catalog</CmdType>');
    expect(result.formattedBody).toContain('Catalog');
    expect(result.formattedBody).toContain('<!--note-->');
    expect(
      xmlHighlighter.highlight(result.formattedBody ?? '', { language: 'xml' }).value,
    ).toContain('hljs-tag');
  });

  it('保留 XML 声明并支持独立 XML 消息', () => {
    const result = formatInteractionMessage(
      '<?xml version="1.0" encoding="GB2312"?><Notify><CmdType>Alarm</CmdType></Notify>',
    );

    expect(result.formatted).toBe(true);
    expect(result.headers).toBe('');
    expect(result.formattedBody).toBe(
      '<?xml version="1.0" encoding="GB2312"?>\n<Notify>\n  <CmdType>Alarm</CmdType>\n</Notify>',
    );
  });

  it('纯 SIP 报文保持原始内容并判定为非 XML', () => {
    const message = 'SIP/2.0 200 OK\r\nContent-Length: 0';
    const result = formatInteractionMessage(message);

    expect(result.isXml).toBe(false);
    expect(result.formatted).toBe(false);
    expect(result.body).toBe(message);
  });

  it('非法 XML 回退原始内容并返回原因', () => {
    const message =
      'MESSAGE sip:device SIP/2.0\r\nContent-Type: application/xml\r\n\r\n<Notify><CmdType>Catalog</Notify>';
    const result = formatInteractionMessage(message);

    expect(result.isXml).toBe(true);
    expect(result.formatted).toBe(false);
    expect(result.formattedBody).toBeNull();
    expect(result.error).toContain('XML 解析失败');
  });

  it('超大消息不进入 XML 格式化', () => {
    const body = `<Notify>${'x'.repeat(MAX_FORMATTABLE_MESSAGE_LENGTH)}</Notify>`;
    const result = formatInteractionMessage(
      `MESSAGE sip:device SIP/2.0\r\nContent-Type: application/xml\r\n\r\n${body}`,
    );

    expect(result.isXml).toBe(true);
    expect(result.formatted).toBe(false);
    expect(result.error).toContain('1 MB');
  });
});
