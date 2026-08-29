import { ref, type Ref } from 'vue';

import type { MediaService } from '../services/media-service';
import {
  CaptureDeviceStatus,
  EncoderBackend,
  isFrameRateSupported,
  selectableFrameRates,
  type CaptureDeviceCapabilities,
  type CaptureDeviceInfo,
  type GlobalMediaConfig,
  type VideoCodec,
  type VideoEncoderCapability,
} from '../types/media-config';

type FieldErrors = Partial<Record<string, string>>;
type OperationResult = { ok: true } | { ok: false; message: string };

/** Owns native device discovery and capture/encoder capability normalization. */
export function createMediaCapabilityController(
  service: MediaService,
  draftConfig: Ref<GlobalMediaConfig>,
  fieldErrors: Ref<FieldErrors>,
  errorMessage: (error: unknown) => string,
) {
  const videoDevices = ref<CaptureDeviceInfo[]>([]);
  const audioDevices = ref<CaptureDeviceInfo[]>([]);
  const videoCapabilities = ref<CaptureDeviceCapabilities | null>(null);
  const supportedVideoCodecs = ref<VideoCodec[]>([]);
  const videoEncoderCapabilities = ref<VideoEncoderCapability[]>([]);
  const capabilityError = ref<string | null>(null);
  const encoderCapabilityError = ref<string | null>(null);
  const isRefreshingDevices = ref(false);
  const isLoadingVideoCapabilities = ref(false);

  async function loadForInitialization(): Promise<void> {
    const [videoResult, audioResult] = await Promise.allSettled([
      service.listVideoDevices(),
      service.listAudioDevices(),
    ]);
    const nextVideoDevices = videoResult.status === 'fulfilled' ? videoResult.value : [];
    const nextAudioDevices = audioResult.status === 'fulfilled' ? audioResult.value : [];
    setCaptureDevices(nextVideoDevices, nextAudioDevices);
    const failed =
      videoResult.status === 'rejected'
        ? videoResult.reason
        : audioResult.status === 'rejected'
          ? audioResult.reason
          : null;
    if (failed !== null) capabilityError.value = errorMessage(failed);
  }

  async function refreshCaptureDevices(): Promise<OperationResult> {
    isRefreshingDevices.value = true;
    capabilityError.value = null;
    try {
      const [nextVideoDevices, nextAudioDevices] = await Promise.all([
        service.listVideoDevices(),
        service.listAudioDevices(),
      ]);
      setCaptureDevices(nextVideoDevices, nextAudioDevices);
      const [captureResult, encoderResult] = await Promise.all([
        refreshVideoCapabilities(draftConfig.value.source.camera.video.deviceId),
        refreshVideoEncoderCapabilities(),
      ]);
      return captureResult.ok ? encoderResult : captureResult;
    } catch (error) {
      const message = errorMessage(error);
      capabilityError.value = message;
      return { ok: false, message };
    } finally {
      isRefreshingDevices.value = false;
    }
  }

  async function refreshVideoCapabilities(deviceId: string): Promise<OperationResult> {
    capabilityError.value = null;
    delete fieldErrors.value['source.camera.video.deviceId'];
    delete fieldErrors.value['source.camera.video.resolution'];
    delete fieldErrors.value['source.camera.video.framesPerSecond'];
    const device = videoDevices.value.find((item) => item.id === deviceId);
    if (
      device?.status === CaptureDeviceStatus.Busy ||
      device?.status === CaptureDeviceStatus.Unavailable ||
      device?.status === CaptureDeviceStatus.PermissionDenied
    ) {
      return captureFailure('所选摄像头当前不可用。', 'source.camera.video.deviceId');
    }
    if (deviceId.trim().length === 0) {
      return captureFailure('请选择摄像头。', 'source.camera.video.deviceId');
    }

    isLoadingVideoCapabilities.value = true;
    try {
      const capabilities = await service.getVideoCapabilities(deviceId);
      if (capabilities.modes.length === 0) {
        return captureFailure('摄像头未报告可用的分辨率和帧率。', 'source.camera.video.resolution');
      }
      videoCapabilities.value = capabilities;
      normalizeVideoMode(capabilities);
      return { ok: true };
    } catch (error) {
      return captureFailure(errorMessage(error), 'source.camera.video.deviceId');
    } finally {
      isLoadingVideoCapabilities.value = false;
    }
  }

  async function refreshVideoEncoderCapabilities(): Promise<OperationResult> {
    encoderCapabilityError.value = null;
    try {
      const capabilities = await service.getVideoEncoderCapabilities();
      videoEncoderCapabilities.value = capabilities.encoders;
      supportedVideoCodecs.value = [...new Set(capabilities.encoders.map((item) => item.codec))];
      const selectedVideo = draftConfig.value.source.camera.video;
      if (
        selectedVideo.encoderBackend !== EncoderBackend.Auto &&
        !capabilities.encoders.some(
          (item) =>
            item.codec === selectedVideo.codec && item.backend === selectedVideo.encoderBackend,
        )
      ) {
        selectedVideo.encoderBackend = EncoderBackend.Auto;
      }
      if (capabilities.encoders.length === 0) {
        const message = '当前 FFmpeg 未包含 H.264 或 H.265 编码器。';
        encoderCapabilityError.value = message;
        return { ok: false, message };
      }
      return { ok: true };
    } catch (error) {
      supportedVideoCodecs.value = [];
      videoEncoderCapabilities.value = [];
      const message = errorMessage(error);
      encoderCapabilityError.value = message;
      return { ok: false, message };
    }
  }

  function setVideoResolution(width: number, height: number): void {
    const mode = videoCapabilities.value?.modes.find(
      (item) => item.width === width && item.height === height,
    );
    if (mode === undefined) return;
    const video = draftConfig.value.source.camera.video;
    video.width = width;
    video.height = height;
    if (!isFrameRateSupported(mode, video.framesPerSecond)) {
      video.framesPerSecond = nearestFrameRate(selectableFrameRates(mode), video.framesPerSecond);
    }
    delete fieldErrors.value['source.camera.video.resolution'];
    delete fieldErrors.value['source.camera.video.framesPerSecond'];
  }

  function clearCapabilityError(): void {
    capabilityError.value = null;
  }

  function captureFailure(message: string, field: string): OperationResult {
    videoCapabilities.value = null;
    capabilityError.value = message;
    fieldErrors.value = { ...fieldErrors.value, [field]: message };
    return { ok: false, message };
  }

  function normalizeVideoMode(capabilities: CaptureDeviceCapabilities): void {
    const video = draftConfig.value.source.camera.video;
    let mode = capabilities.modes.find(
      (item) => item.width === video.width && item.height === video.height,
    );
    mode ??= capabilities.modes[0];
    if (mode === undefined) return;
    video.width = mode.width;
    video.height = mode.height;
    if (!isFrameRateSupported(mode, video.framesPerSecond)) {
      video.framesPerSecond = nearestFrameRate(selectableFrameRates(mode), video.framesPerSecond);
    }
  }

  function nearestFrameRate(frameRates: number[], preferred: number): number {
    return (
      frameRates.reduce<number | undefined>((nearest, candidate) => {
        if (nearest === undefined) return candidate;
        return Math.abs(candidate - preferred) < Math.abs(nearest - preferred)
          ? candidate
          : nearest;
      }, undefined) ?? 25
    );
  }

  function setCaptureDevices(
    nextVideoDevices: CaptureDeviceInfo[],
    nextAudioDevices: CaptureDeviceInfo[],
  ): void {
    videoDevices.value = nextVideoDevices;
    audioDevices.value = nextAudioDevices;

    const availableVideo = nextVideoDevices.find(
      (device) => device.status === CaptureDeviceStatus.Available,
    );
    const selectedVideo = nextVideoDevices.find(
      (device) => device.id === draftConfig.value.source.camera.video.deviceId,
    );
    if (selectedVideo?.status !== CaptureDeviceStatus.Available) {
      draftConfig.value.source.camera.video.deviceId = availableVideo?.id ?? '';
      videoCapabilities.value = null;
    }

    const availableAudio = nextAudioDevices.find(
      (device) => device.status === CaptureDeviceStatus.Available,
    );
    const selectedAudio = nextAudioDevices.find(
      (device) => device.id === draftConfig.value.source.camera.audio.deviceId,
    );
    if (selectedAudio?.status !== CaptureDeviceStatus.Available) {
      draftConfig.value.source.camera.audio.deviceId = availableAudio?.id ?? '';
    }
  }

  return {
    videoDevices,
    audioDevices,
    videoCapabilities,
    supportedVideoCodecs,
    videoEncoderCapabilities,
    capabilityError,
    encoderCapabilityError,
    isRefreshingDevices,
    isLoadingVideoCapabilities,
    loadForInitialization,
    refreshCaptureDevices,
    refreshVideoCapabilities,
    refreshVideoEncoderCapabilities,
    setVideoResolution,
    clearCapabilityError,
  };
}
