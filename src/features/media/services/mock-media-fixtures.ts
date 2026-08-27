import {
  AudioCodec,
  CaptureDeviceStatus,
  VideoCodec,
  type CaptureDeviceCapabilities,
  type CaptureDeviceInfo,
} from '../types/media-config';
import {
  MediaSourceStatus,
  RecordingStatus,
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

export const MOCK_VIDEO_DEVICES: CaptureDeviceInfo[] = [
  {
    id: 'camera-integrated',
    name: 'Integrated / FaceTime HD Camera',
    status: CaptureDeviceStatus.Available,
  },
  {
    id: 'camera-usb',
    name: 'External USB Camera',
    status: CaptureDeviceStatus.Available,
  },
  {
    id: 'camera-busy',
    name: 'Virtual Camera (Busy)',
    status: CaptureDeviceStatus.Busy,
  },
];

export const MOCK_AUDIO_DEVICES: CaptureDeviceInfo[] = [
  {
    id: 'microphone-built-in',
    name: 'Built-in Microphone',
    status: CaptureDeviceStatus.Available,
  },
  {
    id: 'microphone-usb',
    name: 'External USB Microphone',
    status: CaptureDeviceStatus.Available,
  },
];

export const MOCK_VIDEO_CAPABILITIES: Record<string, CaptureDeviceCapabilities> = {
  'camera-integrated': {
    deviceId: 'camera-integrated',
    modes: [
      { width: 640, height: 480, supportedFramesPerSecond: [15, 25, 30] },
      { width: 1280, height: 720, supportedFramesPerSecond: [25, 30, 60] },
      { width: 1920, height: 1080, supportedFramesPerSecond: [25, 30] },
    ],
    supportedCodecs: [VideoCodec.H264, VideoCodec.H265],
  },
  'camera-usb': {
    deviceId: 'camera-usb',
    modes: [
      { width: 640, height: 480, supportedFramesPerSecond: [15, 30] },
      { width: 1280, height: 720, supportedFramesPerSecond: [25, 30] },
    ],
    supportedCodecs: [VideoCodec.H264],
  },
};

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
    activeLiveSessions: 0,
    activePlaybackSessions: 0,
    recording: {
      status: RecordingStatus.Disabled,
      currentFile: null,
      recordedDurationSeconds: 0,
      usedSpaceBytes: 0,
    },
    errorMessage: null,
  };
}
