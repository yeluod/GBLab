import {
  AudioCodec,
  EncoderBackend,
  MediaSourceType,
  VideoCodec,
  type GlobalMediaConfig,
} from './media-config';

/** 可直接加载的全局媒体默认配置。 */
export function createDefaultMediaConfig(): GlobalMediaConfig {
  return {
    source: {
      type: MediaSourceType.Mp4,
      mp4: {
        filePath: '/mock/media/h265-aac-demo.mp4',
        isLooping: true,
      },
      camera: {
        video: {
          deviceId: 'camera-integrated',
          width: 1920,
          height: 1080,
          framesPerSecond: 25,
          codec: VideoCodec.H265,
          bitrateKbps: 4_096,
          encoderBackend: EncoderBackend.Auto,
        },
        audio: {
          isEnabled: true,
          deviceId: 'microphone-built-in',
          codec: AudioCodec.Aac,
          sampleRate: 48_000,
          channels: 2,
          bitrateKbps: 128,
        },
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
