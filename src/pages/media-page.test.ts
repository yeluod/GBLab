import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('naive-ui', async (importOriginal) => {
  const naiveUi = await importOriginal<typeof import('naive-ui')>();
  return {
    ...naiveUi,
    useMessage: () => ({ error: vi.fn(), success: vi.fn() }),
  };
});

import {
  MediaSourceStatus,
  MediaSourceType,
  MockMediaService,
  configureMediaService,
  useMediaStore,
} from '@/features/media';
import MediaPage from './media-page.vue';

describe('音视频源页面', () => {
  beforeEach(() => {
    configureMediaService(new MockMediaService());
    setActivePinia(createPinia());
  });

  it('展示全局 MP4 配置、媒体信息和运行状态', async () => {
    const wrapper = mount(MediaPage, { global: { plugins: [createPinia()] } });
    await flushPromises();

    expect(wrapper.text()).toContain('所有模拟设备和通道共享');
    expect(wrapper.text()).toContain('MP4 文件');
    expect(wrapper.text()).toContain('H.265');
    expect(wrapper.text()).toContain('MEDIA INFORMATION');
    expect(wrapper.text()).toContain('RUNTIME STATUS');
  });

  it('无音频 MP4 显示 None 且不显示为错误', async () => {
    const wrapper = mount(MediaPage, { global: { plugins: [createPinia()] } });
    await flushPromises();
    const store = useMediaStore();

    await store.selectMp4();
    await store.selectMp4();
    await flushPromises();

    expect(wrapper.text()).toContain('None（正常）');
    expect(store.runtimeStatus.sourceStatus).not.toBe(MediaSourceStatus.Error);
  });

  it('页面按钮驱动 Apply、Save、Reset 和 Preview Store 行为', async () => {
    const wrapper = mount(MediaPage, { global: { plugins: [createPinia()] } });
    await flushPromises();
    const store = useMediaStore();
    store.draftConfig.source.mp4.isLooping = false;

    await wrapper.get('[data-testid="apply-media"]').trigger('click');
    await flushPromises();
    expect(store.hasUnsavedChanges).toBe(true);

    await wrapper.get('[data-testid="save-media"]').trigger('click');
    await flushPromises();
    expect(store.hasUnsavedChanges).toBe(false);

    store.draftConfig.source.mp4.isLooping = true;
    await wrapper.get('[data-testid="reset-media"]').trigger('click');
    await flushPromises();
    expect(store.draftConfig.source.mp4.isLooping).toBe(false);

    await wrapper.get('[data-testid="start-preview"]').trigger('click');
    await flushPromises();
    expect(store.runtimeStatus.sourceStatus).toBe(MediaSourceStatus.Previewing);
    await wrapper.get('[data-testid="stop-preview"]').trigger('click');
    await flushPromises();
    expect(store.runtimeStatus.sourceStatus).toBe(MediaSourceStatus.Ready);
  });

  it('摄像头模式使用摄像头预览文案而不是 MP4 文案', async () => {
    const wrapper = mount(MediaPage, { global: { plugins: [createPinia()] } });
    await flushPromises();
    const store = useMediaStore();

    await store.setSourceType(MediaSourceType.Camera);
    await flushPromises();

    expect(wrapper.text()).toContain('选择摄像头和采集参数后可开始预览');
    expect(wrapper.text()).not.toContain('选择并检测 MP4 后可开始预览');
  });
});
