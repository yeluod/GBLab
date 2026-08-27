import type {
  CaptureDeviceCapabilities,
  CaptureDeviceInfo,
  GlobalMediaConfig,
} from '../types/media-config';
import type { MediaProbeResult, MediaRuntimeStatus } from '../types/media-runtime';

/** UI 与媒体后端之间的稳定应用层契约。 */
export interface MediaService {
  loadConfig(): Promise<GlobalMediaConfig>;
  saveConfig(config: GlobalMediaConfig): Promise<GlobalMediaConfig>;
  applyConfig(config: GlobalMediaConfig): Promise<MediaRuntimeStatus>;
  selectMp4(currentPath: string): Promise<string | null>;
  selectRecordingDirectory(currentDirectory: string): Promise<string | null>;
  probeMp4(filePath: string): Promise<MediaProbeResult>;
  listVideoDevices(): Promise<CaptureDeviceInfo[]>;
  listAudioDevices(): Promise<CaptureDeviceInfo[]>;
  getVideoCapabilities(deviceId: string): Promise<CaptureDeviceCapabilities>;
  startPreview(config: GlobalMediaConfig): Promise<MediaRuntimeStatus>;
  stopPreview(): Promise<MediaRuntimeStatus>;
  getRuntimeStatus(): Promise<MediaRuntimeStatus>;
}

export class MediaServiceError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'MediaServiceError';
  }
}
