import { computed, ref, toRaw } from 'vue';
import { defineStore } from 'pinia';

import { getMediaService } from '../services/media-service-provider';
import { createPreviewController } from '../controllers/preview-controller';
import { createDefaultMediaConfig } from '../types/media-defaults';
import { MediaSourceType, type GlobalMediaConfig } from '../types/media-config';
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
    durationSeconds: null,
    positionSeconds: 0,
    playbackRate: 1,
    decodedFrames: 0,
    metrics: createEmptyMediaRuntimeMetrics(),
    muted: false,
    volume: 1,
    errorMessage: message,
    pipelineErrorMessage: null,
    audioSink: null,
  };
}

/** 校验全局媒体草稿，并返回可直接绑定到表单字段的错误。 */
export function validateMediaConfig(config: GlobalMediaConfig): MediaFieldErrors {
  const errors: MediaFieldErrors = {};

  if (config.source.mp4.filePath.trim().length === 0) {
    errors['source.mp4.filePath'] = '请选择 MP4 文件。';
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
    durationSeconds: null,
    positionSeconds: 0,
    playbackRate: 1,
    decodedFrames: 0,
    metrics: createEmptyMediaRuntimeMetrics(),
    muted: false,
    volume: 1,
    errorMessage: null,
    pipelineErrorMessage: null,
    audioSink: null,
  });
  const probeResult = ref<MediaProbeResult | null>(null);
  const fieldErrors = ref<MediaFieldErrors>({});
  const serviceError = ref<string | null>(null);
  const isInitializing = ref(false);
  const isProbing = ref(false);
  const isApplying = ref(false);
  const isSaving = ref(false);
  const isPreviewPending = ref(false);
  const preview = createPreviewController(service, runtimeStatus, (error) => {
    handleServiceFailure(error, true);
  });

  const hasUnsavedChanges = computed(
    () => JSON.stringify(savedConfig.value) !== JSON.stringify(draftConfig.value),
  );
  const canStartPreview = computed(() => {
    if (
      isPreviewPending.value ||
      isInitializing.value ||
      runtimeStatus.value.sourceStatus === MediaSourceStatus.Previewing
    ) {
      return false;
    }
    return draftConfig.value.source.mp4.filePath.trim().length > 0;
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
    return { ok: true };
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
    // 先阻止后续读帧进入 IPC，再关闭后端会话。
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
    const wasPreviewing = runtimeStatus.value.sourceStatus === MediaSourceStatus.Previewing;
    preview.beginSeek();
    try {
      runtimeStatus.value = await service.seek(positionSeconds);
      if (runtimeStatus.value.sourceStatus === MediaSourceStatus.Paused) {
        await preview.step();
      } else if (
        wasPreviewing &&
        runtimeStatus.value.sourceStatus === MediaSourceStatus.Previewing
      ) {
        // A frame that was in flight before seek is discarded by the controller. Restart
        // the loop only after the backend has established the new timeline.
        preview.start();
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
    fieldErrors,
    serviceError,
    isInitializing,
    isProbing,
    isApplying,
    isSaving,
    isPreviewPending,
    hasUnsavedChanges,
    canStartPreview,
    previewFrame: preview.previewFrame,
    initialize,
    selectMp4,
    probeCurrentMp4,
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
    stepPreviewFrame,
  };
});
