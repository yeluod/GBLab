import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';
import {
  MediaSourceStatus,
  MockMediaService,
  VideoCodec,
  configureMediaService,
  useMediaStore,
} from '@/features/media';

describe('全局 MP4 媒体 Store', () => {
  beforeEach(() => {
    configureMediaService(new MockMediaService());
    setActivePinia(createPinia());
  });
  it('初始化加载探测结果并区分草稿与保存配置', async () => {
    const store = useMediaStore();
    expect(await store.initialize()).toEqual({ ok: true });
    expect(store.savedConfig).not.toBe(store.draftConfig);
    expect(store.probeResult?.video.codec).toBe(VideoCodec.H265);
    expect(store.runtimeStatus.sourceStatus).toBe(MediaSourceStatus.Ready);
  });
  it('Apply 与 Save 遵循草稿生命周期', async () => {
    const store = useMediaStore();
    await store.initialize();
    store.draftConfig.source.mp4.isLooping = false;
    expect(await store.applyDraft()).toEqual({ ok: true });
    expect(store.hasUnsavedChanges).toBe(true);
    expect(await store.saveDraft()).toEqual({ ok: true });
    expect(store.hasUnsavedChanges).toBe(false);
  });
  it('预览、暂停、跳转、倍速和停止', async () => {
    const store = useMediaStore();
    await store.initialize();
    expect(await store.startPreview()).toEqual({ ok: true });
    expect(store.runtimeStatus.sourceStatus).toBe(MediaSourceStatus.Previewing);
    expect(await store.pausePreview()).toEqual({ ok: true });
    expect(await store.seekPreview(10)).toEqual({ ok: true });
    expect(await store.setPlaybackRate(2)).toEqual({ ok: true });
    expect(await store.stopPreview()).toEqual({ ok: true });
    expect(store.runtimeStatus.sourceStatus).toBe(MediaSourceStatus.Stopped);
  });
});
