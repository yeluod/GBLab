<script setup lang="ts">
  import { computed, h, nextTick, onMounted, reactive, ref, watch } from 'vue';
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
    type DataTableInst,
  } from 'naive-ui';

  import {
    useSimulatorStore,
    type BatchDeviceDraft,
    type DeviceUpdateDraft,
    type InteractionLog,
    type SimulatedChannel,
    type SimulatedDevice,
  } from '@/features/simulator';

  const store = useSimulatorStore();
  const message = useMessage();
  const dialog = useDialog();
  const searchKeyword = ref('');
  const statusFilter = ref<'all' | 'registered' | 'unregistered'>('all');
  const devicePage = ref(1);
  const devicePageSize = ref(10);
  const selectedDeviceId = ref<string | null>(null);
  const isDetailOpen = ref(false);
  const isDeviceModalOpen = ref(false);
  const isBatchModalOpen = ref(false);
  const isChannelsModalOpen = ref(false);
  const editingDeviceId = ref<string | null>(null);
  const channelDeviceId = ref<string | null>(null);
  const interactionLogTableRef = ref<DataTableInst | null>(null);
  const deviceTypeOptions = ['摄像机', '球机', 'NVR', '门禁设备'].map((value) => ({
    label: value,
    value,
  }));
  const statusOptions = [
    { label: '全部状态', value: 'all' },
    { label: '已注册', value: 'registered' },
    { label: '未注册', value: 'unregistered' },
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
        statusFilter.value === 'all' ||
        (statusFilter.value === 'registered' && device.registrationStatus === 'registered') ||
        (statusFilter.value === 'unregistered' && device.registrationStatus === 'unregistered');
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

  async function scrollInteractionLogToLatest(): Promise<void> {
    await nextTick();
    const table = interactionLogTableRef.value;
    if (table !== null && typeof HTMLElement.prototype.scrollTo === 'function') {
      table.scrollTo({ top: Number.MAX_SAFE_INTEGER });
    }
  }

  watch(
    () => store.interactionLogs.length,
    () => {
      void scrollInteractionLogToLatest();
    },
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
    void scrollInteractionLogToLatest();
  });

  function formatCreatedAt(timestamp: number): string {
    return new Date(timestamp).toLocaleString('zh-CN', { hour12: false });
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

  function handleRegisterAllDevices(): void {
    const result = store.registerAllDevices();
    if (!result.ok) {
      message.error(result.message);
      return;
    }
    message.success(`已向 ${store.devices.length} 台设备发起注册。`);
  }

  function handleStopAllRegistration(): void {
    const result = store.stopAllDeviceRegistration();
    if (!result.ok) {
      message.error(result.message);
      return;
    }
    message.success(`已停止 ${store.devices.length} 台设备的注册。`);
  }

  async function openChannels(device: SimulatedDevice): Promise<void> {
    channelDeviceId.value = device.id;
    const result = await store.loadDeviceChannels(device.id);
    if (!result.ok) {
      message.error(result.message);
      return;
    }
    isChannelsModalOpen.value = true;
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
          {
            type: device.registrationStatus === 'registered' ? 'success' : 'default',
            size: 'small',
          },
          { default: () => (device.registrationStatus === 'registered' ? '已注册' : '未注册') },
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

  const interactionLogColumns: DataTableColumns<InteractionLog> = [
    { title: '时间', key: 'timestamp', width: 174, align: 'center' },
    { title: '设备 ID', key: 'deviceId', minWidth: 210, align: 'center' },
    { title: '通道 ID', key: 'channelId', minWidth: 250, align: 'center' },
    {
      title: '消息',
      key: 'message',
      minWidth: 480,
      render: (log) => h('code', { class: 'interaction-message' }, log.message),
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
              :disabled="store.hasCompletedBatchAdd || store.isDeviceLoading"
              :loading="store.isDeviceSaving"
              @click="isBatchModalOpen = true"
              >{{ store.hasCompletedBatchAdd ? '设备已批量添加' : '批量添加设备' }}</NButton
            >
            <NButton
              secondary
              type="primary"
              :disabled="
                store.devices.length === 0 || store.registeredDeviceCount === store.devices.length
              "
              @click="handleRegisterAllDevices"
              >全量注册</NButton
            >
            <NButton
              secondary
              type="warning"
              :disabled="store.devices.length === 0 || store.registeredDeviceCount === 0"
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

      <NCard class="data-surface interaction-log-surface" :bordered="false">
        <div class="section-card-header">
          <div>
            <h2>交互日志</h2>
            <p>展示模拟设备与平台之间发生的 SIP / GB28181 交互消息。</p>
          </div>
        </div>
        <div class="interaction-log-scroll">
          <NDataTable
            ref="interactionLogTableRef"
            class="interaction-log-table"
            flex-height
            :columns="interactionLogColumns"
            :data="store.interactionLogs"
            :pagination="false"
            :scroll-x="1120"
            :scrollbar-props="{ trigger: 'none', size: 10 }"
            :row-key="(log) => log.id"
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
              ><NTag
                :type="selectedDevice.registrationStatus === 'registered' ? 'success' : 'default'"
                >{{
                  selectedDevice.registrationStatus === 'registered' ? '已注册' : '未注册'
                }}</NTag
              ></dd
            ></div
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

    <NModal
      v-model:show="isChannelsModalOpen"
      preset="card"
      :title="channelDevice === null ? '通道列表' : `${channelDevice.name} · 通道列表`"
      style="width: min(920px, calc(100vw - 48px))"
    >
      <p v-if="channelDevice !== null" class="form-hint">
        {{ channelDevice.id }} ·
        {{ channelDevice.channelCount }} 路通道；通道由设备编号与通道数量实时生成，不写入配置文件。
      </p>
      <NDataTable
        :columns="[
          { title: '通道 ID', key: 'id', minWidth: 260, align: 'center' },
          { title: '通道名称', key: 'name', minWidth: 210, align: 'center' },
          { title: '序号', key: 'index', width: 82, align: 'center' },
          {
            title: '平台订阅项',
            key: 'platformSubscriptions',
            minWidth: 240,
            align: 'center',
            render: (channel: SimulatedChannel) =>
              channel.platformSubscriptions.length === 0
                ? h('span', { class: 'channel-subscription-empty' }, '未订阅')
                : h(
                    'div',
                    { class: 'channel-subscription-list' },
                    channel.platformSubscriptions.map((kind) =>
                      h(
                        NTag,
                        { type: 'info', size: 'small', bordered: false },
                        { default: () => getSubscriptionLabel(kind) },
                      ),
                    ),
                  ),
          },
        ]"
        :data="selectedChannels"
        :pagination="false"
        :row-key="(channel: SimulatedChannel) => channel.id"
      />
      <template #footer>
        <div class="modal-actions">
          <NButton @click="isChannelsModalOpen = false">关闭</NButton>
        </div>
      </template>
    </NModal>
  </section>
</template>
