import { invokeCommand } from '@/infrastructure/tauri';
import { open, type OpenDialogOptions } from '@tauri-apps/plugin-dialog';

import { MediaSourceType } from '../types/media-config';
import {
  MediaSourceStatus,
  type DetectedAudioCodec,
  type AudioSinkInfo,
  type MediaProbeResult,
  type MediaRuntimeStatus,
  normalizeDetectedAudioCodec,
} from '../types/media-runtime';
import type { GlobalMediaConfig } from '../types/media-config';
import { MediaServiceError, type MediaService, type MediaVideoFrame } from './media-service';

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
  sourceKind: 'mp4' | null;
  video: BackendStreamInfo | null;
  audio: BackendStreamInfo | null;
  durationSeconds: number | null;
  positionSeconds: number;
  playbackRate: number;
  decodedFrames: number;
  metrics: MediaRuntimeStatus['metrics'];
  muted: boolean;
  volume: number;
  activeLiveConsumers: number;
  lastError: string | null;
  lastPipelineError: string | null;
  audioSink: AudioSinkInfo | null;
}

type BackendMediaConfig = {
  source: {
    type: 'mp4';
    mp4: { filePath: string; isLooping: boolean };
  };
  preferences: { shouldProbeAfterSelection: boolean };
};

function fromBackendConfig(value: BackendMediaConfig): GlobalMediaConfig {
  return {
    source: {
      type: MediaSourceType.Mp4,
      mp4: { ...value.source.mp4 },
    },
    preferences: { ...value.preferences },
  };
}

function toBackendConfig(value: GlobalMediaConfig): BackendMediaConfig {
  return {
    source: {
      type: 'mp4',
      mp4: { ...value.source.mp4 },
    },
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
  const codec: DetectedAudioCodec = normalizeDetectedAudioCodec(value.codec);
  return {
    codec,
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
  const sourceStatus = value.lastError
    ? MediaSourceStatus.Error
    : value.sourceStatus === 'playing'
      ? MediaSourceStatus.Previewing
      : value.sourceStatus === 'paused'
        ? MediaSourceStatus.Paused
        : value.sourceStatus === 'unconfigured'
          ? MediaSourceStatus.Unconfigured
          : value.sourceStatus === 'stopped'
            ? MediaSourceStatus.Stopped
            : MediaSourceStatus.Ready;
  return {
    sourceStatus,
    sourceLabel: value.sourceKind === MediaSourceType.Mp4 ? 'MP4 文件' : '未配置',
    video: value.video ? frontendVideo(value.video) : null,
    audio: value.audio ? frontendAudio(value.audio) : null,
    // Live/Playback managers are not implemented yet; never expose source
    // type as a fabricated business-session count.
    activeLiveConsumers: value.activeLiveConsumers,
    durationSeconds: value.durationSeconds,
    positionSeconds: value.positionSeconds,
    playbackRate: value.playbackRate,
    decodedFrames: value.decodedFrames,
    metrics: value.metrics,
    muted: value.muted,
    volume: value.volume,
    errorMessage: value.lastError,
    pipelineErrorMessage: value.lastPipelineError,
    audioSink: value.audioSink,
  };
}

/** Tauri 媒体适配器；MP4 的探测和播放在 Rust/rsmpeg 内完成。 */
export class TauriMediaService implements MediaService {
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
    return this.open(config);
  }
  async selectMp4(currentPath: string): Promise<string | null> {
    const options: OpenDialogOptions = {
      multiple: false,
      directory: false,
      filters: [{ name: 'MP4 视频', extensions: ['mp4'] }],
    };
    const trimmedPath = currentPath.trim();
    if (trimmedPath.length > 0) options.defaultPath = trimmedPath;
    const selected = await open(options);
    return typeof selected === 'string' ? selected : null;
  }

  async probeMp4(filePath: string): Promise<MediaProbeResult> {
    return toProbe(await invokeCommand<BackendProbeResult>('probe_mp4', { filePath }));
  }
  async startPreview(config: GlobalMediaConfig): Promise<MediaRuntimeStatus> {
    const current = await invokeCommand<BackendRuntimeStatus>('get_media_runtime_status');
    if (current.activeLiveConsumers > 0) {
      return toRuntime(await invokeCommand<BackendRuntimeStatus>('attach_media_preview'));
    }
    await this.open(config);
    await invokeCommand<BackendRuntimeStatus>('attach_media_preview');
    return toRuntime(await invokeCommand<BackendRuntimeStatus>('play_media'));
  }
  async stopPreview(): Promise<MediaRuntimeStatus> {
    return toRuntime(await invokeCommand<BackendRuntimeStatus>('detach_media_preview'));
  }
  async pausePreview(): Promise<MediaRuntimeStatus> {
    return toRuntime(await invokeCommand<BackendRuntimeStatus>('pause_media'));
  }
  async resumePreview(): Promise<MediaRuntimeStatus> {
    return toRuntime(await invokeCommand<BackendRuntimeStatus>('play_media'));
  }
  async seek(positionSeconds: number): Promise<MediaRuntimeStatus> {
    return toRuntime(await invokeCommand<BackendRuntimeStatus>('seek_media', { positionSeconds }));
  }
  async setPlaybackRate(rate: number): Promise<MediaRuntimeStatus> {
    return toRuntime(
      await invokeCommand<BackendRuntimeStatus>('set_media_playback_rate', { rate }),
    );
  }
  async setAudioControl(muted: boolean, volume: number): Promise<MediaRuntimeStatus> {
    return toRuntime(
      await invokeCommand<BackendRuntimeStatus>('set_media_audio_control', { muted, volume }),
    );
  }
  async stepFrame(): Promise<MediaVideoFrame | null> {
    return decodePreviewFrame(await invokeCommand<Uint8Array>('step_media_frame'));
  }
  async getRuntimeStatus(): Promise<MediaRuntimeStatus> {
    return toRuntime(await invokeCommand<BackendRuntimeStatus>('get_media_runtime_status'));
  }

  async readFrame(): Promise<MediaVideoFrame | null> {
    return decodePreviewFrame(await invokeCommand<Uint8Array>('read_media_frame'));
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

function decodePreviewFrame(payload: Uint8Array): MediaVideoFrame | null {
  const bytes = payload instanceof Uint8Array ? payload : new Uint8Array(payload);
  const headerSize = 21;
  if (bytes.byteLength === 0) return null;
  if (
    bytes.byteLength < headerSize ||
    bytes[0] !== 0x47 ||
    bytes[1] !== 0x42 ||
    bytes[2] !== 0x50 ||
    bytes[3] !== 0x46 ||
    bytes[4] !== 1
  ) {
    throw new MediaServiceError('预览帧二进制格式无效。');
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const width = view.getUint32(5, true);
  const height = view.getUint32(9, true);
  const positionSeconds = view.getFloat64(13, true);
  const expectedLength = headerSize + width * height * 4;
  if (bytes.byteLength !== expectedLength) {
    throw new MediaServiceError('预览帧像素数据长度无效。');
  }
  return {
    width,
    height,
    positionSeconds,
    rgba: bytes.slice(headerSize),
  };
}
