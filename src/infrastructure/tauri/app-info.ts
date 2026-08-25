import { invokeCommand } from './invoke-command';

/** 桌面后端返回的应用与核心状态。 */
export interface AppInfo {
  appName: string;
  appVersion: string;
  coreVersion: string;
  configurationReady: boolean;
}

/** 获取桌面后端和 Rust 核心的基础状态。 */
export function getAppInfo(): Promise<AppInfo> {
  return invokeCommand<AppInfo>('get_app_info');
}
