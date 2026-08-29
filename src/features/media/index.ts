export { configureMediaService } from './services/media-service-provider';
export {
  MediaServiceError,
  type MediaService,
  type MediaVideoFrame,
} from './services/media-service';
export {
  MockMediaService,
  type MockMediaOperation,
  type MockMediaServiceOptions,
} from './services/mock-media-service';
export { TauriMediaService } from './services/tauri-media-service';
export {
  MOCK_AUDIO_DEVICES,
  MOCK_MP4_PATHS,
  MOCK_PROBE_RESULTS,
  MOCK_VIDEO_CAPABILITIES,
  MOCK_VIDEO_ENCODER_CAPABILITIES,
  MOCK_VIDEO_DEVICES,
} from './services/mock-media-fixtures';
export {
  useMediaStore,
  validateMediaConfig,
  type MediaFieldErrors,
  type MediaOperationResult,
} from './stores/media-store';
export { createDefaultMediaConfig } from './types/media-defaults';
export {
  AudioCodec,
  CaptureDeviceStatus,
  EncoderBackend,
  MediaSourceType,
  VideoCodec,
  isFrameRateSupported,
  selectableFrameRates,
} from './types/media-config';
export type {
  AudioCaptureConfig,
  CameraSourceConfig,
  CaptureDeviceCapabilities,
  CaptureDeviceInfo,
  GlobalMediaConfig,
  MediaPreferences,
  MediaSourceConfig,
  Mp4SourceConfig,
  RecordingConfig,
  VideoCaptureConfig,
  VideoCaptureMode,
  VideoEncoderCapabilities,
  VideoEncoderCapability,
} from './types/media-config';
export { MediaSourceStatus } from './types/media-runtime';
export type {
  AudioStreamInfo,
  MediaProbeResult,
  MediaRuntimeStatus,
  VideoStreamInfo,
} from './types/media-runtime';
