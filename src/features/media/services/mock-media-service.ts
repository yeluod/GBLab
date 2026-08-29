import { MediaSourceType } from '../types/media-config';
import { createDefaultMediaConfig } from '../types/media-defaults';
import {
  MediaSourceStatus,
  RecordingStatus,
  type MediaRuntimeStatus,
} from '../types/media-runtime';
import { MediaServiceError, type MediaService, type MediaVideoFrame } from './media-service';
import {
  MOCK_AUDIO_DEVICES,
  MOCK_MP4_PATHS,
  MOCK_PROBE_RESULTS,
  MOCK_VIDEO_CAPABILITIES,
  MOCK_VIDEO_ENCODER_CAPABILITIES,
  MOCK_VIDEO_DEVICES,
  createInitialRuntimeStatus,
} from './mock-media-fixtures';

import type {
  CaptureDeviceCapabilities,
  CaptureDeviceInfo,
  GlobalMediaConfig,
  VideoEncoderCapabilities,
} from '../types/media-config';
import type { MediaProbeResult } from '../types/media-runtime';

export type MockMediaOperation =
  | 'loadConfig'
  | 'saveConfig'
  | 'applyConfig'
  | 'probeMp4'
  | 'listVideoDevices'
  | 'listAudioDevices'
  | 'getVideoCapabilities'
  | 'getVideoEncoderCapabilities'
  | 'startPreview'
  | 'stopPreview'
  | 'getRuntimeStatus';

export interface MockMediaServiceOptions {
  failures?: MockMediaOperation[];
  initialConfig?: GlobalMediaConfig;
}

function clone<T>(value: T): T {
  return structuredClone(value);
}

function fileName(filePath: string): string {
  return filePath.split(/[\\/]/).at(-1) ?? filePath;
}

/** 第一阶段媒体后端；只模拟契约和状态，不访问真实文件或采集设备。 */
export class MockMediaService implements MediaService {
  private readonly failures: Set<MockMediaOperation>;
  private savedConfig: GlobalMediaConfig;
  private runtimeStatus = createInitialRuntimeStatus();

  constructor(options: MockMediaServiceOptions = {}) {
    this.failures = new Set(options.failures ?? []);
    this.savedConfig = clone(options.initialConfig ?? createDefaultMediaConfig());
  }

  async loadConfig(): Promise<GlobalMediaConfig> {
    this.failIfRequested('loadConfig');
    return clone(this.savedConfig);
  }

  async saveConfig(config: GlobalMediaConfig): Promise<GlobalMediaConfig> {
    this.failIfRequested('saveConfig');
    this.savedConfig = clone(config);
    return clone(this.savedConfig);
  }

  async applyConfig(config: GlobalMediaConfig): Promise<MediaRuntimeStatus> {
    this.failIfRequested('applyConfig');
    this.runtimeStatus = this.createRuntimeStatus(config, MediaSourceStatus.Ready);
    return clone(this.runtimeStatus);
  }

  async selectMp4(currentPath: string): Promise<string | null> {
    const paths = Object.values(MOCK_MP4_PATHS).filter(
      (path) => path !== MOCK_MP4_PATHS.probeError,
    );
    const currentIndex = paths.indexOf(currentPath as (typeof paths)[number]);
    return paths[(currentIndex + 1 + paths.length) % paths.length] ?? paths[0] ?? null;
  }

  async selectRecordingDirectory(currentDirectory: string): Promise<string | null> {
    return currentDirectory === '/mock/records' ? '/mock/records/archive' : '/mock/records';
  }

  async probeMp4(filePath: string): Promise<MediaProbeResult> {
    this.failIfRequested('probeMp4');
    const result = MOCK_PROBE_RESULTS[filePath];
    if (result === undefined || filePath === MOCK_MP4_PATHS.probeError) {
      throw new MediaServiceError('无法解析所选 MP4 文件。');
    }
    return clone(result);
  }

  async listVideoDevices(): Promise<CaptureDeviceInfo[]> {
    this.failIfRequested('listVideoDevices');
    return clone(MOCK_VIDEO_DEVICES);
  }

  async listAudioDevices(): Promise<CaptureDeviceInfo[]> {
    this.failIfRequested('listAudioDevices');
    return clone(MOCK_AUDIO_DEVICES);
  }

  async getVideoCapabilities(deviceId: string): Promise<CaptureDeviceCapabilities> {
    this.failIfRequested('getVideoCapabilities');
    const capabilities = MOCK_VIDEO_CAPABILITIES[deviceId];
    if (capabilities === undefined) {
      throw new MediaServiceError('摄像头不可用或不支持能力检测。');
    }
    return clone(capabilities);
  }

  async getVideoEncoderCapabilities(): Promise<VideoEncoderCapabilities> {
    this.failIfRequested('getVideoEncoderCapabilities');
    return clone(MOCK_VIDEO_ENCODER_CAPABILITIES);
  }

  async startPreview(config: GlobalMediaConfig): Promise<MediaRuntimeStatus> {
    this.failIfRequested('startPreview');
    this.runtimeStatus = this.createRuntimeStatus(config, MediaSourceStatus.Previewing);
    return clone(this.runtimeStatus);
  }

  async stopPreview(): Promise<MediaRuntimeStatus> {
    this.failIfRequested('stopPreview');
    this.runtimeStatus = {
      ...this.runtimeStatus,
      sourceStatus: MediaSourceStatus.Ready,
      errorMessage: null,
    };
    return clone(this.runtimeStatus);
  }

  async getRuntimeStatus(): Promise<MediaRuntimeStatus> {
    this.failIfRequested('getRuntimeStatus');
    return clone(this.runtimeStatus);
  }

  async readFrame(): Promise<MediaVideoFrame | null> {
    if (this.runtimeStatus.sourceStatus !== MediaSourceStatus.Previewing) return null;
    const width = 320;
    const height = 180;
    const rgba = new Array<number>(width * height * 4).fill(0);
    for (let i = 0; i < rgba.length; i += 4) {
      rgba[i] = 20;
      rgba[i + 1] = 120;
      rgba[i + 2] = 170;
      rgba[i + 3] = 255;
    }
    return { width, height, rgba, positionSeconds: 0 };
  }

  private failIfRequested(operation: MockMediaOperation): void {
    if (this.failures.has(operation)) {
      throw new MediaServiceError(`Mock MediaService 执行 ${operation} 失败。`);
    }
  }

  private createRuntimeStatus(
    config: GlobalMediaConfig,
    sourceStatus: MediaSourceStatus,
  ): MediaRuntimeStatus {
    const recording = config.recording.isEnabled
      ? {
          status: RecordingStatus.Ready,
          currentFile: 'GBLab-preview-001.mp4',
          recordedDurationSeconds: 128,
          usedSpaceBytes: 67_108_864,
        }
      : {
          status: RecordingStatus.Disabled,
          currentFile: null,
          recordedDurationSeconds: 0,
          usedSpaceBytes: 0,
        };

    if (config.source.type === MediaSourceType.Mp4) {
      const probeResult = MOCK_PROBE_RESULTS[config.source.mp4.filePath];
      if (probeResult === undefined) {
        throw new MediaServiceError('当前 MP4 尚未完成媒体检测。');
      }
      return {
        sourceStatus,
        sourceLabel: `MP4 · ${fileName(config.source.mp4.filePath)}`,
        video: clone(probeResult.video),
        audio: clone(probeResult.audio),
        activeLiveSessions: 0,
        activePlaybackSessions: 0,
        recording,
        errorMessage: null,
      };
    }

    const { video, audio } = config.source.camera;
    return {
      sourceStatus,
      sourceLabel:
        MOCK_VIDEO_DEVICES.find((device) => device.id === video.deviceId)?.name ?? 'Camera',
      video: {
        codec: video.codec,
        width: video.width,
        height: video.height,
        framesPerSecond: video.framesPerSecond,
        bitrateKbps: video.bitrateKbps,
        durationSeconds: null,
      },
      audio: audio.isEnabled
        ? {
            codec: audio.codec,
            sampleRate: audio.sampleRate,
            channels: audio.channels,
            bitrateKbps: audio.bitrateKbps,
          }
        : null,
      activeLiveSessions: 0,
      activePlaybackSessions: 0,
      recording,
      errorMessage: null,
    };
  }
}
