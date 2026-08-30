import { MediaSourceType } from '../types/media-config';
import { createDefaultMediaConfig } from '../types/media-defaults';
import {
  createEmptyMediaRuntimeMetrics,
  MediaSourceStatus,
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

/** 浏览器和测试环境使用的媒体后端；只模拟契约和状态。 */
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
      sourceStatus: MediaSourceStatus.Stopped,
      errorMessage: null,
    };
    return clone(this.runtimeStatus);
  }

  async pausePreview(): Promise<MediaRuntimeStatus> {
    this.runtimeStatus.sourceStatus = MediaSourceStatus.Paused;
    return clone(this.runtimeStatus);
  }

  async resumePreview(): Promise<MediaRuntimeStatus> {
    this.runtimeStatus.sourceStatus = MediaSourceStatus.Previewing;
    return clone(this.runtimeStatus);
  }

  async seek(positionSeconds: number): Promise<MediaRuntimeStatus> {
    this.runtimeStatus.positionSeconds = positionSeconds;
    return clone(this.runtimeStatus);
  }

  async setPlaybackRate(rate: number): Promise<MediaRuntimeStatus> {
    this.runtimeStatus.playbackRate = rate;
    return clone(this.runtimeStatus);
  }

  async setAudioControl(muted: boolean, volume: number): Promise<MediaRuntimeStatus> {
    this.runtimeStatus.muted = muted;
    this.runtimeStatus.volume = volume;
    return clone(this.runtimeStatus);
  }

  async setAudioMonitoring(enabled: boolean): Promise<MediaRuntimeStatus> {
    this.runtimeStatus.audioMonitoring = enabled;
    return clone(this.runtimeStatus);
  }

  async stepFrame(): Promise<MediaVideoFrame | null> {
    const previous = this.runtimeStatus.sourceStatus;
    this.runtimeStatus.sourceStatus = MediaSourceStatus.Previewing;
    const frame = await this.readFrame();
    this.runtimeStatus.sourceStatus = previous;
    return frame;
  }

  async getRuntimeStatus(): Promise<MediaRuntimeStatus> {
    this.failIfRequested('getRuntimeStatus');
    return clone(this.runtimeStatus);
  }

  async readFrame(): Promise<MediaVideoFrame | null> {
    if (this.runtimeStatus.sourceStatus !== MediaSourceStatus.Previewing) return null;
    const width = 320;
    const height = 180;
    const rgba = new Uint8Array(width * height * 4);
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
        activeLiveConsumers: 0,
        activeRecorderConsumers: 0,
        durationSeconds: probeResult.video.durationSeconds,
        positionSeconds: 0,
        playbackRate: 1,
        decodedFrames: 0,
        metrics: createEmptyMediaRuntimeMetrics(),
        muted: false,
        volume: 1,
        audioMonitoring: false,
        errorMessage: null,
        pipelineErrorMessage: null,
        audioSink: null,
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
      activeLiveConsumers: 0,
      activeRecorderConsumers: 0,
      durationSeconds: null,
      positionSeconds: 0,
      playbackRate: 1,
      decodedFrames: 0,
      metrics: createEmptyMediaRuntimeMetrics(),
      muted: false,
      volume: 1,
      audioMonitoring: false,
      errorMessage: null,
      pipelineErrorMessage: null,
      audioSink: null,
    };
  }
}
