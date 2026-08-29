/** 全局媒体源类型。 */
export enum MediaSourceType {
  Mp4 = 'mp4',
  Camera = 'camera',
}

/** 可供媒体管线使用的视频编码。 */
export enum VideoCodec {
  H264 = 'h264',
  H265 = 'h265',
}

/** 可供采集音频使用的编码。 */
export enum AudioCodec {
  G711A = 'g711a',
  G711U = 'g711u',
  Aac = 'aac',
}

/** 编码后端；界面只展示当前 FFmpeg 实际提供的实现。 */
export enum EncoderBackend {
  Auto = 'auto',
  VideoToolbox = 'videotoolbox',
  MediaFoundation = 'media-foundation',
  Nvenc = 'nvenc',
  Qsv = 'qsv',
  Amf = 'amf',
}

export enum CaptureDeviceStatus {
  Available = 'available',
  Unavailable = 'unavailable',
  PermissionDenied = 'permission-denied',
  Busy = 'busy',
}

export interface Mp4SourceConfig {
  filePath: string;
  isLooping: boolean;
}

export interface VideoCaptureConfig {
  deviceId: string;
  width: number;
  height: number;
  framesPerSecond: number;
  codec: VideoCodec;
  bitrateKbps: number;
  encoderBackend: EncoderBackend;
}

export interface AudioCaptureConfig {
  isEnabled: boolean;
  deviceId: string;
  codec: AudioCodec;
  sampleRate: number;
  channels: number;
  bitrateKbps: number;
}

export interface CameraSourceConfig {
  video: VideoCaptureConfig;
  audio: AudioCaptureConfig;
}

export interface MediaSourceConfig {
  type: MediaSourceType;
  mp4: Mp4SourceConfig;
  camera: CameraSourceConfig;
}

export interface RecordingConfig {
  isEnabled: boolean;
  directory: string;
  segmentDurationMinutes: 5 | 10 | 30 | 60;
}

export interface MediaPreferences {
  shouldProbeAfterSelection: boolean;
}

/** 所有模拟设备和通道共享的唯一媒体配置。 */
export interface GlobalMediaConfig {
  source: MediaSourceConfig;
  recording: RecordingConfig;
  preferences: MediaPreferences;
}

export interface CaptureDeviceInfo {
  id: string;
  name: string;
  status: CaptureDeviceStatus;
}

export interface VideoCaptureMode {
  width: number;
  height: number;
  frameRates: FrameRateCapability[];
}

export type FrameRateCapability =
  { kind: 'exact'; value: number } | { kind: 'range'; minimum: number; maximum: number };

export function isFrameRateSupported(mode: VideoCaptureMode, value: number): boolean {
  return mode.frameRates.some((capability) =>
    capability.kind === 'exact'
      ? Math.abs(capability.value - value) < 0.0001
      : value >= capability.minimum && value <= capability.maximum,
  );
}

export function selectableFrameRates(mode: VideoCaptureMode): number[] {
  const values = mode.frameRates.flatMap((capability) =>
    capability.kind === 'exact' ? [capability.value] : [capability.minimum, capability.maximum],
  );
  return [...new Set(values)].sort((left, right) => left - right);
}

export interface CaptureDeviceCapabilities {
  deviceId: string;
  modes: VideoCaptureMode[];
}

/** 与摄像头采集模式独立的 FFmpeg 视频编码器能力。 */
export interface VideoEncoderCapabilities {
  encoders: VideoEncoderCapability[];
}

export interface VideoEncoderCapability {
  codec: VideoCodec;
  backend: EncoderBackend;
  encoderName: string;
  hardware: boolean;
}
