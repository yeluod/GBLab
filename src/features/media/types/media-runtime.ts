import type { AudioCodec, VideoCodec } from './media-config';

export enum MediaSourceStatus {
  Unconfigured = 'unconfigured',
  Loading = 'loading',
  Ready = 'ready',
  Previewing = 'previewing',
  Paused = 'paused',
  Error = 'error',
  Unavailable = 'unavailable',
}

export enum RecordingStatus {
  Disabled = 'disabled',
  Ready = 'ready',
  Recording = 'recording',
  Error = 'error',
}

export interface VideoStreamInfo {
  codec: VideoCodec;
  width: number;
  height: number;
  framesPerSecond: number;
  bitrateKbps: number;
  durationSeconds: number | null;
}

export interface AudioStreamInfo {
  codec: AudioCodec;
  sampleRate: number;
  channels: number;
  bitrateKbps: number;
}

export interface MediaProbeResult {
  filePath: string;
  video: VideoStreamInfo;
  audio: AudioStreamInfo | null;
}

export interface RecordingRuntimeInfo {
  status: RecordingStatus;
  currentFile: string | null;
  recordedDurationSeconds: number;
  usedSpaceBytes: number;
}

export interface MediaRuntimeStatus {
  sourceStatus: MediaSourceStatus;
  sourceLabel: string;
  video: VideoStreamInfo | null;
  audio: AudioStreamInfo | null;
  activeLiveSessions: number;
  activePlaybackSessions: number;
  durationSeconds: number | null;
  positionSeconds: number;
  playbackRate: number;
  decodedFrames: number;
  metrics: MediaRuntimeMetrics;
  muted: boolean;
  volume: number;
  audioMonitoring: boolean;
  recording: RecordingRuntimeInfo;
  errorMessage: string | null;
  pipelineErrorMessage: string | null;
}

export interface MediaRuntimeMetrics {
  videoPacketsCaptured: number;
  videoFramesDecoded: number;
  videoPreviewFrames: number;
  videoPacketsEncoded: number;
  audioPacketsCaptured: number;
  audioFramesDecoded: number;
  audioPacketsEncoded: number;
  audioRms: number;
  audioPeak: number;
}

export function createEmptyMediaRuntimeMetrics(): MediaRuntimeMetrics {
  return {
    videoPacketsCaptured: 0,
    videoFramesDecoded: 0,
    videoPreviewFrames: 0,
    videoPacketsEncoded: 0,
    audioPacketsCaptured: 0,
    audioFramesDecoded: 0,
    audioPacketsEncoded: 0,
    audioRms: 0,
    audioPeak: 0,
  };
}
