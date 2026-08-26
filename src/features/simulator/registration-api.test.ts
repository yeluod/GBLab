import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeCommand = vi.hoisted(() => vi.fn());
vi.mock('@/infrastructure/tauri', () => ({ invokeCommand }));

import { getInteractionLogPage } from './registration-api';

describe('registration-api', () => {
  beforeEach(() => invokeCommand.mockReset());

  it('应将日志分页和过滤条件交给 Rust 查询接口', async () => {
    invokeCommand.mockResolvedValue({ items: [], total: 0, offset: 20, limit: 20 });
    const query = {
      offset: 20,
      limit: 20,
      deviceId: '34020000002000000100',
      direction: 'receive' as const,
      method: 'MESSAGE',
      keyword: 'Catalog',
    };

    await getInteractionLogPage(query);

    expect(invokeCommand).toHaveBeenCalledWith('get_interaction_log_page', { query });
  });
});
