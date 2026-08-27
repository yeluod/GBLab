import { computed, ref, toRaw } from 'vue';
import { defineStore } from 'pinia';

import { getMediaService } from '../services/media-service-provider';
import { createDefaultMediaConfig } from '../types/media-defaults';
import {
  CaptureDeviceStatus,
  MediaSourceType,
  type CaptureDeviceCapabilities,
  type CaptureDeviceInfo,
  type GlobalMediaConfig,
} from '../types/media-config';
import {
  MediaSourceStatus,
  RecordingStatus,
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
  return '媒体服务暂时不可用，请重试。';
}

function unavailableRuntime(message: string): MediaRuntimeStatus {
  return {
    sourceStatus: MediaSourceStatus.Error,
    sourceLabel: '媒体服务异常',
    video: null,
    audio: null,
    activeLiveSessions: 0,
    activePlaybackSessions: 0,
    recording: {
      status: RecordingStatus.Error,
      currentFile: null,
      recordedDurationSeconds: 0,
      usedSpaceBytes: 0,
    },
    errorMessage: message,
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
    activeLiveSessions: 0,
    activePlaybackSessions: 0,
    recording: {
      status: RecordingStatus.Disabled,
      currentFile: null,
      recordedDurationSeconds: 0,
      usedSpaceBytes: 0,
    },
    errorMessage: null,
  });
  const probeResult = ref<MediaProbeResult | null>(null);
  const videoDevices = ref<CaptureDeviceInfo[]>([]);
  const audioDevices = ref<CaptureDeviceInfo[]>([]);
  const videoCapabilities = ref<CaptureDeviceCapabilities | null>(null);
  const fieldErrors = ref<MediaFieldErrors>({});
  const serviceError = ref<string | null>(null);
  const isInitializing = ref(false);
  const isProbing = ref(false);
  const isApplying = ref(false);
  const isSaving = ref(false);
  const isPreviewPending = ref(false);

  const hasUnsavedChanges = computed(
    () => JSON.stringify(savedConfig.value) !== JSON.stringify(draftConfig.value),
  );
  const canStartPreview = computed(
    () =>
      !isPreviewPending.value &&
      !isInitializing.value &&
      runtimeStatus.value.sourceStatus !== MediaSourceStatus.Previewing,
  );

  async function initialize(): Promise<MediaOperationResult> {
    isInitializing.value = true;
    serviceError.value = null;
    runtimeStatus.value = {
      ...runtimeStatus.value,
      sourceStatus: MediaSourceStatus.Loading,
      errorMessage: null,
    };
    try {
      const [config, nextVideoDevices, nextAudioDevices, nextRuntimeStatus] = await Promise.all([
        service.loadConfig(),
        service.listVideoDevices(),
        service.listAudioDevices(),
        service.getRuntimeStatus(),
      ]);
      savedConfig.value = clone(config);
      draftConfig.value = clone(config);
      videoDevices.value = nextVideoDevices;
      audioDevices.value = nextAudioDevices;
      runtimeStatus.value = nextRuntimeStatus;
      await refreshVideoCapabilities(config.source.camera.video.deviceId);
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
    if (sourceType === MediaSourceType.Camera) {
      probeResult.value = null;
      await refreshVideoCapabilities(draftConfig.value.source.camera.video.deviceId);
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
    return refreshVideoCapabilities(deviceId);
  }

  async function refreshVideoCapabilities(deviceId: string): Promise<MediaOperationResult> {
    const device = videoDevices.value.find((item) => item.id === deviceId);
    if (device?.status !== CaptureDeviceStatus.Available) {
      videoCapabilities.value = null;
      const message = '所选摄像头当前不可用。';
      fieldErrors.value = {
        ...fieldErrors.value,
        'source.camera.video.deviceId': message,
      };
      return { ok: false, message };
    }

    try {
      const capabilities = await service.getVideoCapabilities(deviceId);
      videoCapabilities.value = capabilities;
      normalizeVideoMode(capabilities);
      return { ok: true };
    } catch (error) {
      videoCapabilities.value = null;
      return handleServiceFailure(error);
    }
  }

  function setVideoResolution(width: number, height: number): void {
    const mode = videoCapabilities.value?.modes.find(
      (item) => item.width === width && item.height === height,
    );
    if (mode === undefined) {
      return;
    }
    const video = draftConfig.value.source.camera.video;
    video.width = width;
    video.height = height;
    if (!mode.supportedFramesPerSecond.includes(video.framesPerSecond)) {
      video.framesPerSecond = mode.supportedFramesPerSecond[0] ?? 25;
    }
    delete fieldErrors.value['source.camera.video.resolution'];
    delete fieldErrors.value['source.camera.video.framesPerSecond'];
  }

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
      return handleServiceFailure(error);
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
    return refreshVideoCapabilities(draftConfig.value.source.camera.video.deviceId);
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
      serviceError.value = null;
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error);
    } finally {
      isPreviewPending.value = false;
    }
  }

  async function stopPreview(): Promise<MediaOperationResult> {
    isPreviewPending.value = true;
    try {
      runtimeStatus.value = await service.stopPreview();
      serviceError.value = null;
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error);
    } finally {
      isPreviewPending.value = false;
    }
  }

  function validateDraft(): MediaOperationResult {
    fieldErrors.value = validateMediaConfig(draftConfig.value);
    const firstMessage = Object.values(fieldErrors.value)[0];
    return firstMessage === undefined ? { ok: true } : { ok: false, message: firstMessage };
  }

  function normalizeVideoMode(capabilities: CaptureDeviceCapabilities): void {
    const video = draftConfig.value.source.camera.video;
    let mode = capabilities.modes.find(
      (item) => item.width === video.width && item.height === video.height,
    );
    mode ??= capabilities.modes[0];
    if (mode !== undefined) {
      video.width = mode.width;
      video.height = mode.height;
      if (!mode.supportedFramesPerSecond.includes(video.framesPerSecond)) {
        video.framesPerSecond = mode.supportedFramesPerSecond[0] ?? 25;
      }
    }
    if (!capabilities.supportedCodecs.includes(video.codec)) {
      video.codec = capabilities.supportedCodecs[0] ?? video.codec;
    }
  }

  function handleServiceFailure(error: unknown): MediaOperationResult {
    const message = errorMessage(error);
    serviceError.value = message;
    runtimeStatus.value = unavailableRuntime(message);
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
    fieldErrors,
    serviceError,
    isInitializing,
    isProbing,
    isApplying,
    isSaving,
    isPreviewPending,
    hasUnsavedChanges,
    canStartPreview,
    initialize,
    setSourceType,
    selectMp4,
    probeCurrentMp4,
    setVideoDevice,
    setVideoResolution,
    selectRecordingDirectory,
    applyDraft,
    saveDraft,
    resetDraft,
    startPreview,
    stopPreview,
  };
});
