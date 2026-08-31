import { invoke } from '@tauri-apps/api/core';

/** 保留 Tauri Command 返回的稳定错误码和可诊断消息。 */
export class TauriCommandError extends Error {
  constructor(
    public readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = 'TauriCommandError';
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

/** 将 Tauri 的序列化错误统一转换为标准 Error，避免业务层丢失后端消息。 */
export function normalizeInvokeError(error: unknown): Error {
  if (error instanceof Error) return error;
  if (isRecord(error) && typeof error.message === 'string') {
    return new TauriCommandError(
      typeof error.code === 'string' ? error.code : 'command_error',
      error.message,
    );
  }
  if (typeof error === 'string' && error.length > 0) return new Error(error);
  return new Error('桌面命令执行失败。');
}

/** 调用类型化 Tauri Command。 */
export async function invokeCommand<TResult>(
  command: string,
  args?: Record<string, unknown>,
): Promise<TResult> {
  try {
    return await invoke<TResult>(command, args);
  } catch (error) {
    throw normalizeInvokeError(error);
  }
}
