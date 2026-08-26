/** 全部模拟设备共享的唯一 SIP 服务配置。 */
export interface SipServiceConfig {
  uri: string;
  /** 当前版本实际实现的信令传输，仅支持 UDP。 */
  transport: 'UDP';
  platformId: string;
  domain: string;
  password: string;
  localBindAddress: string;
  advertisedAddress: string;
  localPort: number;
  registerExpires: number;
  keepaliveInterval: number;
}

/** Tauri Command 返回的结构化错误。 */
export interface ConfigurationCommandError {
  code: string;
  message: string;
}
