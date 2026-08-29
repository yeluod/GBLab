import { describe, expect, it } from 'vitest';

import { TauriCommandError, normalizeInvokeError } from './invoke-command';

describe('Tauri Command 错误归一化', () => {
  it('保留序列化错误的错误码和消息', () => {
    const error = normalizeInvokeError({ code: 'media_error', message: '摄像头不可用。' });

    expect(error).toBeInstanceOf(TauriCommandError);
    expect(error.message).toBe('摄像头不可用。');
    expect((error as TauriCommandError).code).toBe('media_error');
  });

  it('保留标准 Error 实例', () => {
    const original = new Error('原始错误');

    expect(normalizeInvokeError(original)).toBe(original);
  });
});
