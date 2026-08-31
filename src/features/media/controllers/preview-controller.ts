import { shallowRef, type Ref } from 'vue';

import type { MediaService, MediaVideoFrame } from '../services/media-service';
import { MediaSourceStatus, type MediaRuntimeStatus } from '../types/media-runtime';

/** Owns the bounded preview transport loop independently from media configuration state. */
export function createPreviewController(
  service: MediaService,
  runtimeStatus: Ref<MediaRuntimeStatus>,
  onFailure: (error: unknown) => void,
) {
  const previewFrame = shallowRef<MediaVideoFrame | null>(null);
  let frameTimer: ReturnType<typeof setTimeout> | null = null;
  let frameLoopActive = false;
  let statusTimer: ReturnType<typeof setTimeout> | null = null;
  let timelineGeneration = 0;

  function applyFrame(frame: MediaVideoFrame): void {
    previewFrame.value = frame;
    const previousPosition = runtimeStatus.value.positionSeconds;
    const isPlaying = runtimeStatus.value.sourceStatus === MediaSourceStatus.Previewing;
    const isLoopReset =
      isPlaying &&
      runtimeStatus.value.durationSeconds !== null &&
      frame.positionSeconds + 0.5 < previousPosition;
    // Decoder timestamps can briefly move backwards around reordered frames. Keep the
    // visible playback clock monotonic, while still allowing a real loop to reset it.
    runtimeStatus.value.positionSeconds =
      isPlaying && !isLoopReset
        ? Math.max(previousPosition, frame.positionSeconds)
        : frame.positionSeconds;
    runtimeStatus.value.decodedFrames += 1;
  }

  function stop(clearFrame = true): void {
    frameLoopActive = false;
    if (frameTimer !== null) clearTimeout(frameTimer);
    frameTimer = null;
    if (statusTimer !== null) clearTimeout(statusTimer);
    statusTimer = null;
    if (clearFrame) previewFrame.value = null;
  }

  /** Invalidates in-flight reads so a frame from before seek cannot rewind the UI. */
  function beginSeek(): void {
    timelineGeneration += 1;
  }

  function start(): void {
    stop(false);
    frameLoopActive = true;
    const refreshStatus = async (): Promise<void> => {
      if (!frameLoopActive) return;
      const generation = timelineGeneration;
      try {
        const nextStatus = await service.getRuntimeStatus();
        if (!frameLoopActive || generation !== timelineGeneration) {
          return;
        }
        // 播放期间位置由逐帧时钟推进；状态轮询可能晚于 readFrame 返回，不能覆盖更新的进度。
        if (runtimeStatus.value.sourceStatus === MediaSourceStatus.Previewing) {
          nextStatus.positionSeconds = runtimeStatus.value.positionSeconds;
          nextStatus.decodedFrames = runtimeStatus.value.decodedFrames;
        }
        runtimeStatus.value = nextStatus;
      } catch (error) {
        stop();
        onFailure(error);
        return;
      }
      if (
        runtimeStatus.value.sourceStatus === MediaSourceStatus.Stopped ||
        runtimeStatus.value.sourceStatus === MediaSourceStatus.Unconfigured ||
        runtimeStatus.value.sourceStatus === MediaSourceStatus.Error
      ) {
        stop();
        return;
      }
      if (frameLoopActive) statusTimer = setTimeout(() => void refreshStatus(), 400);
    };
    const read = async (): Promise<void> => {
      if (!frameLoopActive) return;
      const generation = timelineGeneration;
      try {
        const frame = await service.readFrame();
        if (!frameLoopActive) return;
        if (generation !== timelineGeneration) {
          return;
        }
        if (frame !== null) applyFrame(frame);
      } catch (error) {
        stop();
        onFailure(error);
        return;
      }
      const sourceFramesPerSecond = runtimeStatus.value.video?.framesPerSecond || 25;
      const delay = Math.max(
        8,
        Math.round(1000 / sourceFramesPerSecond / runtimeStatus.value.playbackRate),
      );
      frameTimer = setTimeout(() => void read(), delay);
    };
    void read();
    statusTimer = setTimeout(() => void refreshStatus(), 400);
  }

  async function step(): Promise<void> {
    const frame = await service.stepFrame();
    if (frame !== null) applyFrame(frame);
  }

  return { previewFrame, start, stop, step, beginSeek };
}
