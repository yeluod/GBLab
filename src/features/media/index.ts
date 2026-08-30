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
export { MOCK_MP4_PATHS, MOCK_PROBE_RESULTS } from './services/mock-media-fixtures';
export {
  useMediaStore,
  validateMediaConfig,
  type MediaFieldErrors,
  type MediaOperationResult,
} from './stores/media-store';
export { createDefaultMediaConfig } from './types/media-defaults';
export { AudioCodec, MediaSourceType, VideoCodec } from './types/media-config';
export type {
  GlobalMediaConfig,
  MediaPreferences,
  MediaSourceConfig,
  Mp4SourceConfig,
  RecordingConfig,
} from './types/media-config';
export { MediaSourceStatus, normalizeDetectedAudioCodec } from './types/media-runtime';
export type {
  AudioSinkInfo,
  AudioSinkStatus,
  AudioStreamInfo,
  DetectedAudioCodec,
  MediaProbeResult,
  MediaRuntimeStatus,
  VideoStreamInfo,
} from './types/media-runtime';
