import { invokeCommand } from '@/infrastructure/tauri';
import { open, type OpenDialogOptions } from '@tauri-apps/plugin-dialog';

import { CaptureDeviceStatus, MediaSourceType } from '../types/media-config';
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
  VideoEncoderCapabilities,
} from '../types/media-config';
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

interface BackendCaptureDevice {
  id: string;
  name: string;
  status: 'available' | 'unavailable' | 'permission-denied' | 'busy';
}

interface BackendCaptureDeviceLists {
  video: BackendCaptureDevice[];
  audio: BackendCaptureDevice[];
}

interface BackendRuntimeStatus {
  sourceStatus: 'unconfigured' | 'ready' | 'playing' | 'paused' | 'stopped';
  sourceKind: MediaSourceType | null;
  video: BackendStreamInfo | null;
  audio: BackendStreamInfo | null;
  durationSeconds: number | null;
  positionSeconds: number;
  playbackRate: number;
  decodedFrames: number;
  muted: boolean;
  volume: number;
  lastError: string | null;
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
  const sourceStatus = value.lastError
    ? MediaSourceStatus.Error
    : value.sourceStatus === 'playing'
      ? MediaSourceStatus.Previewing
      : value.sourceStatus === 'paused'
        ? MediaSourceStatus.Paused
        : value.sourceStatus === 'unconfigured'
          ? MediaSourceStatus.Unconfigured
          : value.sourceStatus === 'stopped'
            ? MediaSourceStatus.Ready
            : MediaSourceStatus.Ready;
  return {
    sourceStatus,
    sourceLabel:
      value.sourceKind === MediaSourceType.Mp4
        ? 'MP4 文件'
        : value.sourceKind === MediaSourceType.Camera
          ? '摄像头'
          : '未配置',
    video: value.video ? frontendVideo(value.video) : null,
    audio: value.audio ? frontendAudio(value.audio) : null,
    activeLiveSessions:
      value.sourceKind === MediaSourceType.Camera && value.sourceStatus === 'playing' ? 1 : 0,
    activePlaybackSessions:
      value.sourceKind === MediaSourceType.Mp4 && value.sourceStatus === 'playing' ? 1 : 0,
    durationSeconds: value.durationSeconds,
    positionSeconds: value.positionSeconds,
    playbackRate: value.playbackRate,
    decodedFrames: value.decodedFrames,
    muted: value.muted,
    volume: value.volume,
    recording: {
      status: RecordingStatus.Disabled,
      currentFile: null,
      recordedDurationSeconds: 0,
      usedSpaceBytes: 0,
    },
    errorMessage: value.lastError,
  };
}

/** Tauri 媒体适配器；MP4 的探测和播放在 Rust/rsmpeg 内完成。 */
export class TauriMediaService implements MediaService {
  private captureDevicesRequest: Promise<BackendCaptureDeviceLists> | null = null;

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

  async selectRecordingDirectory(currentDirectory: string): Promise<string | null> {
    const options: OpenDialogOptions = {
      multiple: false,
      directory: true,
    };
    const trimmedDirectory = currentDirectory.trim();
    if (trimmedDirectory.length > 0) options.defaultPath = trimmedDirectory;
    const selected = await open(options);
    return typeof selected === 'string' ? selected : null;
  }
  async probeMp4(filePath: string): Promise<MediaProbeResult> {
    return toProbe(await invokeCommand<BackendProbeResult>('probe_mp4', { filePath }));
  }
  async listVideoDevices(): Promise<CaptureDeviceInfo[]> {
    const devices = await this.listCaptureDevices();
    return devices.video.map(toCaptureDevice);
  }
  async listAudioDevices(): Promise<CaptureDeviceInfo[]> {
    const devices = await this.listCaptureDevices();
    return devices.audio.map(toCaptureDevice);
  }
  async getVideoCapabilities(deviceId: string): Promise<CaptureDeviceCapabilities> {
    return invokeCommand<CaptureDeviceCapabilities>('get_video_capture_capabilities', {
      deviceId,
    });
  }
  getVideoEncoderCapabilities(): Promise<VideoEncoderCapabilities> {
    return invokeCommand<VideoEncoderCapabilities>('get_video_encoder_capabilities');
  }
  async startPreview(config: GlobalMediaConfig): Promise<MediaRuntimeStatus> {
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
    if (config.source.type === MediaSourceType.Camera) {
      return toRuntime(
        await invokeCommand<BackendRuntimeStatus>('open_camera', {
          configuration: {
            videoDeviceId: config.source.camera.video.deviceId,
            videoCodec: config.source.camera.video.codec,
            videoBitrate: config.source.camera.video.bitrateKbps * 1000,
            encoderBackend: config.source.camera.video.encoderBackend,
            audioEnabled: config.source.camera.audio.isEnabled,
            audioDeviceId: config.source.camera.audio.deviceId,
            audioCodec: config.source.camera.audio.codec,
            audioSampleRate: config.source.camera.audio.sampleRate,
            audioChannels: config.source.camera.audio.channels,
            audioBitrate: config.source.camera.audio.bitrateKbps * 1000,
            width: config.source.camera.video.width,
            height: config.source.camera.video.height,
            framesPerSecond: config.source.camera.video.framesPerSecond,
          },
        }),
      );
    }
    return toRuntime(
      await invokeCommand<BackendRuntimeStatus>('open_mp4', {
        filePath: config.source.mp4.filePath,
        looping: config.source.mp4.isLooping,
      }),
    );
  }

  private async listCaptureDevices(): Promise<BackendCaptureDeviceLists> {
    this.captureDevicesRequest ??= invokeCommand<BackendCaptureDeviceLists>(
      'list_capture_devices',
    ).finally(() => {
      this.captureDevicesRequest = null;
    });
    return this.captureDevicesRequest;
  }
}
function toCaptureDevice(device: BackendCaptureDevice): CaptureDeviceInfo {
  return {
    id: device.id,
    name: device.name,
    status: device.status as CaptureDeviceStatus,
  };
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
