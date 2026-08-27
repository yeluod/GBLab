import { computed, ref } from 'vue';
import { defineStore } from 'pinia';

import {
  getSipServiceConfiguration,
  saveSipServiceConfiguration,
  type ConfigurationCommandError,
  type SipServiceConfig,
} from '@/features/settings';
import {
  addDevicesInBatchCommand,
  clearDevicesCommand,
  deleteDeviceCommand,
  getDeviceChannels,
  getDeviceSnapshot,
  updateDeviceCommand,
} from './device-api';
import {
  getRegistrationSnapshot,
  getRegistrationDeviceStates,
  listenInteractionLogs,
  listenRegistrationDeviceStates,
  listenRegistrationSubscriptions,
  listenRegistrationSnapshot,
  registerAllDevicesCommand,
  stopAllDeviceRegistrationCommand,
  triggerAlarmCommand,
  controlDeviceCommand,
  controlPtzCommand,
  triggerMobilePositionCommand,
} from './registration-api';

import type {
  BatchDeviceDraft,
  DeviceRegistrationSnapshot,
  DeviceUpdateDraft,
  DeviceSubscription,
  DeviceSnapshot,
  InteractionLog,
  OperationResult,
  RegistrationOperationStatus,
  RegistrationSnapshot,
  RegistrationStatus,
  SimulatedChannel,
  SimulatedDevice,
  SubscriptionSnapshot,
  SubscriptionKind,
} from './types';

const DEVICE_ID_PATTERN = /^\d{20}$/;
const MAX_BATCH_DEVICE_COUNT = 1_000;
const MAX_CHANNEL_COUNT = 128;
const MAX_INTERACTION_LOG_COUNT = 10_000;

function getConfigurationErrorMessage(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const commandError = error as ConfigurationCommandError;
    if (typeof commandError.message === 'string' && commandError.message.length > 0) {
      return commandError.message;
    }
  }
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return '桌面后端暂时不可用，请重试。';
}

function normalizeDeviceDraft(draft: DeviceUpdateDraft): DeviceUpdateDraft {
  return {
    ...draft,
    name: draft.name.trim(),
    manufacturer: draft.manufacturer.trim(),
    model: draft.model.trim(),
    firmwareVersion: draft.firmwareVersion.trim(),
  };
}

function validateDeviceDraft(draft: DeviceUpdateDraft): OperationResult {
  if (draft.name.length === 0) {
    return { ok: false, message: '请输入设备名称。' };
  }
  if (
    draft.manufacturer.length === 0 ||
    draft.model.length === 0 ||
    draft.firmwareVersion.length === 0
  ) {
    return { ok: false, message: '请填写制造商、设备型号和固件版本。' };
  }
  if (
    !Number.isInteger(draft.channelCount) ||
    draft.channelCount < 1 ||
    draft.channelCount > MAX_CHANNEL_COUNT
  ) {
    return { ok: false, message: `通道数量必须介于 1 到 ${MAX_CHANNEL_COUNT}。` };
  }
  return { ok: true };
}

function validateSipServiceConfig(config: SipServiceConfig): OperationResult {
  if (config.transport !== 'UDP') {
    return { ok: false, message: '当前真实 SIP 传输仅支持 UDP。' };
  }
  if (!['GB2312', 'GBK', 'UTF-8'].includes(config.signalCharset)) {
    return { ok: false, message: '请选择有效的信令字符集。' };
  }
  if (!config.uri.trim().startsWith('sip:')) {
    return { ok: false, message: 'SIP 地址必须以 sip: 开头。' };
  }
  if (!DEVICE_ID_PATTERN.test(config.platformId.trim())) {
    return { ok: false, message: '平台 ID 必须为 20 位数字。' };
  }
  if (config.password.length === 0 || config.password.length > 128) {
    return { ok: false, message: '密码不能为空且长度不能超过 128 个字符。' };
  }
  if (/\p{Cc}/u.test(config.password)) {
    return { ok: false, message: '密码不能包含控制字符。' };
  }
  if (
    config.domain.trim().length === 0 ||
    config.localBindAddress.trim().length === 0 ||
    !Number.isInteger(config.localPort) ||
    config.localPort < 1 ||
    config.localPort > 65_535 ||
    config.registerExpires <= 0 ||
    config.keepaliveInterval <= 0
  ) {
    return { ok: false, message: '请填写有效的服务配置。' };
  }

  return { ok: true };
}

function normalizeSipServiceConfig(config: SipServiceConfig): SipServiceConfig {
  return {
    ...config,
    uri: config.uri.trim(),
    platformId: config.platformId.trim(),
    domain: config.domain.trim(),
    localBindAddress: config.localBindAddress.trim(),
    advertisedAddress: config.advertisedAddress.trim(),
  };
}

/** 设备配置通过类型化 IPC 持久化；注册、订阅和日志仅保存在运行内存。 */
export const useSimulatorStore = defineStore('simulator', () => {
  const sipService = ref<SipServiceConfig>({
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
  const isSipServiceLoading = ref(false);
  const isSipServiceSaving = ref(false);
  const isDeviceLoading = ref(false);
  const isDeviceSaving = ref(false);
  const isRegistrationCommandPending = ref(false);
  const registrationOperationStatus = ref<RegistrationOperationStatus>('idle');
  const registrationStatusByDevice = ref(new Map<string, RegistrationStatus>());
  const registrationSnapshotByDevice = ref(new Map<string, DeviceRegistrationSnapshot>());
  const registrationErrorByDevice = ref(new Map<string, string>());
  const devices = ref<SimulatedDevice[]>([]);
  const subscriptions = ref<DeviceSubscription[]>([]);
  const subscriptionSnapshots = ref<SubscriptionSnapshot[]>([]);
  const channels = ref<SimulatedChannel[]>([]);
  const interactionLogs = ref<InteractionLog[]>([]);
  const hasCompletedBatchAdd = ref(false);

  const registeredDeviceCount = computed(
    () => devices.value.filter((device) => device.registrationStatus === 'registered').length,
  );
  const activeSubscriptionCount = computed(
    () => subscriptions.value.filter((subscription) => subscription.status === 'active').length,
  );
  const isRegistrationActive = computed(() => registrationOperationStatus.value !== 'idle');

  let registrationListenersPromise: Promise<void> | null = null;
  function mapInteractionLog(
    log: Omit<InteractionLog, 'id'> & { sequence: number },
  ): InteractionLog {
    return { ...log, id: `sip-${log.sequence}` };
  }

  function applyRegistrationSnapshot(snapshot: RegistrationSnapshot): void {
    registrationOperationStatus.value = snapshot.operationStatus;
  }

  function applyDeviceStates(states: DeviceRegistrationSnapshot[]): void {
    registrationStatusByDevice.value = new Map(
      states.map((device) => [device.deviceId, device.status]),
    );
    registrationSnapshotByDevice.value = new Map(states.map((device) => [device.deviceId, device]));
    registrationErrorByDevice.value = new Map(
      states.flatMap((device) =>
        device.lastError === null ? [] : [[device.deviceId, device.lastError] as const],
      ),
    );
    devices.value.forEach((device) => {
      const runtime = registrationSnapshotByDevice.value.get(device.id);
      device.registrationStatus = runtime?.status ?? 'unregistered';
      device.online = runtime?.online ?? false;
      device.lastHeartbeatAt = runtime?.lastHeartbeatAt ?? null;
      device.lastPlatformRequestAt = runtime?.lastPlatformRequestAt ?? null;
      device.heartbeatFailures = runtime?.heartbeatFailures ?? 0;
      device.lastControlAction = runtime?.lastControlAction ?? null;
      device.ptzAction = runtime?.ptzAction ?? null;
      device.guarded = runtime?.guarded ?? false;
      device.alarmActive = runtime?.alarmActive ?? false;
    });
  }

  function applySubscriptions(nextSubscriptions: SubscriptionSnapshot[]): void {
    subscriptionSnapshots.value = nextSubscriptions;
    subscriptions.value = nextSubscriptions
      .filter((subscription) =>
        ['catalog', 'alarm', 'mobilePosition'].includes(subscription.commandType),
      )
      .map((subscription) => ({
        id: `${subscription.deviceId}:${subscription.channelId ?? ''}:${subscription.commandType}`,
        deviceId: subscription.deviceId,
        kind:
          subscription.commandType === 'alarm'
            ? 'alarm'
            : subscription.commandType === 'mobilePosition'
              ? 'mobile-position'
              : 'catalog',
        status: subscription.status === 'active' ? 'active' : 'inactive',
        expiresAt:
          subscription.expiresAt === null ? null : new Date(subscription.expiresAt).toISOString(),
        lastNotifiedAt:
          subscription.lastNotifiedAt === null
            ? null
            : new Date(subscription.lastNotifiedAt).toISOString(),
        catalogPreview: [],
      }));
  }

  async function ensureRegistrationListeners(): Promise<void> {
    if (registrationListenersPromise !== null) {
      return registrationListenersPromise;
    }
    registrationListenersPromise = Promise.all([
      listenRegistrationSnapshot(applyRegistrationSnapshot),
      listenRegistrationDeviceStates(applyDeviceStates),
      listenRegistrationSubscriptions(applySubscriptions),
      listenInteractionLogs((logs) => appendInteractionLogs(logs.map(mapInteractionLog))),
    ]).then(() => undefined);
    return registrationListenersPromise;
  }

  function applyDeviceSnapshot(snapshot: DeviceSnapshot): void {
    devices.value = snapshot.devices.map((device) => ({
      ...device,
      registrationStatus: registrationStatusByDevice.value.get(device.id) ?? 'unregistered',
      online: registrationSnapshotByDevice.value.get(device.id)?.online ?? false,
      lastHeartbeatAt: registrationSnapshotByDevice.value.get(device.id)?.lastHeartbeatAt ?? null,
      lastPlatformRequestAt:
        registrationSnapshotByDevice.value.get(device.id)?.lastPlatformRequestAt ?? null,
      heartbeatFailures: registrationSnapshotByDevice.value.get(device.id)?.heartbeatFailures ?? 0,
      lastControlAction:
        registrationSnapshotByDevice.value.get(device.id)?.lastControlAction ?? null,
      ptzAction: registrationSnapshotByDevice.value.get(device.id)?.ptzAction ?? null,
      guarded: registrationSnapshotByDevice.value.get(device.id)?.guarded ?? false,
      alarmActive: registrationSnapshotByDevice.value.get(device.id)?.alarmActive ?? false,
    }));
    channels.value = [];
    // 空设备集合不应被一次性批量标记锁死，兼容旧版本删除全部设备后遗留的配置。
    hasCompletedBatchAdd.value = snapshot.hasCompletedBatchAdd && snapshot.devices.length > 0;
  }

  function applyDeviceChannels(derivedChannels: SimulatedChannel[]): void {
    channels.value = derivedChannels.map((channel) => {
      const platformSubscriptions = subscriptions.value
        .filter(
          (subscription) =>
            subscription.deviceId === channel.deviceId && subscription.status === 'active',
        )
        .map((subscription) => subscription.kind);
      return {
        ...channel,
        platformSubscriptions: [...new Set(platformSubscriptions)] as SubscriptionKind[],
      };
    });
  }

  function appendInteractionLogs(logs: InteractionLog[]): void {
    interactionLogs.value.push(...logs);
    if (interactionLogs.value.length > MAX_INTERACTION_LOG_COUNT) {
      interactionLogs.value.splice(0, interactionLogs.value.length - MAX_INTERACTION_LOG_COUNT);
    }
  }

  function clearInteractionLogs(): void {
    interactionLogs.value = [];
  }

  function updateSipService(config: SipServiceConfig): OperationResult {
    const normalized = normalizeSipServiceConfig(config);
    const validation = validateSipServiceConfig(normalized);
    if (!validation.ok) {
      return validation;
    }

    sipService.value = normalized;
    return { ok: true };
  }

  async function loadSipService(): Promise<OperationResult> {
    if (isSipServiceLoading.value) {
      return { ok: false, message: '全局配置正在加载。' };
    }

    isSipServiceLoading.value = true;
    try {
      await ensureRegistrationListeners();
      const [configuration, registrationSnapshot, deviceStates] = await Promise.all([
        getSipServiceConfiguration(),
        getRegistrationSnapshot(),
        getRegistrationDeviceStates(),
      ]);
      sipService.value = configuration;
      applyRegistrationSnapshot(registrationSnapshot);
      applyDeviceStates(deviceStates);
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    } finally {
      isSipServiceLoading.value = false;
    }
  }

  async function saveSipService(config: SipServiceConfig): Promise<OperationResult> {
    if (isRegistrationActive.value) {
      return { ok: false, message: '请先完成全量停止注册，再修改全局配置。' };
    }
    if (isSipServiceSaving.value) {
      return { ok: false, message: '全局配置正在保存。' };
    }
    const normalized = normalizeSipServiceConfig(config);
    const validation = validateSipServiceConfig(normalized);
    if (!validation.ok) {
      return validation;
    }

    isSipServiceSaving.value = true;
    try {
      sipService.value = await saveSipServiceConfiguration(normalized);
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    } finally {
      isSipServiceSaving.value = false;
    }
  }

  async function loadDevices(): Promise<OperationResult> {
    if (isDeviceLoading.value) {
      return { ok: false, message: '设备配置正在加载。' };
    }
    isDeviceLoading.value = true;
    try {
      await ensureRegistrationListeners();
      const [deviceSnapshot, registrationSnapshot, deviceStates] = await Promise.all([
        getDeviceSnapshot(),
        getRegistrationSnapshot(),
        getRegistrationDeviceStates(),
      ]);
      applyRegistrationSnapshot(registrationSnapshot);
      applyDeviceStates(deviceStates);
      applyDeviceSnapshot(deviceSnapshot);
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    } finally {
      isDeviceLoading.value = false;
    }
  }

  async function loadDeviceChannels(deviceId: string): Promise<OperationResult> {
    try {
      applyDeviceChannels(await getDeviceChannels(deviceId));
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    }
  }

  async function updateDevice(
    deviceId: string,
    draft: DeviceUpdateDraft,
  ): Promise<OperationResult> {
    if (isRegistrationActive.value) {
      return { ok: false, message: '请先完成全量停止注册，再修改设备。' };
    }
    const device = devices.value.find((item) => item.id === deviceId);
    if (device === undefined) {
      return { ok: false, message: '设备不存在或已被删除。' };
    }
    const normalizedDraft = normalizeDeviceDraft(draft);
    const validation = validateDeviceDraft(normalizedDraft);
    if (!validation.ok) {
      return validation;
    }

    if (isDeviceSaving.value) {
      return { ok: false, message: '设备配置正在保存。' };
    }
    isDeviceSaving.value = true;
    try {
      applyDeviceSnapshot(await updateDeviceCommand(deviceId, normalizedDraft));
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    } finally {
      isDeviceSaving.value = false;
    }
  }

  async function addDevicesInBatch(draft: BatchDeviceDraft): Promise<OperationResult> {
    if (isRegistrationActive.value) {
      return { ok: false, message: '请先完成全量停止注册，再添加设备。' };
    }
    if (hasCompletedBatchAdd.value) {
      return { ok: false, message: '设备仅允许批量添加一次。' };
    }
    if (!Number.isInteger(draft.count) || draft.count < 1 || draft.count > MAX_BATCH_DEVICE_COUNT) {
      return { ok: false, message: `设备数量必须介于 1 到 ${MAX_BATCH_DEVICE_COUNT}。` };
    }
    if (!DEVICE_ID_PATTERN.test(draft.startDeviceId.trim())) {
      return { ok: false, message: '起始设备 ID 必须为 20 位数字。' };
    }
    const normalizedDraft = {
      ...draft,
      startDeviceId: draft.startDeviceId.trim(),
      nameTemplate: draft.nameTemplate.trim(),
      manufacturer: draft.manufacturer.trim(),
      model: draft.model.trim(),
      firmwareVersion: draft.firmwareVersion.trim(),
    };
    if (normalizedDraft.nameTemplate.length === 0) {
      return { ok: false, message: '请输入设备名称模板。' };
    }
    if (
      normalizedDraft.manufacturer.length === 0 ||
      normalizedDraft.model.length === 0 ||
      normalizedDraft.firmwareVersion.length === 0
    ) {
      return { ok: false, message: '请填写制造商、设备型号和固件版本。' };
    }
    if (
      !Number.isInteger(normalizedDraft.channelCount) ||
      normalizedDraft.channelCount < 1 ||
      normalizedDraft.channelCount > MAX_CHANNEL_COUNT
    ) {
      return { ok: false, message: `通道数量必须介于 1 到 ${MAX_CHANNEL_COUNT}。` };
    }

    if (isDeviceSaving.value) {
      return { ok: false, message: '设备配置正在保存。' };
    }
    isDeviceSaving.value = true;
    try {
      applyDeviceSnapshot(await addDevicesInBatchCommand(normalizedDraft));
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    } finally {
      isDeviceSaving.value = false;
    }
  }

  async function clearDevices(): Promise<OperationResult> {
    if (isRegistrationActive.value) {
      return { ok: false, message: '请先完成全量停止注册，再清空设备。' };
    }
    if (devices.value.length === 0) {
      return { ok: false, message: '当前没有可清空的设备。' };
    }
    if (isDeviceSaving.value) {
      return { ok: false, message: '设备配置正在保存。' };
    }

    isDeviceSaving.value = true;
    try {
      applyDeviceSnapshot(await clearDevicesCommand());
      subscriptions.value = [];
      channels.value = [];
      registrationStatusByDevice.value = new Map();
      registrationErrorByDevice.value = new Map();
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    } finally {
      isDeviceSaving.value = false;
    }
  }

  async function deleteDevice(deviceId: string): Promise<OperationResult> {
    if (isRegistrationActive.value) {
      return { ok: false, message: '请先完成全量停止注册，再删除设备。' };
    }
    const deviceIndex = devices.value.findIndex((device) => device.id === deviceId);
    if (deviceIndex === -1) {
      return { ok: false, message: '设备不存在或已被删除。' };
    }

    if (isDeviceSaving.value) {
      return { ok: false, message: '设备配置正在保存。' };
    }
    isDeviceSaving.value = true;
    try {
      applyDeviceSnapshot(await deleteDeviceCommand(deviceId));
      subscriptions.value = subscriptions.value.filter(
        (subscription) => subscription.deviceId !== deviceId,
      );
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    } finally {
      isDeviceSaving.value = false;
    }
  }

  async function registerAllDevices(): Promise<OperationResult> {
    if (devices.value.length === 0) {
      return { ok: false, message: '当前没有可注册的设备。' };
    }
    if (isRegistrationCommandPending.value || isRegistrationActive.value) {
      return { ok: false, message: '全量注册生命周期已经在运行。' };
    }
    isRegistrationCommandPending.value = true;
    try {
      await ensureRegistrationListeners();
      await registerAllDevicesCommand();
      registrationOperationStatus.value = 'registering';
      devices.value.forEach((device) => {
        device.registrationStatus = 'queued';
      });
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    } finally {
      isRegistrationCommandPending.value = false;
    }
  }

  async function stopAllDeviceRegistration(): Promise<OperationResult> {
    if (devices.value.length === 0) {
      return { ok: false, message: '当前没有可停止注册的设备。' };
    }
    if (isRegistrationCommandPending.value) {
      return { ok: false, message: '注册操作正在提交，请稍后重试。' };
    }
    if (!isRegistrationActive.value) {
      return { ok: false, message: '当前没有运行中的注册生命周期。' };
    }
    isRegistrationCommandPending.value = true;
    try {
      await stopAllDeviceRegistrationCommand();
      registrationOperationStatus.value = 'stopping';
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    } finally {
      isRegistrationCommandPending.value = false;
    }
  }

  async function triggerAlarm(
    deviceId: string,
    channelId: string,
    alarmType = '1',
    description = '模拟报警',
  ): Promise<OperationResult> {
    if (
      !isRegistrationActive.value ||
      devices.value.find((device) => device.id === deviceId)?.registrationStatus !== 'registered'
    ) {
      return { ok: false, message: '设备尚未注册，无法触发报警。' };
    }
    try {
      await triggerAlarmCommand(deviceId, channelId, alarmType, description);
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    }
  }

  async function triggerMobilePosition(
    deviceId: string,
    channelId: string,
    longitude = 116.397,
    latitude = 39.908,
  ): Promise<OperationResult> {
    if (
      !isRegistrationActive.value ||
      devices.value.find((device) => device.id === deviceId)?.registrationStatus !== 'registered'
    ) {
      return { ok: false, message: '设备尚未注册，无法上报移动位置。' };
    }
    try {
      await triggerMobilePositionCommand(deviceId, channelId, longitude, latitude);
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    }
  }

  async function controlDevice(deviceId: string, action: string): Promise<OperationResult> {
    if (
      !isRegistrationActive.value ||
      devices.value.find((device) => device.id === deviceId)?.registrationStatus !== 'registered'
    ) {
      return { ok: false, message: '设备尚未注册，无法执行设备控制。' };
    }
    try {
      await controlDeviceCommand(deviceId, action);
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    }
  }

  async function controlPtz(
    deviceId: string,
    channelId: string,
    action: string,
  ): Promise<OperationResult> {
    if (
      !isRegistrationActive.value ||
      devices.value.find((device) => device.id === deviceId)?.registrationStatus !== 'registered'
    ) {
      return { ok: false, message: '设备尚未注册，无法执行 PTZ 控制。' };
    }
    try {
      await controlPtzCommand(deviceId, channelId, action);
      return { ok: true };
    } catch (error: unknown) {
      return { ok: false, message: getConfigurationErrorMessage(error) };
    }
  }

  return {
    sipService,
    isSipServiceLoading,
    isSipServiceSaving,
    isDeviceLoading,
    isDeviceSaving,
    isRegistrationCommandPending,
    registrationOperationStatus,
    isRegistrationActive,
    registrationErrorByDevice,
    devices,
    subscriptions,
    subscriptionSnapshots,
    channels,
    interactionLogs,
    clearInteractionLogs,
    hasCompletedBatchAdd,
    registeredDeviceCount,
    activeSubscriptionCount,
    updateSipService,
    loadSipService,
    saveSipService,
    loadDevices,
    loadDeviceChannels,
    updateDevice,
    addDevicesInBatch,
    clearDevices,
    deleteDevice,
    registerAllDevices,
    stopAllDeviceRegistration,
    triggerAlarm,
    triggerMobilePosition,
    controlDevice,
    controlPtz,
  };
});
