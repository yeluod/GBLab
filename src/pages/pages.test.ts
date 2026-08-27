import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { nextTick } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('naive-ui', async (importOriginal) => {
  const naiveUi = await importOriginal<typeof import('naive-ui')>();
  return {
    ...naiveUi,
    useDialog: () => ({ warning: vi.fn() }),
    useMessage: () => ({ error: vi.fn(), success: vi.fn() }),
  };
});

const settingsMocks = vi.hoisted(() => ({
  getSipServiceConfiguration: vi.fn(),
  saveSipServiceConfiguration: vi.fn(),
}));
const deviceApiMocks = vi.hoisted(() => ({
  getDeviceSnapshot: vi.fn(),
  getDeviceChannels: vi.fn(),
  addDevicesInBatchCommand: vi.fn(),
  updateDeviceCommand: vi.fn(),
  deleteDeviceCommand: vi.fn(),
}));
const registrationApiMocks = vi.hoisted(() => ({
  getRegistrationSnapshot: vi.fn(),
  getRegistrationDeviceStates: vi.fn(),
  listenRegistrationSnapshot: vi.fn(),
  listenRegistrationDeviceStates: vi.fn(),
  listenRegistrationSubscriptions: vi.fn(),
  listenInteractionLogs: vi.fn(),
  registerAllDevicesCommand: vi.fn(),
  stopAllDeviceRegistrationCommand: vi.fn(),
}));

vi.mock('@/features/settings', () => settingsMocks);
vi.mock('@/features/simulator/device-api', () => deviceApiMocks);
vi.mock('@/features/simulator/registration-api', () => registrationApiMocks);

import DevicesPage from './devices-page.vue';
import GlobalSettingsPage from './global-settings-page.vue';
import { useSimulatorStore } from '@/features/simulator';

function findButtonByText(wrapper: ReturnType<typeof mount>, text: string) {
  const button = wrapper.findAll('button').find((item) => item.text() === text);
  if (button === undefined) {
    throw new Error(`未找到按钮：${text}`);
  }
  return button;
}

function deviceSnapshot(hasCompletedBatchAdd = false) {
  return {
    devices: [
      {
        id: '34020000001320000001',
        name: '模拟摄像机-001',
        type: '摄像机',
        manufacturer: 'GBLab',
        model: 'SIM-CAM-100',
        firmwareVersion: 'V1.0.0',
        channelCount: 2,
        registrationStatus: 'unregistered',
        createdAt: 1_777_777_777_000,
      },
      {
        id: '34020000001320000003',
        name: '园区球机-001',
        type: '球机',
        manufacturer: 'GBLab',
        model: 'SIM-PTZ-100',
        firmwareVersion: 'V1.0.0',
        channelCount: 1,
        registrationStatus: 'unregistered',
        createdAt: 1_777_777_777_000,
      },
    ],
    hasCompletedBatchAdd,
  };
}

function deviceChannels() {
  return [
    {
      id: '34020000001320001001',
      deviceId: '34020000001320000001',
      name: '模拟摄像机-001 · 通道 01',
      index: 1,
      platformSubscriptions: [],
    },
    {
      id: '34020000001320001002',
      deviceId: '34020000001320000001',
      name: '模拟摄像机-001 · 通道 02',
      index: 2,
      platformSubscriptions: [],
    },
    {
      id: '34020000001320003001',
      deviceId: '34020000001320000003',
      name: '园区球机-001 · 通道 01',
      index: 1,
      platformSubscriptions: [],
    },
  ];
}

describe('静态演示页面', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    document.body.innerHTML = '';
    settingsMocks.getSipServiceConfiguration.mockResolvedValue({
      uri: 'sip:192.168.1.100:5060',
      transport: 'UDP',
      platformId: '34020000002000000001',
      domain: '3402000000',
      password: '',
      localBindAddress: '0.0.0.0',
      advertisedAddress: '',
      localPort: 5_060,
      registerExpires: 3_600,
      keepaliveInterval: 60,
      signalCharset: 'GB2312',
    });
    settingsMocks.saveSipServiceConfiguration.mockImplementation(async (configuration) =>
      Promise.resolve(configuration),
    );
    deviceApiMocks.getDeviceSnapshot.mockResolvedValue(deviceSnapshot());
    deviceApiMocks.getDeviceChannels.mockImplementation(async (deviceId: string) =>
      deviceChannels().filter((channel) => channel.deviceId === deviceId),
    );
    deviceApiMocks.addDevicesInBatchCommand.mockResolvedValue(deviceSnapshot(true));
    registrationApiMocks.getRegistrationSnapshot.mockResolvedValue({
      operationStatus: 'idle',
      operationId: null,
      totalDevices: 0,
      registeredCount: 0,
      failedCount: 0,
      activeSubscriptions: 0,
      droppedLogs: 0,
    });
    registrationApiMocks.getRegistrationDeviceStates.mockResolvedValue([]);
    registrationApiMocks.listenRegistrationSnapshot.mockResolvedValue(() => undefined);
    registrationApiMocks.listenRegistrationDeviceStates.mockResolvedValue(() => undefined);
    registrationApiMocks.listenRegistrationSubscriptions.mockResolvedValue(() => undefined);
    registrationApiMocks.listenInteractionLogs.mockResolvedValue(() => undefined);
    registrationApiMocks.registerAllDevicesCommand.mockResolvedValue({
      operationId: '1',
      total: 2,
    });
    registrationApiMocks.stopAllDeviceRegistrationCommand.mockResolvedValue({
      operationId: '1',
      total: 2,
    });
  });

  it('设备管理页应按关键字筛选设备', async () => {
    const wrapper = mount(DevicesPage, { global: { plugins: [createPinia()] } });
    await flushPromises();

    await wrapper.get('input.n-input__input-el').setValue('园区球机');

    expect(wrapper.text()).toContain('园区球机-001');
    expect(wrapper.text()).not.toContain('模拟摄像机-001');
    expect(wrapper.find('.device-table-scroll').exists()).toBe(true);
    expect(wrapper.find('.device-pagination').exists()).toBe(true);
    expect(wrapper.findAll('.n-pagination')).toHaveLength(1);
  });

  it('点击批量添加设备后应展示批量表单', async () => {
    const wrapper = mount(DevicesPage, {
      attachTo: document.body,
      global: { plugins: [createPinia()] },
    });

    await findButtonByText(wrapper, '批量添加设备').trigger('click');
    await nextTick();

    expect(document.body.textContent).toContain('默认未注册');
  });

  it('完成一次批量添加后应禁用再次添加', async () => {
    const wrapper = mount(DevicesPage, { global: { plugins: [createPinia()] } });
    const store = useSimulatorStore();
    await flushPromises();
    const result = await store.addDevicesInBatch({
      count: 1,
      startDeviceId: '34020000001320000100',
      nameTemplate: '批量设备-{序号}',
      type: '摄像机',
      manufacturer: 'GBLab',
      model: 'SIM-CAM-100',
      firmwareVersion: 'V1.0.0',
      channelCount: 1,
    });

    await nextTick();

    expect(result).toEqual({ ok: true });
    expect(findButtonByText(wrapper, '设备已批量添加').attributes('disabled')).toBeDefined();
  });

  it('点击通道操作后应展示通道列表与平台订阅项', async () => {
    const wrapper = mount(DevicesPage, {
      attachTo: document.body,
      global: { plugins: [createPinia()] },
    });
    await flushPromises();

    await findButtonByText(wrapper, '通道').trigger('click');
    await flushPromises();

    expect(document.body.textContent).toContain('平台订阅');
    expect(document.body.textContent).toContain('未订阅');
  });

  it('应通过全量操作更新全部设备注册状态并记录日志', async () => {
    const wrapper = mount(DevicesPage, { global: { plugins: [createPinia()] } });
    const store = useSimulatorStore();
    await flushPromises();

    await findButtonByText(wrapper, '全量注册').trigger('click');
    await flushPromises();

    expect(store.devices.every((device) => device.registrationStatus === 'queued')).toBe(true);
    expect(store.interactionLogs).toHaveLength(0);

    await findButtonByText(wrapper, '全量停止注册').trigger('click');
    await nextTick();

    expect(store.registrationOperationStatus).toBe('stopping');
  });

  it('全局配置页应拆分平台与设备配置并通过桌面后端保存', async () => {
    const wrapper = mount(GlobalSettingsPage, { global: { plugins: [createPinia()] } });
    await flushPromises();
    const passwordInput = wrapper
      .findAll('input')
      .find((input) => input.attributes('maxlength') === '128');
    if (passwordInput === undefined) {
      throw new Error('未找到认证密码输入框');
    }

    await passwordInput.setValue('test-only-password');
    await findButtonByText(wrapper, '保存配置').trigger('click');
    await flushPromises();

    expect(wrapper.text()).toContain('认证密码');
    expect(wrapper.text()).toContain('平台配置');
    expect(wrapper.text()).toContain('设备配置');
    expect(passwordInput.attributes('type')).toBe('text');
    expect(settingsMocks.saveSipServiceConfiguration).toHaveBeenCalledWith(
      expect.objectContaining({ password: 'test-only-password' }),
    );

    const deviceTab = wrapper.findAll('.n-tabs-tab').find((tab) => tab.text().includes('设备配置'));
    if (deviceTab === undefined) {
      throw new Error('未找到设备配置页签');
    }
    await deviceTab.trigger('click');
    await nextTick();
    expect(wrapper.text()).toContain('信令字符集');
    expect(wrapper.text()).toContain('GB2312');
  });
});
