import type { GlobalMediaConfig } from '../types/media-config';
import type { MediaProbeResult, MediaRuntimeStatus } from '../types/media-runtime';

export interface MediaVideoFrame {
  width: number;
  height: number;
  rgba: Uint8Array;
  positionSeconds: number;
}

/** UI 与媒体后端之间的稳定应用层契约。 */
export interface MediaService {
  loadConfig(): Promise<GlobalMediaConfig>;
  saveConfig(config: GlobalMediaConfig): Promise<GlobalMediaConfig>;
  applyConfig(config: GlobalMediaConfig): Promise<MediaRuntimeStatus>;
  selectMp4(currentPath: string): Promise<string | null>;
  probeMp4(filePath: string): Promise<MediaProbeResult>;
  startPreview(config: GlobalMediaConfig): Promise<MediaRuntimeStatus>;
  stopPreview(): Promise<MediaRuntimeStatus>;
  pausePreview(): Promise<MediaRuntimeStatus>;
  resumePreview(): Promise<MediaRuntimeStatus>;
  seek(positionSeconds: number): Promise<MediaRuntimeStatus>;
  setPlaybackRate(rate: number): Promise<MediaRuntimeStatus>;
  setAudioControl(muted: boolean, volume: number): Promise<MediaRuntimeStatus>;
  stepFrame(): Promise<MediaVideoFrame | null>;
  getRuntimeStatus(): Promise<MediaRuntimeStatus>;
  readFrame(): Promise<MediaVideoFrame | null>;
}

export class MediaServiceError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'MediaServiceError';
  }
}
