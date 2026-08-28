import { invokeCommand } from '@/infrastructure/tauri';

import { MediaSourceType } from '../types/media-config';
import { createDefaultMediaConfig } from '../types/media-defaults';
import {
  MediaSourceStatus,
  RecordingStatus,
  type MediaProbeResult,
  type MediaRuntimeStatus,
} from '../types/media-runtime';
import type {
  CaptureDeviceCapabilities,
  CaptureDeviceInfo,
  GlobalMediaConfig,
} from '../types/media-config';
import { MediaServiceError, type MediaService } from './media-service';
import { MockMediaService } from './mock-media-service';

interface BackendStreamInfo {
  codec: string;
  width?: number;
  height?: number;
  framesPerSecond?: number;
  sampleRate?: number;
  channels?: number;
  bitrate: number | null;
  durationSeconds?: number | null;
}

interface BackendProbeResult {
  filePath: string;
  video: BackendStreamInfo;
  audio: BackendStreamInfo | null;
  durationSeconds: number | null;
  bitrate: number | null;
}

interface BackendRuntimeStatus {
  sourceStatus: 'unconfigured' | 'ready' | 'playing' | 'paused' | 'stopped';
  sourceKind: MediaSourceType | null;
  video: BackendStreamInfo | null;
  audio: BackendStreamInfo | null;
  durationSeconds: number | null;
  positionSeconds: number;
}

type BackendMediaConfig = {
  source: {
    type: 'mp4' | 'camera';
    mp4: { filePath: string; isLooping: boolean };
    camera: {
      video: {
        deviceId: string;
        width: number;
        height: number;
        framesPerSecond: number;
        codec: string;
        bitrateKbps: number;
        encoderBackend: string;
      };
      audio: {
        isEnabled: boolean;
        deviceId: string;
        codec: string;
        sampleRate: number;
        channels: number;
        bitrateKbps: number;
      };
    };
  };
  recording: {
    isEnabled: boolean;
    directory: string;
    segmentDurationMinutes: 5 | 10 | 30 | 60;
  };
  preferences: { shouldProbeAfterSelection: boolean };
};

const encoderToBackend: Record<string, string> = {
  auto: 'auto',
  videotoolbox: 'videotoolbox',
  'media-foundation': 'media-foundation',
  nvenc: 'nvenc',
  qsv: 'qsv',
  amf: 'amf',
};

function fromBackendConfig(value: BackendMediaConfig): GlobalMediaConfig {
  return {
    source: {
      type: value.source.type as MediaSourceType,
      mp4: { ...value.source.mp4 },
      camera: {
        video: {
          ...value.source.camera.video,
          codec: value.source.camera.video
            .codec as GlobalMediaConfig['source']['camera']['video']['codec'],
          encoderBackend: value.source.camera.video
            .encoderBackend as GlobalMediaConfig['source']['camera']['video']['encoderBackend'],
        },
        audio: {
          ...value.source.camera.audio,
          codec: value.source.camera.audio
            .codec as GlobalMediaConfig['source']['camera']['audio']['codec'],
        },
      },
    },
    recording: { ...value.recording },
    preferences: { ...value.preferences },
  };
}

function toBackendConfig(value: GlobalMediaConfig): BackendMediaConfig {
  return {
    source: {
      type: value.source.type,
      mp4: { ...value.source.mp4 },
      camera: {
        video: {
          ...value.source.camera.video,
          encoderBackend: encoderToBackend[value.source.camera.video.encoderBackend] ?? 'auto',
        },
        audio: { ...value.source.camera.audio },
      },
    },
    recording: { ...value.recording },
    preferences: { ...value.preferences },
  };
}

function bitrateKbps(value: number | null | undefined): number {
  return Math.round((value ?? 0) / 1000);
}

function frontendVideo(value: BackendStreamInfo): MediaProbeResult['video'] {
  return {
    codec: value.codec as MediaProbeResult['video']['codec'],
    width: value.width ?? 0,
    height: value.height ?? 0,
    framesPerSecond: value.framesPerSecond ?? 0,
    bitrateKbps: bitrateKbps(value.bitrate),
    durationSeconds: value.durationSeconds ?? null,
  };
}

function frontendAudio(value: BackendStreamInfo): NonNullable<MediaProbeResult['audio']> {
  return {
    codec: value.codec as unknown as NonNullable<MediaProbeResult['audio']>['codec'],
    sampleRate: value.sampleRate ?? 0,
    channels: value.channels ?? 0,
    bitrateKbps: bitrateKbps(value.bitrate),
  };
}

function toProbe(value: BackendProbeResult): MediaProbeResult {
  return {
    filePath: value.filePath,
    video: frontendVideo(value.video),
    audio: value.audio ? frontendAudio(value.audio) : null,
  };
}

function toRuntime(value: BackendRuntimeStatus): MediaRuntimeStatus {
  const sourceStatus =
    value.sourceStatus === 'playing'
      ? MediaSourceStatus.Previewing
      : value.sourceStatus === 'paused'
        ? MediaSourceStatus.Ready
        : value.sourceStatus === 'unconfigured'
          ? MediaSourceStatus.Unconfigured
          : value.sourceStatus === 'stopped'
            ? MediaSourceStatus.Ready
            : MediaSourceStatus.Ready;
  return {
    sourceStatus,
    sourceLabel:
      value.sourceKind === MediaSourceType.Mp4 ? 'MP4 文件' : (value.sourceKind ?? '未配置'),
    video: value.video ? frontendVideo(value.video) : null,
    audio: value.audio ? frontendAudio(value.audio) : null,
    activeLiveSessions: 0,
    activePlaybackSessions: value.sourceStatus === 'playing' ? 1 : 0,
    recording: {
      status: RecordingStatus.Disabled,
      currentFile: null,
      recordedDurationSeconds: 0,
      usedSpaceBytes: 0,
    },
    errorMessage: null,
  };
}

/** Tauri 媒体适配器；MP4 的探测和播放在 Rust/rsmpeg 内完成。 */
export class TauriMediaService implements MediaService {
  private readonly fallback = new MockMediaService({ initialConfig: createDefaultMediaConfig() });

  async loadConfig(): Promise<GlobalMediaConfig> {
    return fromBackendConfig(await invokeCommand<BackendMediaConfig>('get_media_configuration'));
  }
  async saveConfig(config: GlobalMediaConfig): Promise<GlobalMediaConfig> {
    return fromBackendConfig(
      await invokeCommand<BackendMediaConfig>('save_media_configuration', {
        configuration: toBackendConfig(config),
      }),
    );
  }
  applyConfig(config: GlobalMediaConfig): Promise<MediaRuntimeStatus> {
    if (config.source.type !== MediaSourceType.Mp4)
      return Promise.reject(new MediaServiceError('摄像头采集将在下一阶段接入。'));
    return this.open(config);
  }
  selectMp4(currentPath: string): Promise<string | null> {
    return this.fallback.selectMp4(currentPath);
  }
  selectRecordingDirectory(currentDirectory: string): Promise<string | null> {
    return this.fallback.selectRecordingDirectory(currentDirectory);
  }
  async probeMp4(filePath: string): Promise<MediaProbeResult> {
    return toProbe(await invokeCommand<BackendProbeResult>('probe_mp4', { filePath }));
  }
  listVideoDevices(): Promise<CaptureDeviceInfo[]> {
    return Promise.resolve([]);
  }
  listAudioDevices(): Promise<CaptureDeviceInfo[]> {
    return Promise.resolve([]);
  }
  getVideoCapabilities(_deviceId: string): Promise<CaptureDeviceCapabilities> {
    return Promise.reject(new MediaServiceError('摄像头采集将在下一阶段接入。'));
  }
  async startPreview(config: GlobalMediaConfig): Promise<MediaRuntimeStatus> {
    await this.open(config);
    return toRuntime(await invokeCommand<BackendRuntimeStatus>('play_media'));
  }
  async stopPreview(): Promise<MediaRuntimeStatus> {
    return toRuntime(await invokeCommand<BackendRuntimeStatus>('stop_media'));
  }
  async getRuntimeStatus(): Promise<MediaRuntimeStatus> {
    return toRuntime(await invokeCommand<BackendRuntimeStatus>('get_media_runtime_status'));
  }

  private async open(config: GlobalMediaConfig): Promise<MediaRuntimeStatus> {
    return toRuntime(
      await invokeCommand<BackendRuntimeStatus>('open_mp4', {
        filePath: config.source.mp4.filePath,
        looping: config.source.mp4.isLooping,
      }),
    );
  }
}
