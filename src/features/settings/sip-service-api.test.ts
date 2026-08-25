import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeCommand = vi.hoisted(() => vi.fn());

vi.mock('@/infrastructure/tauri', () => ({ invokeCommand }));

import { getSipServiceConfiguration, saveSipServiceConfiguration } from './sip-service-api';

describe('SIP 服务配置 IPC', () => {
  beforeEach(() => {
    invokeCommand.mockReset();
  });

  it('应调用读取配置命令', async () => {
    invokeCommand.mockResolvedValue({ password: '' });

    await getSipServiceConfiguration();

    expect(invokeCommand).toHaveBeenCalledWith('get_sip_service_configuration');
  });

  it('应将完整配置传给保存命令', async () => {
    const configuration = {
      uri: 'sip:10.0.0.8:5060',
      transport: 'UDP',
      platformId: '34020000002000000001',
      domain: '3402000000',
      password: 'test-only-password',
      registerExpires: 3_600,
      keepaliveInterval: 60,
    } as const;
    invokeCommand.mockResolvedValue(configuration);

    await saveSipServiceConfiguration(configuration);

    expect(invokeCommand).toHaveBeenCalledWith('save_sip_service_configuration', {
      configuration,
    });
  });
});
