import type { MediaService } from './media-service';
import { MockMediaService } from './mock-media-service';

let mediaService: MediaService = new MockMediaService();

/** 获取当前媒体服务；第二阶段在应用装配时替换为 Tauri 实现。 */
export function getMediaService(): MediaService {
  return mediaService;
}

/** 替换应用级媒体服务，主要用于后续 Tauri 装配和受控测试。 */
export function configureMediaService(service: MediaService): void {
  mediaService = service;
}
