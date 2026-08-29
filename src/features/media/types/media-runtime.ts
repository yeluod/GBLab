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
  muted: boolean;
  volume: number;
  recording: RecordingRuntimeInfo;
  errorMessage: string | null;
}
