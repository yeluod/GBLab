import { shallowRef, type Ref } from 'vue';

import type { MediaService, MediaVideoFrame } from '../services/media-service';
import type { MediaRuntimeStatus } from '../types/media-runtime';

/** Owns the bounded preview transport loop independently from media configuration state. */
export function createPreviewController(
  service: MediaService,
  runtimeStatus: Ref<MediaRuntimeStatus>,
  onFailure: (error: unknown) => void,
) {
  const previewFrame = shallowRef<MediaVideoFrame | null>(null);
  let frameTimer: ReturnType<typeof setTimeout> | null = null;
  let frameLoopActive = false;

  function applyFrame(frame: MediaVideoFrame): void {
    previewFrame.value = frame;
    runtimeStatus.value.positionSeconds = frame.positionSeconds;
    runtimeStatus.value.decodedFrames += 1;
  }

  function stop(clearFrame = true): void {
    frameLoopActive = false;
    if (frameTimer !== null) clearTimeout(frameTimer);
    frameTimer = null;
    if (clearFrame) previewFrame.value = null;
  }

  function start(): void {
    stop(false);
    frameLoopActive = true;
    const read = async (): Promise<void> => {
      if (!frameLoopActive) return;
      try {
        const frame = await service.readFrame();
        if (!frameLoopActive) return;
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
  }

  async function step(): Promise<void> {
    const frame = await service.stepFrame();
    if (frame !== null) applyFrame(frame);
  }

  return { previewFrame, start, stop, step };
}
