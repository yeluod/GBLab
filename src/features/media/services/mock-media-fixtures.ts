import { AudioCodec, VideoCodec } from '../types/media-config';
import {
  createEmptyMediaRuntimeMetrics,
  MediaSourceStatus,
  type MediaProbeResult,
  type MediaRuntimeStatus,
} from '../types/media-runtime';
export { createDefaultMediaConfig } from '../types/media-defaults';

export const MOCK_MP4_PATHS = {
  h264WithAudio: '/mock/media/h264-aac-demo.mp4',
  h265WithAudio: '/mock/media/h265-aac-demo.mp4',
  h264VideoOnly: '/mock/media/h264-video-only.mp4',
  h265VideoOnly: '/mock/media/h265-video-only.mp4',
  probeError: '/mock/media/probe-error.mp4',
} as const;

const commonAudio = {
  codec: AudioCodec.Aac,
  sampleRate: 48_000,
  channels: 2,
  bitrateKbps: 128,
};

export const MOCK_PROBE_RESULTS: Record<string, MediaProbeResult> = {
  [MOCK_MP4_PATHS.h264WithAudio]: {
    filePath: MOCK_MP4_PATHS.h264WithAudio,
    video: {
      codec: VideoCodec.H264,
      width: 1920,
      height: 1080,
      framesPerSecond: 25,
      bitrateKbps: 4_096,
      durationSeconds: 180,
    },
    audio: commonAudio,
  },
  [MOCK_MP4_PATHS.h265WithAudio]: {
    filePath: MOCK_MP4_PATHS.h265WithAudio,
    video: {
      codec: VideoCodec.H265,
      width: 1920,
      height: 1080,
      framesPerSecond: 25,
      bitrateKbps: 2_560,
      durationSeconds: 240,
    },
    audio: commonAudio,
  },
  [MOCK_MP4_PATHS.h264VideoOnly]: {
    filePath: MOCK_MP4_PATHS.h264VideoOnly,
    video: {
      codec: VideoCodec.H264,
      width: 1280,
      height: 720,
      framesPerSecond: 30,
      bitrateKbps: 2_048,
      durationSeconds: 90,
    },
    audio: null,
  },
  [MOCK_MP4_PATHS.h265VideoOnly]: {
    filePath: MOCK_MP4_PATHS.h265VideoOnly,
    video: {
      codec: VideoCodec.H265,
      width: 3840,
      height: 2160,
      framesPerSecond: 30,
      bitrateKbps: 8_192,
      durationSeconds: 120,
    },
    audio: null,
  },
};

export function createInitialRuntimeStatus(): MediaRuntimeStatus {
  const defaultProbeResult = MOCK_PROBE_RESULTS[MOCK_MP4_PATHS.h265WithAudio];
  if (defaultProbeResult === undefined) {
    throw new Error('默认 Mock 媒体信息缺失。');
  }
  return {
    sourceStatus: MediaSourceStatus.Ready,
    sourceLabel: 'MP4 · h265-aac-demo.mp4',
    video: structuredClone(defaultProbeResult.video),
    audio: structuredClone(defaultProbeResult.audio),
    activeLiveConsumers: 0,
    activeRecorderConsumers: 0,
    durationSeconds: defaultProbeResult.video.durationSeconds,
    positionSeconds: 0,
    playbackRate: 1,
    decodedFrames: 0,
    metrics: createEmptyMediaRuntimeMetrics(),
    muted: false,
    volume: 1,
    errorMessage: null,
    pipelineErrorMessage: null,
    audioSink: null,
  };
}
