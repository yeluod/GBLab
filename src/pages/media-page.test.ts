import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('naive-ui', async (importOriginal) => {
  const naiveUi = await importOriginal<typeof import('naive-ui')>();
  return { ...naiveUi, useMessage: () => ({ error: vi.fn(), success: vi.fn() }) };
});
import {
  MediaSourceStatus,
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
  it('展示 MP4 配置、媒体信息和运行状态', async () => {
    const wrapper = mount(MediaPage, { global: { plugins: [createPinia()] } });
    await flushPromises();
    expect(wrapper.text()).toContain('MP4 文件');
    expect(wrapper.text()).toContain('MEDIA INFORMATION');
    expect(wrapper.text()).toContain('RUNTIME STATUS');
  });
  it('页面按钮驱动预览状态', async () => {
    const wrapper = mount(MediaPage, { global: { plugins: [createPinia()] } });
    await flushPromises();
    const store = useMediaStore();
    await wrapper.get('[data-testid="start-preview"]').trigger('click');
    await flushPromises();
    expect(store.runtimeStatus.sourceStatus).toBe(MediaSourceStatus.Previewing);
    await wrapper.get('[data-testid="stop-preview"]').trigger('click');
    await flushPromises();
    expect(store.runtimeStatus.sourceStatus).toBe(MediaSourceStatus.Stopped);
  });
});
