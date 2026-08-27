import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';

import {
  AudioCodec,
  MediaSourceStatus,
  MediaSourceType,
  MockMediaService,
  RecordingStatus,
  VideoCodec,
  configureMediaService,
  createDefaultMediaConfig,
  useMediaStore,
} from '@/features/media';

describe('全局媒体 Store', () => {
  beforeEach(() => {
    configureMediaService(new MockMediaService());
    setActivePinia(createPinia());
  });

  it('初始化后区分已保存配置、草稿和运行状态', async () => {
    const store = useMediaStore();

    const result = await store.initialize();

    expect(result).toEqual({ ok: true });
    expect(store.savedConfig).not.toBe(store.draftConfig);
    expect(store.probeResult?.video.codec).toBe(VideoCodec.H265);
    expect(store.runtimeStatus.sourceStatus).toBe(MediaSourceStatus.Ready);
  });

  it('切换 Camera 后加载当前设备能力', async () => {
    const store = useMediaStore();
    await store.initialize();

    await store.setSourceType(MediaSourceType.Camera);

    expect(store.draftConfig.source.type).toBe(MediaSourceType.Camera);
    expect(store.videoCapabilities?.deviceId).toBe('camera-integrated');
    expect(store.videoCapabilities?.modes).toHaveLength(3);
  });

  it('切换摄像头时按能力联动分辨率、FPS 和编码', async () => {
    const store = useMediaStore();
    await store.initialize();
    await store.setSourceType(MediaSourceType.Camera);

    await store.setVideoDevice('camera-usb');

    expect(store.draftConfig.source.camera.video.width).toBe(640);
    expect(store.draftConfig.source.camera.video.framesPerSecond).toBe(15);
    expect(store.draftConfig.source.camera.video.codec).toBe(VideoCodec.H264);
  });

  it('选择支持的分辨率后联动到对应 FPS 集合', async () => {
    const store = useMediaStore();
    await store.initialize();
    await store.setSourceType(MediaSourceType.Camera);
    store.draftConfig.source.camera.video.framesPerSecond = 60;

    store.setVideoResolution(1920, 1080);

    expect(store.draftConfig.source.camera.video.framesPerSecond).toBe(25);
  });

  it('关闭音频时保留字段但不参与校验', async () => {
    const store = useMediaStore();
    await store.initialize();
    await store.setSourceType(MediaSourceType.Camera);
    store.draftConfig.source.camera.audio.isEnabled = false;
    store.draftConfig.source.camera.audio.deviceId = '';
    store.draftConfig.source.camera.audio.sampleRate = 0;
    store.draftConfig.source.camera.audio.channels = 0;

    const result = await store.applyDraft();

    expect(result).toEqual({ ok: true });
    expect(store.runtimeStatus.audio).toBeNull();
  });

  it('启用音频时要求选择麦克风和有效采集参数', async () => {
    const store = useMediaStore();
    await store.initialize();
    await store.setSourceType(MediaSourceType.Camera);
    store.draftConfig.source.camera.audio.isEnabled = true;
    store.draftConfig.source.camera.audio.deviceId = '';

    const result = await store.applyDraft();

    expect(result.ok).toBe(false);
    expect(store.fieldErrors['source.camera.audio.deviceId']).toContain('麦克风');
  });

  it('允许 Camera + Microphone 使用 G711A 配置', async () => {
    const store = useMediaStore();
    await store.initialize();
    await store.setSourceType(MediaSourceType.Camera);
    store.draftConfig.source.camera.audio.codec = AudioCodec.G711A;
    store.draftConfig.source.camera.audio.sampleRate = 8_000;
    store.draftConfig.source.camera.audio.channels = 1;
    store.draftConfig.source.camera.audio.bitrateKbps = 64;

    const result = await store.applyDraft();

    expect(result).toEqual({ ok: true });
    expect(store.runtimeStatus.audio?.codec).toBe(AudioCodec.G711A);
  });

  it('启用录像时要求指定目录，关闭时不校验目录', async () => {
    const store = useMediaStore();
    await store.initialize();
    store.draftConfig.recording.isEnabled = true;
    store.draftConfig.recording.directory = '';

    const invalid = await store.applyDraft();
    store.draftConfig.recording.isEnabled = false;
    const valid = await store.applyDraft();

    expect(invalid.ok).toBe(false);
    expect(store.fieldErrors['recording.directory']).toBeUndefined();
    expect(valid).toEqual({ ok: true });
    expect(store.runtimeStatus.recording.status).toBe(RecordingStatus.Disabled);
  });

  it('Apply 只更新 Mock Runtime，不改写保存配置', async () => {
    const store = useMediaStore();
    await store.initialize();
    store.draftConfig.source.mp4.isLooping = false;

    const result = await store.applyDraft();

    expect(result).toEqual({ ok: true });
    expect(store.savedConfig.source.mp4.isLooping).toBe(true);
    expect(store.hasUnsavedChanges).toBe(true);
  });

  it('Save 保存草稿并清除未保存状态', async () => {
    const store = useMediaStore();
    await store.initialize();
    store.draftConfig.source.mp4.isLooping = false;

    const result = await store.saveDraft();

    expect(result).toEqual({ ok: true });
    expect(store.savedConfig.source.mp4.isLooping).toBe(false);
    expect(store.hasUnsavedChanges).toBe(false);
  });

  it('Reset 恢复最后保存的配置', async () => {
    const initialConfig = createDefaultMediaConfig();
    initialConfig.source.mp4.isLooping = false;
    configureMediaService(new MockMediaService({ initialConfig }));
    setActivePinia(createPinia());
    const store = useMediaStore();
    await store.initialize();
    store.draftConfig.source.mp4.isLooping = true;

    await store.resetDraft();

    expect(store.draftConfig.source.mp4.isLooping).toBe(false);
    expect(store.hasUnsavedChanges).toBe(false);
  });

  it('Preview 从加载态进入预览态并可停止', async () => {
    const store = useMediaStore();
    await store.initialize();

    const started = await store.startPreview();
    const stopped = await store.stopPreview();

    expect(started).toEqual({ ok: true });
    expect(stopped).toEqual({ ok: true });
    expect(store.runtimeStatus.sourceStatus).toBe(MediaSourceStatus.Ready);
  });

  it('服务异常会投影为 Error 状态和可见错误信息', async () => {
    configureMediaService(new MockMediaService({ failures: ['getRuntimeStatus'] }));
    setActivePinia(createPinia());
    const store = useMediaStore();

    const result = await store.initialize();

    expect(result.ok).toBe(false);
    expect(store.runtimeStatus.sourceStatus).toBe(MediaSourceStatus.Error);
    expect(store.serviceError).toContain('getRuntimeStatus 失败');
  });
});
