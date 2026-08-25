/** 全部模拟设备共享的唯一 SIP 服务配置。 */
export interface SipServiceConfig {
  uri: string;
  transport: 'UDP' | 'TCP';
  platformId: string;
  domain: string;
  password: string;
  registerExpires: number;
  keepaliveInterval: number;
}

/** Tauri Command 返回的结构化错误。 */
export interface ConfigurationCommandError {
  code: string;
  message: string;
}
