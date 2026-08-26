<script setup lang="ts">
  import { computed, h, onMounted, reactive, ref, watch } from 'vue';
  import {
    NButton,
    NCard,
    NDataTable,
    NDrawer,
    NDrawerContent,
    NForm,
    NFormItem,
    NInput,
    NInputNumber,
    NModal,
    NPagination,
    NSelect,
    NTag,
    useDialog,
    useMessage,
    type DataTableColumns,
  } from 'naive-ui';

  import {
    useSimulatorStore,
    type BatchDeviceDraft,
    type DeviceUpdateDraft,
    type SimulatedChannel,
    type SimulatedDevice,
    type RegistrationStatus,
  } from '@/features/simulator';

  const store = useSimulatorStore();
  const message = useMessage();
  const dialog = useDialog();
  const searchKeyword = ref('');
  const statusFilter = ref<'all' | RegistrationStatus>('all');
  const devicePage = ref(1);
  const devicePageSize = ref(10);
  const selectedDeviceId = ref<string | null>(null);
  const isDetailOpen = ref(false);
  const isDeviceModalOpen = ref(false);
  const isBatchModalOpen = ref(false);
  const isChannelsDrawerOpen = ref(false);
  const isTriggerModalOpen = ref(false);
  const triggerKind = ref<'alarm' | 'mobile-position'>('alarm');
  const triggerChannelId = ref('');
  const alarmType = ref('1');
  const alarmDescription = ref('模拟报警');
  const longitude = ref(116.397);
  const latitude = ref(39.908);
  const editingDeviceId = ref<string | null>(null);
  const channelDeviceId = ref<string | null>(null);
  const deviceTypeOptions = ['摄像机', '球机', 'NVR', '门禁设备'].map((value) => ({
    label: value,
    value,
  }));
  const statusOptions = [
    { label: '全部状态', value: 'all' },
    { label: '已注册', value: 'registered' },
    { label: '未注册', value: 'unregistered' },
    { label: '排队中', value: 'queued' },
    { label: '注册中', value: 'registering' },
    { label: '注销中', value: 'unregistering' },
    { label: '失败', value: 'failed' },
  ];
  const devicePageSizeOptions = [10, 20, 50, 100];
  const deviceDraft = reactive<DeviceUpdateDraft>({
    name: '',
    type: '摄像机',
    manufacturer: '',
    model: '',
    firmwareVersion: '',
    channelCount: 1,
  });
  const batchDraft = reactive<BatchDeviceDraft>({
    count: 10,
    startDeviceId: '34020000001320000100',
    nameTemplate: '模拟摄像机-{序号}',
    type: '摄像机',
    manufacturer: 'GBLab',
    model: 'SIM-CAM-100',
    firmwareVersion: 'V1.0.0',
    channelCount: 1,
  });

  const filteredDevices = computed(() => {
    const keyword = searchKeyword.value.trim().toLowerCase();
    return store.devices.filter((device) => {
      const matchesKeyword =
        keyword.length === 0 || `${device.id}${device.name}`.toLowerCase().includes(keyword);
      const matchesStatus =
        statusFilter.value === 'all' || device.registrationStatus === statusFilter.value;
      return matchesKeyword && matchesStatus;
    });
  });
  const selectedDevice = computed(() =>
    selectedDeviceId.value === null
      ? null
      : (store.devices.find((device) => device.id === selectedDeviceId.value) ?? null),
  );
  const pagedDevices = computed(() => {
    const firstIndex = (devicePage.value - 1) * devicePageSize.value;
    return filteredDevices.value.slice(firstIndex, firstIndex + devicePageSize.value);
  });
  const channelDevice = computed(() =>
    channelDeviceId.value === null
      ? null
      : (store.devices.find((device) => device.id === channelDeviceId.value) ?? null),
  );
  const selectedChannels = computed(() =>
    channelDevice.value === null
      ? []
      : store.channels.filter((channel) => channel.deviceId === channelDevice.value?.id),
  );

  watch([searchKeyword, statusFilter, devicePageSize], () => {
    devicePage.value = 1;
  });

  watch(
    () => filteredDevices.value.length,
    (itemCount) => {
      const lastPage = Math.max(1, Math.ceil(itemCount / devicePageSize.value));
      if (devicePage.value > lastPage) {
        devicePage.value = lastPage;
      }
    },
  );

  onMounted(async () => {
    const result = await store.loadDevices();
    if (!result.ok) {
      message.error(result.message);
    }
  });

  function formatCreatedAt(timestamp: number): string {
    return new Date(timestamp).toLocaleString('zh-CN', { hour12: false });
  }

  function registrationStatusMeta(status: RegistrationStatus): {
    label: string;
    type: 'default' | 'success' | 'warning' | 'error' | 'info';
  } {
    switch (status) {
      case 'queued':
        return { label: '排队中', type: 'info' };
      case 'registering':
        return { label: '注册中', type: 'warning' };
      case 'registered':
        return { label: '已注册', type: 'success' };
      case 'unregistering':
        return { label: '注销中', type: 'warning' };
      case 'failed':
        return { label: '失败', type: 'error' };
      case 'unregistered':
        return { label: '未注册', type: 'default' };
    }
  }

  function selectDevice(device: SimulatedDevice): void {
    selectedDeviceId.value = device.id;
    isDetailOpen.value = true;
  }

  function openEditDevice(device: SimulatedDevice): void {
    editingDeviceId.value = device.id;
    Object.assign(deviceDraft, {
      name: device.name,
      type: device.type,
      manufacturer: device.manufacturer,
      model: device.model,
      firmwareVersion: device.firmwareVersion,
      channelCount: device.channelCount,
    });
    isDeviceModalOpen.value = true;
  }

  async function handleSaveDevice(): Promise<void> {
    if (editingDeviceId.value === null) {
      return;
    }
    const result = await store.updateDevice(editingDeviceId.value, { ...deviceDraft });
    if (!result.ok) {
      message.error(result.message);
      return;
    }
    isDeviceModalOpen.value = false;
    message.success('设备已更新。');
  }

  async function handleBatchCreate(): Promise<void> {
    const result = await store.addDevicesInBatch({ ...batchDraft });
    if (!result.ok) {
      message.error(result.message);
      return;
    }
    isBatchModalOpen.value = false;
    message.success(`已新增 ${batchDraft.count} 台模拟设备。`);
  }

  async function handleRegisterAllDevices(): Promise<void> {
    const result = await store.registerAllDevices();
    if (!result.ok) {
      message.error(result.message);
      return;
    }
    message.success(`已向 ${store.devices.length} 台设备发起注册。`);
  }

  async function handleStopAllRegistration(): Promise<void> {
    const result = await store.stopAllDeviceRegistration();
    if (!result.ok) {
      message.error(result.message);
      return;
    }
    message.success(`已向 ${store.devices.length} 台设备发起停止注册。`);
  }

  async function openChannels(device: SimulatedDevice): Promise<void> {
    channelDeviceId.value = device.id;
    const result = await store.loadDeviceChannels(device.id);
    if (!result.ok) {
      message.error(result.message);
      return;
    }
    isChannelsDrawerOpen.value = true;
  }

  function openAlarmTrigger(channel: SimulatedChannel): void {
    triggerKind.value = 'alarm';
    triggerChannelId.value = channel.id;
    isTriggerModalOpen.value = true;
  }

  function openMobilePositionTrigger(channel: SimulatedChannel): void {
    triggerKind.value = 'mobile-position';
    triggerChannelId.value = channel.id;
    isTriggerModalOpen.value = true;
  }

  async function submitTrigger(): Promise<void> {
    if (channelDevice.value === null) return;
    const result =
      triggerKind.value === 'alarm'
        ? await store.triggerAlarm(
            channelDevice.value.id,
            triggerChannelId.value,
            alarmType.value,
            alarmDescription.value,
          )
        : await store.triggerMobilePosition(
            channelDevice.value.id,
            triggerChannelId.value,
            longitude.value,
            latitude.value,
          );
    if (result.ok)
      message.success(triggerKind.value === 'alarm' ? '报警已发送。' : '移动位置已发送。');
    if (!result.ok) message.error(result.message);
    else isTriggerModalOpen.value = false;
  }

  function getSubscriptionLabel(kind: SimulatedChannel['platformSubscriptions'][number]): string {
    return kind === 'catalog' ? '目录 Catalog' : kind === 'alarm' ? '报警 Alarm' : '移动位置';
  }

  function confirmDeleteDevice(device: SimulatedDevice): void {
    dialog.warning({
      title: '删除设备',
      content: `确定删除“${device.name}”吗？其派生通道将同时消失。`,
      positiveText: '删除',
      negativeText: '取消',
      onPositiveClick: async () => {
        const result = await store.deleteDevice(device.id);
        if (!result.ok) {
          message.error(result.message);
          return;
        }
        if (selectedDeviceId.value === device.id) {
          selectedDeviceId.value = null;
          isDetailOpen.value = false;
        }
        message.success('设备已删除。');
      },
    });
  }

  function confirmClearDevices(): void {
    dialog.error({
      title: '清空设备',
      content:
        '确定清空全部设备配置吗？设备和派生通道将被删除，清空后可以重新批量添加设备。交互日志会保留。',
      positiveText: '清空设备',
      negativeText: '取消',
      onPositiveClick: async () => {
        const result = await store.clearDevices();
        if (!result.ok) {
          message.error(result.message);
          return;
        }
        searchKeyword.value = '';
        statusFilter.value = 'all';
        devicePage.value = 1;
        selectedDeviceId.value = null;
        channelDeviceId.value = null;
        isDetailOpen.value = false;
        isChannelsDrawerOpen.value = false;
        message.success('设备配置已清空，可以重新批量添加。');
      },
    });
  }

  const columns: DataTableColumns<SimulatedDevice> = [
    { title: '设备 ID', key: 'id', minWidth: 210, align: 'center' },
    { title: '名称', key: 'name', minWidth: 160, align: 'center' },
    { title: '类型', key: 'type', width: 100, align: 'center' },
    {
      title: '通道数',
      key: 'channelCount',
      width: 88,
      align: 'center',
      render: (device) => `${device.channelCount} 路`,
    },
    {
      title: '注册状态',
      key: 'registrationStatus',
      width: 104,
      align: 'center',
      render: (device) =>
        h(
          NTag,
          { type: registrationStatusMeta(device.registrationStatus).type, size: 'small' },
          {
            default: () => registrationStatusMeta(device.registrationStatus).label,
          },
        ),
    },
    {
      title: '创建时间',
      key: 'createdAt',
      minWidth: 164,
      align: 'center',
      render: (device) => formatCreatedAt(device.createdAt),
    },
    {
      title: '操作',
      key: 'actions',
      width: 190,
      align: 'center',
      render: (device) =>
        h('div', { class: 'table-actions' }, [
          h(
            NButton,
            {
              size: 'small',
              tertiary: true,
              type: 'primary',
              onClick: (event: MouseEvent) => {
                event.stopPropagation();
                void openChannels(device);
              },
            },
            { default: () => '通道' },
          ),
          h(
            NButton,
            {
              size: 'small',
              tertiary: true,
              disabled: store.isRegistrationActive,
              onClick: (event: MouseEvent) => {
                event.stopPropagation();
                openEditDevice(device);
              },
            },
            { default: () => '编辑' },
          ),
          h(
            NButton,
            {
              size: 'small',
              tertiary: true,
              type: 'error',
              disabled: store.isRegistrationActive,
              onClick: (event: MouseEvent) => {
                event.stopPropagation();
                confirmDeleteDevice(device);
              },
            },
            { default: () => '删除' },
          ),
        ]),
    },
  ];
</script>

<template>
  <section class="page-shell device-page" aria-labelledby="devices-title">
    <header class="page-header compact-header">
      <div>
        <p class="eyebrow">DEVICE FLEET</p>
        <h1 id="devices-title">设备管理</h1>
        <p>所有设备共享 {{ store.sipService.uri }}，批量新增设备默认未注册。</p>
      </div>
      <span class="page-count"
        >{{ store.devices.length }} 台设备 · {{ store.registeredDeviceCount }} 已注册</span
      >
    </header>

    <div class="device-workspace">
      <NCard class="data-surface device-list-surface" :bordered="false">
        <div class="toolbar-row">
          <div class="toolbar-actions">
            <NButton
              type="primary"
              :disabled="
                store.hasCompletedBatchAdd || store.isDeviceLoading || store.isRegistrationActive
              "
              :loading="store.isDeviceSaving"
              @click="isBatchModalOpen = true"
              >{{ store.hasCompletedBatchAdd ? '设备已批量添加' : '批量添加设备' }}</NButton
            >
            <NButton
              secondary
              type="error"
              :disabled="
                store.devices.length === 0 ||
                store.isDeviceLoading ||
                store.isDeviceSaving ||
                store.isRegistrationActive
              "
              @click="confirmClearDevices"
              >清空设备</NButton
            >
            <NButton
              secondary
              type="primary"
              :disabled="
                store.devices.length === 0 ||
                store.isRegistrationActive ||
                store.isRegistrationCommandPending
              "
              :loading="store.isRegistrationCommandPending && !store.isRegistrationActive"
              @click="handleRegisterAllDevices"
              >全量注册</NButton
            >
            <NButton
              secondary
              type="warning"
              :disabled="
                store.devices.length === 0 ||
                !store.isRegistrationActive ||
                store.registrationOperationStatus === 'stopping'
              "
              :loading="store.registrationOperationStatus === 'stopping'"
              @click="handleStopAllRegistration"
              >全量停止注册</NButton
            >
          </div>
          <div class="toolbar-filters">
            <NInput v-model:value="searchKeyword" clearable placeholder="搜索设备 ID 或名称" />
            <NSelect v-model:value="statusFilter" :options="statusOptions" />
          </div>
        </div>

        <div class="device-table-scroll">
          <NDataTable
            flex-height
            :columns="columns"
            :data="pagedDevices"
            :pagination="false"
            :scroll-x="1300"
            :scrollbar-props="{ trigger: 'none', size: 10 }"
            :row-key="(device) => device.id"
            :row-props="
              (device) => ({ onClick: () => selectDevice(device), style: 'cursor: pointer' })
            "
          />
        </div>
        <div class="device-pagination">
          <NPagination
            v-model:page="devicePage"
            v-model:page-size="devicePageSize"
            :item-count="filteredDevices.length"
            :page-sizes="devicePageSizeOptions"
            show-quick-jumper
            show-size-picker
          />
        </div>
      </NCard>
    </div>

    <NDrawer v-model:show="isDetailOpen" :width="420" placement="right">
      <NDrawerContent v-if="selectedDevice !== null" :title="selectedDevice.name" closable>
        <dl class="detail-list">
          <div
            ><dt>设备 ID</dt><dd>{{ selectedDevice.id }}</dd></div
          >
          <div
            ><dt>设备类型</dt><dd>{{ selectedDevice.type }}</dd></div
          >
          <div
            ><dt>制造商</dt><dd>{{ selectedDevice.manufacturer }}</dd></div
          >
          <div
            ><dt>设备型号</dt><dd>{{ selectedDevice.model }}</dd></div
          >
          <div
            ><dt>固件版本</dt><dd>{{ selectedDevice.firmwareVersion }}</dd></div
          >
          <div
            ><dt>通道数量</dt><dd>{{ selectedDevice.channelCount }} 路</dd></div
          >
          <div
            ><dt>注册状态</dt
            ><dd
              ><NTag :type="registrationStatusMeta(selectedDevice.registrationStatus).type">{{
                registrationStatusMeta(selectedDevice.registrationStatus).label
              }}</NTag></dd
            ></div
          >
          <div v-if="store.registrationErrorByDevice.has(selectedDevice.id)"
            ><dt>最近错误</dt
            ><dd>{{ store.registrationErrorByDevice.get(selectedDevice.id) }}</dd></div
          >
          <div
            ><dt>共享服务</dt><dd>{{ store.sipService.uri }}</dd></div
          >
        </dl>
      </NDrawerContent>
    </NDrawer>

    <NModal v-model:show="isDeviceModalOpen" preset="card" title="编辑设备" style="width: 560px">
      <NForm label-placement="top">
        <NFormItem label="设备名称">
          <NInput v-model:value="deviceDraft.name" />
        </NFormItem>
        <NFormItem label="设备类型">
          <NSelect v-model:value="deviceDraft.type" :options="deviceTypeOptions" />
        </NFormItem>
        <div class="form-grid">
          <NFormItem label="制造商">
            <NInput v-model:value="deviceDraft.manufacturer" placeholder="例如：海康威视" />
          </NFormItem>
          <NFormItem label="设备型号">
            <NInput v-model:value="deviceDraft.model" placeholder="例如：DS-2CD2146G2-I" />
          </NFormItem>
        </div>
        <div class="form-grid">
          <NFormItem label="固件版本">
            <NInput v-model:value="deviceDraft.firmwareVersion" placeholder="例如：V5.7.11" />
          </NFormItem>
          <NFormItem label="通道数量（1-128）">
            <NInputNumber
              v-model:value="deviceDraft.channelCount"
              :min="1"
              :max="128"
              :show-button="false"
            />
          </NFormItem>
        </div>
      </NForm>
      <template #footer>
        <div class="modal-actions">
          <NButton @click="isDeviceModalOpen = false">取消</NButton>
          <NButton type="primary" :loading="store.isDeviceSaving" @click="handleSaveDevice"
            >保存</NButton
          >
        </div>
      </template>
    </NModal>

    <NModal v-model:show="isBatchModalOpen" preset="card" title="批量添加设备" style="width: 620px">
      <NForm label-placement="top">
        <div class="form-grid">
          <NFormItem label="设备数量（1-1000）">
            <NInputNumber
              v-model:value="batchDraft.count"
              :min="1"
              :max="1000"
              :show-button="false"
            />
          </NFormItem>
          <NFormItem label="设备类型">
            <NSelect v-model:value="batchDraft.type" :options="deviceTypeOptions" />
          </NFormItem>
        </div>
        <NFormItem label="起始设备 ID">
          <NInput v-model:value="batchDraft.startDeviceId" placeholder="20 位数字" />
        </NFormItem>
        <NFormItem label="设备名称模板">
          <NInput v-model:value="batchDraft.nameTemplate" placeholder="例如：模拟摄像机-{序号}" />
        </NFormItem>
        <div class="form-grid">
          <NFormItem label="制造商">
            <NInput v-model:value="batchDraft.manufacturer" />
          </NFormItem>
          <NFormItem label="设备型号">
            <NInput v-model:value="batchDraft.model" />
          </NFormItem>
        </div>
        <div class="form-grid">
          <NFormItem label="固件版本">
            <NInput v-model:value="batchDraft.firmwareVersion" />
          </NFormItem>
          <NFormItem label="每台通道数量（1-128）">
            <NInputNumber
              v-model:value="batchDraft.channelCount"
              :min="1"
              :max="128"
              :show-button="false"
            />
          </NFormItem>
        </div>
      </NForm>
      <p class="form-hint">
        设备仅允许批量添加一次，新增设备默认未注册；完成后可从列表执行全量注册。
      </p>
      <template #footer>
        <div class="modal-actions">
          <NButton @click="isBatchModalOpen = false">取消</NButton>
          <NButton type="primary" :loading="store.isDeviceSaving" @click="handleBatchCreate"
            >创建 {{ batchDraft.count }} 台设备</NButton
          >
        </div>
      </template>
    </NModal>

    <NDrawer v-model:show="isChannelsDrawerOpen" placement="right" :width="560">
      <NDrawerContent
        v-if="channelDevice !== null"
        :title="`${channelDevice.name} · 通道`"
        closable
      >
        <p class="form-hint">
          {{ channelDevice.id }} · {{ channelDevice.channelCount }} 路通道；通道按规则实时生成。
        </p>
        <div class="channel-card-list">
          <NCard
            v-for="channel in selectedChannels"
            :key="channel.id"
            class="channel-card"
            :bordered="false"
          >
            <div class="channel-card-header">
              <div>
                <strong>{{ channel.name }}</strong>
                <span>{{ channel.id }}</span>
              </div>
              <NTag
                size="small"
                :type="channelDevice.registrationStatus === 'registered' ? 'success' : 'default'"
              >
                {{ channelDevice.registrationStatus === 'registered' ? '可触发' : '未注册' }}
              </NTag>
            </div>
            <div class="channel-card-subscriptions">
              <span>平台订阅</span>
              <template v-if="channel.platformSubscriptions.length">
                <NTag
                  v-for="kind in channel.platformSubscriptions"
                  :key="kind"
                  size="small"
                  type="info"
                  :bordered="false"
                  >{{ getSubscriptionLabel(kind) }}</NTag
                >
              </template>
              <span v-else class="channel-subscription-empty">未订阅</span>
            </div>
            <div class="channel-card-actions">
              <NButton
                size="small"
                type="warning"
                secondary
                :disabled="channelDevice.registrationStatus !== 'registered'"
                @click="openAlarmTrigger(channel)"
                >报警模拟</NButton
              >
              <NButton
                size="small"
                type="info"
                secondary
                :disabled="channelDevice.registrationStatus !== 'registered'"
                @click="openMobilePositionTrigger(channel)"
                >移动位置</NButton
              >
            </div>
          </NCard>
        </div>
      </NDrawerContent>
    </NDrawer>

    <NModal
      v-model:show="isTriggerModalOpen"
      preset="card"
      :title="triggerKind === 'alarm' ? '报警模拟' : '移动位置上报'"
      style="width: min(520px, calc(100vw - 48px))"
    >
      <p class="form-hint">通道 ID：{{ triggerChannelId }}</p>
      <NForm label-placement="top">
        <template v-if="triggerKind === 'alarm'">
          <NFormItem label="报警类型">
            <NInput v-model:value="alarmType" />
          </NFormItem>
          <NFormItem label="报警描述">
            <NInput v-model:value="alarmDescription" />
          </NFormItem>
        </template>
        <template v-else>
          <div class="form-grid">
            <NFormItem label="经度">
              <NInputNumber v-model:value="longitude" :show-button="false" />
            </NFormItem>
            <NFormItem label="纬度">
              <NInputNumber v-model:value="latitude" :show-button="false" />
            </NFormItem>
          </div>
        </template>
      </NForm>
      <template #footer>
        <div class="modal-actions">
          <NButton @click="isTriggerModalOpen = false">取消</NButton>
          <NButton type="primary" @click="submitTrigger">发送</NButton>
        </div>
      </template>
    </NModal>
  </section>
</template>
