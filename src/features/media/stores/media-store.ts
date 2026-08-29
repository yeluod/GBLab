import { computed, ref, toRaw } from 'vue';
import { defineStore } from 'pinia';

import { getMediaService } from '../services/media-service-provider';
import { createMediaCapabilityController } from '../controllers/media-capability-controller';
import { createPreviewController } from '../controllers/preview-controller';
import { createDefaultMediaConfig } from '../types/media-defaults';
import {
  CaptureDeviceStatus,
  MediaSourceType,
  isFrameRateSupported,
  type GlobalMediaConfig,
} from '../types/media-config';
import {
  createEmptyMediaRuntimeMetrics,
  MediaSourceStatus,
  type MediaProbeResult,
  type MediaRuntimeStatus,
} from '../types/media-runtime';

export type MediaFieldErrors = Partial<Record<string, string>>;
export type MediaOperationResult = { ok: true } | { ok: false; message: string };

function clone<T>(value: T): T {
  return structuredClone(toRaw(value));
}

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  if (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof error.message === 'string' &&
    error.message.length > 0
  ) {
    return error.message;
  }
  if (typeof error === 'string' && error.length > 0) return error;
  return '媒体服务暂时不可用，请重试。';
}

function unavailableRuntime(message: string): MediaRuntimeStatus {
  return {
    sourceStatus: MediaSourceStatus.Error,
    sourceLabel: '媒体服务异常',
    video: null,
    audio: null,
    activeLiveConsumers: 0,
    activeRecorderConsumers: 0,
    durationSeconds: null,
    positionSeconds: 0,
    playbackRate: 1,
    decodedFrames: 0,
    metrics: createEmptyMediaRuntimeMetrics(),
    muted: false,
    volume: 1,
    audioMonitoring: false,
    errorMessage: message,
    pipelineErrorMessage: null,
  };
}

/** 校验全局媒体草稿，并返回可直接绑定到表单字段的错误。 */
export function validateMediaConfig(config: GlobalMediaConfig): MediaFieldErrors {
  const errors: MediaFieldErrors = {};

  if (config.source.type === MediaSourceType.Mp4) {
    if (config.source.mp4.filePath.trim().length === 0) {
      errors['source.mp4.filePath'] = '请选择 MP4 文件。';
    }
  } else {
    const { video, audio } = config.source.camera;
    if (video.deviceId.length === 0) {
      errors['source.camera.video.deviceId'] = '请选择摄像头。';
    }
    if (video.width <= 0 || video.height <= 0) {
      errors['source.camera.video.resolution'] = '请选择有效分辨率。';
    }
    if (video.framesPerSecond <= 0) {
      errors['source.camera.video.framesPerSecond'] = '请选择有效帧率。';
    }
    if (video.bitrateKbps < 128 || video.bitrateKbps > 100_000) {
      errors['source.camera.video.bitrateKbps'] = '视频码率必须介于 128 与 100000 Kbps。';
    }

    if (audio.isEnabled) {
      if (audio.deviceId.length === 0) {
        errors['source.camera.audio.deviceId'] = '启用音频后必须选择麦克风。';
      }
      if (audio.sampleRate <= 0) {
        errors['source.camera.audio.sampleRate'] = '请选择音频采样率。';
      }
      if (![1, 2].includes(audio.channels)) {
        errors['source.camera.audio.channels'] = '声道数只能为单声道或双声道。';
      }
      if (audio.bitrateKbps <= 0) {
        errors['source.camera.audio.bitrateKbps'] = '请输入有效音频码率。';
      }
      if (audio.codec === 'g711a' || audio.codec === 'g711u') {
        if (audio.sampleRate !== 8000) {
          errors['source.camera.audio.sampleRate'] = 'G.711 必须使用 8000 Hz。';
        }
        if (audio.channels !== 1) {
          errors['source.camera.audio.channels'] = 'G.711 必须使用单声道。';
        }
        if (audio.bitrateKbps !== 64) {
          errors['source.camera.audio.bitrateKbps'] = 'G.711 必须使用 64 Kbps。';
        }
      }
    }
  }

  if (config.recording.isEnabled && config.recording.directory.trim().length === 0) {
    errors['recording.directory'] = '启用本地录像后必须选择录像目录。';
  }

  return errors;
}

/** 全局媒体配置、草稿和 Mock 运行状态。 */
export const useMediaStore = defineStore('media', () => {
  const service = getMediaService();
  const initialConfig = createDefaultMediaConfig();
  const savedConfig = ref<GlobalMediaConfig>(clone(initialConfig));
  const draftConfig = ref<GlobalMediaConfig>(clone(initialConfig));
  const runtimeStatus = ref<MediaRuntimeStatus>({
    sourceStatus: MediaSourceStatus.Unconfigured,
    sourceLabel: '尚未加载',
    video: null,
    audio: null,
    activeLiveConsumers: 0,
    activeRecorderConsumers: 0,
    durationSeconds: null,
    positionSeconds: 0,
    playbackRate: 1,
    decodedFrames: 0,
    metrics: createEmptyMediaRuntimeMetrics(),
    muted: false,
    volume: 1,
    audioMonitoring: false,
    errorMessage: null,
    pipelineErrorMessage: null,
  });
  const probeResult = ref<MediaProbeResult | null>(null);
  const fieldErrors = ref<MediaFieldErrors>({});
  const serviceError = ref<string | null>(null);
  const isInitializing = ref(false);
  const isProbing = ref(false);
  const isApplying = ref(false);
  const isSaving = ref(false);
  const isPreviewPending = ref(false);
  const capabilities = createMediaCapabilityController(
    service,
    draftConfig,
    fieldErrors,
    errorMessage,
  );
  const {
    videoDevices,
    audioDevices,
    videoCapabilities,
    supportedVideoCodecs,
    videoEncoderCapabilities,
    capabilityError,
    encoderCapabilityError,
    isRefreshingDevices,
    isLoadingVideoCapabilities,
  } = capabilities;
  const preview = createPreviewController(service, runtimeStatus, (error) => {
    handleServiceFailure(error, true);
  });

  const hasUnsavedChanges = computed(
    () => JSON.stringify(savedConfig.value) !== JSON.stringify(draftConfig.value),
  );
  const hasValidCameraMode = computed(() => {
    const video = draftConfig.value.source.camera.video;
    return (
      capabilityError.value === null &&
      videoDevices.value.some(
        (device) => device.id === video.deviceId && device.status === CaptureDeviceStatus.Available,
      ) &&
      videoCapabilities.value?.modes.some(
        (mode) =>
          mode.width === video.width &&
          mode.height === video.height &&
          isFrameRateSupported(mode, video.framesPerSecond),
      ) === true
    );
  });
  const canStartPreview = computed(() => {
    if (
      isPreviewPending.value ||
      isInitializing.value ||
      isLoadingVideoCapabilities.value ||
      runtimeStatus.value.sourceStatus === MediaSourceStatus.Previewing
    ) {
      return false;
    }
    if (draftConfig.value.source.type === MediaSourceType.Mp4) {
      return draftConfig.value.source.mp4.filePath.trim().length > 0;
    }
    return hasValidCameraMode.value;
  });

  async function initialize(): Promise<MediaOperationResult> {
    isInitializing.value = true;
    serviceError.value = null;
    runtimeStatus.value = {
      ...runtimeStatus.value,
      sourceStatus: MediaSourceStatus.Loading,
      errorMessage: null,
    };
    try {
      const [config, nextRuntimeStatus] = await Promise.all([
        service.loadConfig(),
        service.getRuntimeStatus(),
      ]);
      savedConfig.value = clone(config);
      draftConfig.value = clone(config);
      runtimeStatus.value = nextRuntimeStatus;
      await capabilities.loadForInitialization();
      await capabilities.refreshVideoEncoderCapabilities();
      if (config.source.type === MediaSourceType.Camera) {
        await capabilities.refreshVideoCapabilities(draftConfig.value.source.camera.video.deviceId);
      }
      if (config.source.type === MediaSourceType.Mp4 && config.source.mp4.filePath.length > 0) {
        await probeCurrentMp4();
      }
      return { ok: true };
    } catch (error) {
      const message = errorMessage(error);
      serviceError.value = message;
      runtimeStatus.value = unavailableRuntime(message);
      return { ok: false, message };
    } finally {
      isInitializing.value = false;
    }
  }

  async function setSourceType(sourceType: MediaSourceType): Promise<void> {
    draftConfig.value.source.type = sourceType;
    fieldErrors.value = {};
    serviceError.value = null;
    capabilities.clearCapabilityError();
    if (sourceType === MediaSourceType.Camera) {
      probeResult.value = null;
      await Promise.all([
        capabilities.refreshVideoCapabilities(draftConfig.value.source.camera.video.deviceId),
        capabilities.refreshVideoEncoderCapabilities(),
      ]);
      return;
    }
    if (draftConfig.value.source.mp4.filePath.length > 0) {
      await probeCurrentMp4();
    }
  }

  async function selectMp4(): Promise<MediaOperationResult> {
    try {
      const selectedPath = await service.selectMp4(draftConfig.value.source.mp4.filePath);
      if (selectedPath === null) {
        return { ok: true };
      }
      draftConfig.value.source.mp4.filePath = selectedPath;
      delete fieldErrors.value['source.mp4.filePath'];
      if (draftConfig.value.preferences.shouldProbeAfterSelection) {
        return probeCurrentMp4();
      }
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error);
    }
  }

  async function probeCurrentMp4(): Promise<MediaOperationResult> {
    const filePath = draftConfig.value.source.mp4.filePath.trim();
    if (filePath.length === 0) {
      fieldErrors.value = {
        ...fieldErrors.value,
        'source.mp4.filePath': '请选择 MP4 文件。',
      };
      return { ok: false, message: '请选择 MP4 文件。' };
    }

    isProbing.value = true;
    serviceError.value = null;
    try {
      probeResult.value = await service.probeMp4(filePath);
      runtimeStatus.value = await service.getRuntimeStatus();
      serviceError.value = null;
      delete fieldErrors.value['source.mp4.filePath'];
      return { ok: true };
    } catch (error) {
      probeResult.value = null;
      return handleServiceFailure(error);
    } finally {
      isProbing.value = false;
    }
  }

  async function setVideoDevice(deviceId: string): Promise<MediaOperationResult> {
    draftConfig.value.source.camera.video.deviceId = deviceId;
    delete fieldErrors.value['source.camera.video.deviceId'];
    return capabilities.refreshVideoCapabilities(deviceId);
  }

  const refreshCaptureDevices = capabilities.refreshCaptureDevices;
  const setVideoResolution = capabilities.setVideoResolution;

  async function selectRecordingDirectory(): Promise<MediaOperationResult> {
    try {
      const directory = await service.selectRecordingDirectory(
        draftConfig.value.recording.directory,
      );
      if (directory !== null) {
        draftConfig.value.recording.directory = directory;
        delete fieldErrors.value['recording.directory'];
      }
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error);
    }
  }

  async function applyDraft(): Promise<MediaOperationResult> {
    const validation = validateDraft();
    if (!validation.ok) {
      return validation;
    }
    isApplying.value = true;
    try {
      runtimeStatus.value = await service.applyConfig(clone(draftConfig.value));
      serviceError.value = null;
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error, true);
    } finally {
      isApplying.value = false;
    }
  }

  async function saveDraft(): Promise<MediaOperationResult> {
    const validation = validateDraft();
    if (!validation.ok) {
      return validation;
    }
    isSaving.value = true;
    try {
      savedConfig.value = await service.saveConfig(clone(draftConfig.value));
      draftConfig.value = clone(savedConfig.value);
      serviceError.value = null;
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error);
    } finally {
      isSaving.value = false;
    }
  }

  async function resetDraft(): Promise<MediaOperationResult> {
    draftConfig.value = clone(savedConfig.value);
    fieldErrors.value = {};
    serviceError.value = null;
    if (
      draftConfig.value.source.type === MediaSourceType.Mp4 &&
      draftConfig.value.source.mp4.filePath.length > 0
    ) {
      return probeCurrentMp4();
    }
    probeResult.value = null;
    const [captureResult, encoderResult] = await Promise.all([
      capabilities.refreshVideoCapabilities(draftConfig.value.source.camera.video.deviceId),
      capabilities.refreshVideoEncoderCapabilities(),
    ]);
    return captureResult.ok ? encoderResult : captureResult;
  }

  async function startPreview(): Promise<MediaOperationResult> {
    const validation = validateDraft();
    if (!validation.ok) {
      return validation;
    }
    isPreviewPending.value = true;
    runtimeStatus.value = {
      ...runtimeStatus.value,
      sourceStatus: MediaSourceStatus.Loading,
      errorMessage: null,
    };
    try {
      runtimeStatus.value = await service.startPreview(clone(draftConfig.value));
      preview.start();
      serviceError.value = null;
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error, true);
    } finally {
      isPreviewPending.value = false;
    }
  }

  async function stopPreview(): Promise<MediaOperationResult> {
    isPreviewPending.value = true;
    // 先阻止后续读帧进入 IPC，再关闭后端会话，避免摄像头被预览循环重新占用。
    preview.stop();
    try {
      runtimeStatus.value = await service.stopPreview();
      serviceError.value = null;
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error, true);
    } finally {
      isPreviewPending.value = false;
    }
  }

  async function pausePreview(): Promise<MediaOperationResult> {
    isPreviewPending.value = true;
    try {
      runtimeStatus.value = await service.pausePreview();
      preview.stop(false);
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error, true);
    } finally {
      isPreviewPending.value = false;
    }
  }

  async function resumePreview(): Promise<MediaOperationResult> {
    isPreviewPending.value = true;
    try {
      runtimeStatus.value = await service.resumePreview();
      preview.start();
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error, true);
    } finally {
      isPreviewPending.value = false;
    }
  }

  async function seekPreview(positionSeconds: number): Promise<MediaOperationResult> {
    try {
      runtimeStatus.value = await service.seek(positionSeconds);
      if (runtimeStatus.value.sourceStatus === MediaSourceStatus.Paused) {
        await preview.step();
      }
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error, true);
    }
  }

  async function setPlaybackRate(rate: number): Promise<MediaOperationResult> {
    try {
      runtimeStatus.value = await service.setPlaybackRate(rate);
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error);
    }
  }

  async function setAudioControl(muted: boolean, volume: number): Promise<MediaOperationResult> {
    try {
      runtimeStatus.value = await service.setAudioControl(muted, volume);
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error);
    }
  }

  async function setAudioMonitoring(enabled: boolean): Promise<MediaOperationResult> {
    try {
      runtimeStatus.value = await service.setAudioMonitoring(enabled);
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error, true);
    }
  }

  async function stepPreviewFrame(): Promise<MediaOperationResult> {
    try {
      await preview.step();
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error, true);
    }
  }

  function validateDraft(): MediaOperationResult {
    fieldErrors.value = validateMediaConfig(draftConfig.value);
    const firstMessage = Object.values(fieldErrors.value)[0];
    return firstMessage === undefined ? { ok: true } : { ok: false, message: firstMessage };
  }

  function handleServiceFailure(error: unknown, affectsRuntime = false): MediaOperationResult {
    const message = errorMessage(error);
    serviceError.value = message;
    if (affectsRuntime) {
      runtimeStatus.value = {
        ...runtimeStatus.value,
        sourceStatus: MediaSourceStatus.Error,
        errorMessage: message,
      };
    }
    return { ok: false, message };
  }

  return {
    savedConfig,
    draftConfig,
    runtimeStatus,
    probeResult,
    videoDevices,
    audioDevices,
    videoCapabilities,
    supportedVideoCodecs,
    videoEncoderCapabilities,
    fieldErrors,
    serviceError,
    capabilityError,
    encoderCapabilityError,
    isInitializing,
    isProbing,
    isApplying,
    isSaving,
    isPreviewPending,
    isRefreshingDevices,
    isLoadingVideoCapabilities,
    hasUnsavedChanges,
    canStartPreview,
    previewFrame: preview.previewFrame,
    initialize,
    setSourceType,
    selectMp4,
    probeCurrentMp4,
    setVideoDevice,
    refreshCaptureDevices,
    setVideoResolution,
    selectRecordingDirectory,
    applyDraft,
    saveDraft,
    resetDraft,
    startPreview,
    stopPreview,
    pausePreview,
    resumePreview,
    seekPreview,
    setPlaybackRate,
    setAudioControl,
    setAudioMonitoring,
    stepPreviewFrame,
  };
});
