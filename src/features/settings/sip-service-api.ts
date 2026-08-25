import { invokeCommand } from '@/infrastructure/tauri';

import type { SipServiceConfig } from './types';

/** 从 Rust 核心读取唯一 SIP 服务配置。 */
export function getSipServiceConfiguration(): Promise<SipServiceConfig> {
  return invokeCommand<SipServiceConfig>('get_sip_service_configuration');
}

/** 校验并将唯一 SIP 服务配置写入应用 JSON 文件。 */
export function saveSipServiceConfiguration(
  configuration: SipServiceConfig,
): Promise<SipServiceConfig> {
  return invokeCommand<SipServiceConfig>('save_sip_service_configuration', { configuration });
}
