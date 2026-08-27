import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeCommand = vi.hoisted(() => vi.fn());
vi.mock('@/infrastructure/tauri', () => ({ invokeCommand }));

import { registerAllDevicesCommand } from './registration-api';

describe('registration-api', () => {
  beforeEach(() => invokeCommand.mockReset());

  it('应将全量注册命令交给 Rust 运行时', async () => {
    invokeCommand.mockResolvedValue({ operationId: '1', total: 2 });

    await registerAllDevicesCommand();

    expect(invokeCommand).toHaveBeenCalledWith('register_all_devices');
  });
});
