import { computed, ref, shallowRef, toRaw } from 'vue';
import { defineStore } from 'pinia';

import { getMediaService } from '../services/media-service-provider';
import type { MediaVideoFrame } from '../services/media-service';
import { createDefaultMediaConfig } from '../types/media-defaults';
import {
  CaptureDeviceStatus,
  MediaSourceType,
  type VideoCodec,
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
    activeLiveSessions: 0,
    activePlaybackSessions: 0,
    durationSeconds: null,
    positionSeconds: 0,
    playbackRate: 1,
    decodedFrames: 0,
    muted: false,
    volume: 1,
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
    durationSeconds: null,
    positionSeconds: 0,
    playbackRate: 1,
    decodedFrames: 0,
    muted: false,
    volume: 1,
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
  const supportedVideoCodecs = ref<VideoCodec[]>([]);
  const fieldErrors = ref<MediaFieldErrors>({});
  const serviceError = ref<string | null>(null);
  const capabilityError = ref<string | null>(null);
  const encoderCapabilityError = ref<string | null>(null);
  const isInitializing = ref(false);
  const isProbing = ref(false);
  const isApplying = ref(false);
  const isSaving = ref(false);
  const isPreviewPending = ref(false);
  const isRefreshingDevices = ref(false);
  const isLoadingVideoCapabilities = ref(false);
  // 预览帧是大块、不可变的二进制快照，不应交给 Vue 深层代理。
  const previewFrame = shallowRef<MediaVideoFrame | null>(null);
  let frameTimer: ReturnType<typeof setTimeout> | null = null;
  let frameLoopActive = false;

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
          mode.supportedFramesPerSecond.includes(video.framesPerSecond),
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
      const [config, nextVideoDevices, nextAudioDevices, nextRuntimeStatus] = await Promise.all([
        service.loadConfig(),
        service.listVideoDevices(),
        service.listAudioDevices(),
        service.getRuntimeStatus(),
      ]);
      savedConfig.value = clone(config);
      draftConfig.value = clone(config);
      setCaptureDevices(nextVideoDevices, nextAudioDevices);
      runtimeStatus.value = nextRuntimeStatus;
      await refreshVideoEncoderCapabilities();
      if (config.source.type === MediaSourceType.Camera) {
        await refreshVideoCapabilities(draftConfig.value.source.camera.video.deviceId);
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
    capabilityError.value = null;
    if (sourceType === MediaSourceType.Camera) {
      probeResult.value = null;
      await Promise.all([
        refreshVideoCapabilities(draftConfig.value.source.camera.video.deviceId),
        refreshVideoEncoderCapabilities(),
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
    return refreshVideoCapabilities(deviceId);
  }

  /** 重新枚举摄像头和麦克风，并清理不再存在的设备选择。 */
  async function refreshCaptureDevices(): Promise<MediaOperationResult> {
    isRefreshingDevices.value = true;
    serviceError.value = null;
    try {
      const [nextVideoDevices, nextAudioDevices] = await Promise.all([
        service.listVideoDevices(),
        service.listAudioDevices(),
      ]);
      setCaptureDevices(nextVideoDevices, nextAudioDevices);
      if (draftConfig.value.source.type === MediaSourceType.Camera) {
        const [captureResult, encoderResult] = await Promise.all([
          refreshVideoCapabilities(draftConfig.value.source.camera.video.deviceId),
          refreshVideoEncoderCapabilities(),
        ]);
        return captureResult.ok ? encoderResult : captureResult;
      }
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error);
    } finally {
      isRefreshingDevices.value = false;
    }
  }

  async function refreshVideoCapabilities(deviceId: string): Promise<MediaOperationResult> {
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
      videoCapabilities.value = null;
      const message = '所选摄像头当前不可用。';
      capabilityError.value = message;
      fieldErrors.value = {
        ...fieldErrors.value,
        'source.camera.video.deviceId': message,
      };
      return { ok: false, message };
    }

    if (deviceId.trim().length === 0) {
      videoCapabilities.value = null;
      const message = '请选择摄像头。';
      capabilityError.value = message;
      fieldErrors.value = {
        ...fieldErrors.value,
        'source.camera.video.deviceId': message,
      };
      return { ok: false, message };
    }

    isLoadingVideoCapabilities.value = true;
    try {
      const capabilities = await service.getVideoCapabilities(deviceId);
      if (capabilities.modes.length === 0) {
        const message = '摄像头未报告可用的分辨率和帧率。';
        videoCapabilities.value = null;
        capabilityError.value = message;
        fieldErrors.value = {
          ...fieldErrors.value,
          'source.camera.video.resolution': message,
        };
        return { ok: false, message };
      }
      videoCapabilities.value = capabilities;
      normalizeVideoMode(capabilities);
      return { ok: true };
    } catch (error) {
      videoCapabilities.value = null;
      const message = errorMessage(error);
      capabilityError.value = message;
      fieldErrors.value = {
        ...fieldErrors.value,
        'source.camera.video.deviceId': message,
      };
      return { ok: false, message };
    } finally {
      isLoadingVideoCapabilities.value = false;
    }
  }

  async function refreshVideoEncoderCapabilities(): Promise<MediaOperationResult> {
    encoderCapabilityError.value = null;
    try {
      const capabilities = await service.getVideoEncoderCapabilities();
      supportedVideoCodecs.value = capabilities.supportedCodecs;
      if (capabilities.supportedCodecs.length === 0) {
        const message = '当前 FFmpeg 未包含 H.264 或 H.265 编码器。';
        encoderCapabilityError.value = message;
        return { ok: false, message };
      }
      return { ok: true };
    } catch (error) {
      supportedVideoCodecs.value = [];
      const message = errorMessage(error);
      encoderCapabilityError.value = message;
      return { ok: false, message };
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
      video.framesPerSecond = nearestFrameRate(
        mode.supportedFramesPerSecond,
        video.framesPerSecond,
      );
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
      refreshVideoCapabilities(draftConfig.value.source.camera.video.deviceId),
      refreshVideoEncoderCapabilities(),
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
      startFrameLoop();
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
    stopFrameLoop();
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
      stopFrameLoop(false);
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
      startFrameLoop();
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
        const frame = await service.stepFrame();
        if (frame !== null) previewFrame.value = frame;
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
      const frame = await service.stepFrame();
      if (frame !== null) {
        previewFrame.value = frame;
        runtimeStatus.value.positionSeconds = frame.positionSeconds;
        runtimeStatus.value.decodedFrames += 1;
      }
      return { ok: true };
    } catch (error) {
      return handleServiceFailure(error, true);
    }
  }

  function stopFrameLoop(clearFrame = true): void {
    frameLoopActive = false;
    if (frameTimer !== null) clearTimeout(frameTimer);
    frameTimer = null;
    if (clearFrame) previewFrame.value = null;
  }

  function startFrameLoop(): void {
    stopFrameLoop(false);
    frameLoopActive = true;
    const read = async (): Promise<void> => {
      if (!frameLoopActive) return;
      try {
        const frame = await service.readFrame();
        if (!frameLoopActive) return;
        if (frame !== null) {
          previewFrame.value = frame;
          runtimeStatus.value.positionSeconds = frame.positionSeconds;
          runtimeStatus.value.decodedFrames += 1;
        }
      } catch (error) {
        handleServiceFailure(error, true);
        stopFrameLoop();
        return;
      }
      // 按源帧率和倍速驱动预览；下一次读取始终在本次 IPC 完成后发起，避免重入。
      const sourceFramesPerSecond = runtimeStatus.value.video?.framesPerSecond || 25;
      const delay = Math.max(
        8,
        Math.round(1000 / sourceFramesPerSecond / runtimeStatus.value.playbackRate),
      );
      frameTimer = setTimeout(() => void read(), delay);
    };
    void read();
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
        video.framesPerSecond = nearestFrameRate(
          mode.supportedFramesPerSecond,
          video.framesPerSecond,
        );
      }
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
    previewFrame,
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
    stepPreviewFrame,
  };
});
