import { invoke } from '@tauri-apps/api/core';

/** 调用类型化 Tauri Command。 */
export function invokeCommand<TResult>(
  command: string,
  args?: Record<string, unknown>,
): Promise<TResult> {
  return invoke<TResult>(command, args);
}
