import { AudioCodec } from './media-config';
import type { VideoCodec } from './media-config';

/** Codec detected in a source stream; it may be outside the capture target set. */
export type DetectedAudioCodec = AudioCodec | 'other';

export function normalizeDetectedAudioCodec(value: string): DetectedAudioCodec {
  if (value === AudioCodec.Aac) return AudioCodec.Aac;
  if (value === AudioCodec.G711A) return AudioCodec.G711A;
  if (value === AudioCodec.G711U) return AudioCodec.G711U;
  return 'other';
}

export enum MediaSourceStatus {
  Unconfigured = 'unconfigured',
  Loading = 'loading',
  Ready = 'ready',
  Previewing = 'previewing',
  Paused = 'paused',
  Stopped = 'stopped',
  Error = 'error',
  Unavailable = 'unavailable',
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
  codec: DetectedAudioCodec;
  sampleRate: number;
  channels: number;
  bitrateKbps: number;
}

export interface MediaProbeResult {
  filePath: string;
  video: VideoStreamInfo;
  audio: AudioStreamInfo | null;
}

export interface MediaRuntimeStatus {
  sourceStatus: MediaSourceStatus;
  sourceLabel: string;
  video: VideoStreamInfo | null;
  audio: AudioStreamInfo | null;
  activeLiveConsumers: number;
  activeRecorderConsumers: number;
  durationSeconds: number | null;
  positionSeconds: number;
  playbackRate: number;
  decodedFrames: number;
  metrics: MediaRuntimeMetrics;
  muted: boolean;
  volume: number;
  audioMonitoring: boolean;
  errorMessage: string | null;
  pipelineErrorMessage: string | null;
  audioSink: AudioSinkInfo | null;
}

export type AudioSinkStatus = 'unavailable' | 'paused' | 'playing' | 'error';

export interface AudioSinkInfo {
  status: AudioSinkStatus;
  queuedSamples: number;
  playedSamples: number;
  underruns: number;
  droppedSamples: number;
  lastError: string | null;
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
