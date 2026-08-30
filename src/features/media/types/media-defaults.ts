import { MediaSourceType, type GlobalMediaConfig } from './media-config';

/** 可直接加载的全局媒体默认配置。 */
export function createDefaultMediaConfig(): GlobalMediaConfig {
  return {
    source: {
      type: MediaSourceType.Mp4,
      mp4: {
        filePath: '/mock/media/h265-aac-demo.mp4',
        isLooping: true,
      },
    },
    recording: {
      isEnabled: false,
      directory: '/mock/records',
      segmentDurationMinutes: 10,
    },
    preferences: {
      shouldProbeAfterSelection: true,
    },
  };
}
