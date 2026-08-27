/** 全部模拟设备共享的 GB28181 XML 信令字符集。 */
export type SignalCharset = 'GB2312' | 'GBK' | 'UTF-8';

/** 全部模拟设备共享的全局平台与运行配置。 */
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
  signalCharset: SignalCharset;
}

/** Tauri Command 返回的结构化错误。 */
export interface ConfigurationCommandError {
  code: string;
  message: string;
}
