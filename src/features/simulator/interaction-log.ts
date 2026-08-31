import type { InteractionLog } from './types';

export type InteractionMessageType =
  | { kind: 'sip-request'; method: string; label: string }
  | { kind: 'sip-response'; status: number; label: string }
  | { kind: 'gb-command'; commandType: string; label: string }
  | { kind: 'other'; label: string };

const SIP_REQUEST_METHODS = new Set([
  'REGISTER',
  'MESSAGE',
  'SUBSCRIBE',
  'NOTIFY',
  'INVITE',
  'ACK',
  'BYE',
  'CANCEL',
  'OPTIONS',
]);

function extractCommandType(message: string): string | null {
  const match = message.match(/<CmdType>\s*([^<]+?)\s*<\/CmdType>/i);
  const commandType = match?.[1]?.trim();
  return commandType === undefined || commandType.length === 0 ? null : commandType;
}

/** 仅用于日志展示，不参与 SIP 或 XML 业务语义判断。 */
export function classifyInteractionMessage(message: string): InteractionMessageType {
  const firstLine = message.split(/\r?\n/, 1)[0]?.trim() ?? '';
  const responseMatch = firstLine.match(/^SIP\/2\.0\s+(\d{3})(?:\s|$)/i);
  if (responseMatch !== null) {
    const status = Number(responseMatch[1]);
    return { kind: 'sip-response', status, label: `SIP 响应 · ${status}` };
  }

  const requestMatch = firstLine.match(/^([A-Z]+)\s+\S+\s+SIP\/2\.0(?:\s|$)/i);
  const method = requestMatch?.[1]?.toUpperCase();
  const commandType = extractCommandType(message);
  if (method !== undefined && SIP_REQUEST_METHODS.has(method)) {
    return {
      kind: 'sip-request',
      method,
      label: commandType === null ? `SIP 请求 · ${method}` : `${method} · ${commandType}`,
    };
  }

  if (commandType !== null) {
    return { kind: 'gb-command', commandType, label: `GB28181 · ${commandType}` };
  }

  return { kind: 'other', label: '其他' };
}

export function formatTimestamp(timestamp: number): string {
  return new Date(timestamp).toLocaleString('zh-CN', { hour12: false });
}

export function directionLabel(direction: InteractionLog['direction']): string {
  return direction === 'send' ? '设备 → 服务' : '服务 → 设备';
}

export function formatLogsAsTsv(logs: readonly InteractionLog[]): string {
  return [
    ['时间', '方向', '消息类型', '设备 ID', '通道 ID', '消息'].join('\t'),
    ...logs.map((log) =>
      [
        formatTimestamp(log.timestamp),
        directionLabel(log.direction),
        classifyInteractionMessage(log.message).label,
        log.deviceId,
        log.channelId ?? '—',
        log.message,
      ].join('\t'),
    ),
  ].join('\n');
}
