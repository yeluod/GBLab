<script setup lang="ts">
  import { computed, h, reactive, ref } from 'vue';
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
    NSelect,
    NSwitch,
    NTag,
    useDialog,
    useMessage,
    type DataTableColumns,
  } from 'naive-ui';

  import {
    useSimulatorStore,
    type BatchDeviceDraft,
    type DeviceDraft,
    type DeviceType,
    type SimulatedDevice,
  } from '@/features/simulator';

  const store = useSimulatorStore();
  const message = useMessage();
  const dialog = useDialog();
  const searchKeyword = ref('');
  const statusFilter = ref<'all' | 'enabled' | 'disabled'>('all');
  const selectedDeviceId = ref<string | null>(null);
  const isDetailOpen = ref(false);
  const isDeviceModalOpen = ref(false);
  const isBatchModalOpen = ref(false);
  const editingDeviceId = ref<string | null>(null);
  const deviceTypeOptions = ['摄像机', '球机', 'NVR', '门禁设备'].map((value) => ({ label: value, value }));
  const statusOptions = [
    { label: '全部状态', value: 'all' },
    { label: '已启用', value: 'enabled' },
    { label: '未启用', value: 'disabled' },
  ];
  const deviceDraft = reactive<DeviceDraft>({
    id: '',
    name: '',
    type: '摄像机',
    isEnabled: false,
  });
  const batchDraft = reactive<BatchDeviceDraft>({
    count: 10,
    startDeviceId: '34020000001320000100',
    nameTemplate: '模拟摄像机-{序号}',
    type: '摄像机',
    isEnabled: false,
  });

  const filteredDevices = computed(() => {
    const keyword = searchKeyword.value.trim().toLowerCase();
    return store.devices.filter((device) => {
      const matchesKeyword = keyword.length === 0 || `${device.id}${device.name}`.toLowerCase().includes(keyword);
      const matchesStatus =
        statusFilter.value === 'all' ||
        (statusFilter.value === 'enabled' && device.isEnabled) ||
        (statusFilter.value === 'disabled' && !device.isEnabled);
      return matchesKeyword && matchesStatus;
    });
  });
  const selectedDevice = computed(() =>
    selectedDeviceId.value === null
      ? null
      : store.devices.find((device) => device.id === selectedDeviceId.value) ?? null,
  );
  const selectedSubscriptions = computed(() =>
    selectedDevice.value === null
      ? []
      : store.subscriptions.filter((subscription) => subscription.deviceId === selectedDevice.value?.id),
  );

  function selectDevice(device: SimulatedDevice): void {
    selectedDeviceId.value = device.id;
    isDetailOpen.value = true;
  }

  function resetDeviceDraft(): void {
    Object.assign(deviceDraft, { id: '', name: '', type: '摄像机', isEnabled: false });
  }

  function openCreateDevice(): void {
    editingDeviceId.value = null;
    resetDeviceDraft();
    isDeviceModalOpen.value = true;
  }

  function openEditDevice(device: SimulatedDevice): void {
    editingDeviceId.value = device.id;
    Object.assign(deviceDraft, device);
    isDeviceModalOpen.value = true;
  }

  function handleSaveDevice(): void {
    const result =
      editingDeviceId.value === null
        ? store.addDevice({ ...deviceDraft })
        : store.updateDevice(editingDeviceId.value, {
            name: deviceDraft.name,
            type: deviceDraft.type,
            isEnabled: deviceDraft.isEnabled,
          });
    if (!result.ok) {
      message.error(result.message);
      return;
    }
    isDeviceModalOpen.value = false;
    message.success(editingDeviceId.value === null ? '设备已新增，默认可保持未启用。' : '设备已更新。');
  }

  function handleBatchCreate(): void {
    const result = store.addDevicesInBatch({ ...batchDraft });
    if (!result.ok) {
      message.error(result.message);
      return;
    }
    isBatchModalOpen.value = false;
    message.success(`已新增 ${batchDraft.count} 台模拟设备。`);
  }

  function handleToggleDevice(device: SimulatedDevice): void {
    const result = store.toggleDevice(device.id);
    if (!result.ok) {
      message.error(result.message);
      return;
    }
    message.success(device.isEnabled ? '设备已停用。' : '设备已启用。');
  }

  function confirmDeleteDevice(device: SimulatedDevice): void {
    dialog.warning({
      title: '删除设备',
      content: `确定删除“${device.name}”及其订阅记录吗？`,
      positiveText: '删除',
      negativeText: '取消',
      onPositiveClick: () => {
        const result = store.deleteDevice(device.id);
        if (!result.ok) {
          message.error(result.message);
          return;
        }
        if (selectedDeviceId.value === device.id) {
          selectedDeviceId.value = null;
          isDetailOpen.value = false;
        }
        message.success('设备及其订阅记录已删除。');
      },
    });
  }

  const columns: DataTableColumns<SimulatedDevice> = [
    { title: '设备 ID', key: 'id', minWidth: 210 },
    { title: '名称', key: 'name', minWidth: 160 },
    { title: '类型', key: 'type', width: 100 },
    {
      title: '启用状态',
      key: 'isEnabled',
      width: 104,
      render: (device) => h(NTag, { type: device.isEnabled ? 'success' : 'default', size: 'small' }, { default: () => (device.isEnabled ? '已启用' : '未启用') }),
    },
    {
      title: '服务订阅',
      key: 'subscriptions',
      width: 100,
      render: (device) => `${store.subscriptions.filter((subscription) => subscription.deviceId === device.id).length} 项`,
    },
    { title: '创建时间', key: 'createdAt', minWidth: 164 },
    {
      title: '操作',
      key: 'actions',
      width: 210,
      render: (device) =>
        h('div', { class: 'table-actions' }, [
          h(
            NButton,
            { size: 'small', tertiary: true, onClick: (event: MouseEvent) => { event.stopPropagation(); handleToggleDevice(device); } },
            { default: () => (device.isEnabled ? '停用' : '启用') },
          ),
          h(
            NButton,
            { size: 'small', tertiary: true, onClick: (event: MouseEvent) => { event.stopPropagation(); openEditDevice(device); } },
            { default: () => '编辑' },
          ),
          h(
            NButton,
            { size: 'small', tertiary: true, type: 'error', onClick: (event: MouseEvent) => { event.stopPropagation(); confirmDeleteDevice(device); } },
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
        <p>所有设备共享 {{ store.sipService.uri }}，新增设备默认不启用。</p>
      </div>
      <span class="page-count">{{ store.devices.length }} 台设备 · {{ store.enabledDeviceCount }} 已启用</span>
    </header>

    <NCard class="data-surface" :bordered="false">
      <div class="toolbar-row">
        <div class="toolbar-actions">
          <NButton type="primary" @click="isBatchModalOpen = true">批量添加设备</NButton>
          <NButton secondary @click="openCreateDevice">新增设备</NButton>
        </div>
        <div class="toolbar-filters">
          <NInput v-model:value="searchKeyword" clearable placeholder="搜索设备 ID 或名称" />
          <NSelect v-model:value="statusFilter" :options="statusOptions" />
        </div>
      </div>

      <NDataTable
        :columns="columns"
        :data="filteredDevices"
        :pagination="{ pageSize: 10 }"
        :row-key="(device) => device.id"
        :row-props="(device) => ({ onClick: () => selectDevice(device), style: 'cursor: pointer' })"
      />
    </NCard>

    <NDrawer v-model:show="isDetailOpen" :width="420" placement="right">
      <NDrawerContent v-if="selectedDevice !== null" :title="selectedDevice.name" closable>
        <dl class="detail-list">
          <div><dt>设备 ID</dt><dd>{{ selectedDevice.id }}</dd></div>
          <div><dt>设备类型</dt><dd>{{ selectedDevice.type }}</dd></div>
          <div><dt>启用状态</dt><dd><NTag :type="selectedDevice.isEnabled ? 'success' : 'default'">{{ selectedDevice.isEnabled ? '已启用' : '未启用' }}</NTag></dd></div>
          <div><dt>共享服务</dt><dd>{{ store.sipService.uri }}</dd></div>
        </dl>

        <section class="drawer-section">
          <h2>服务订阅</h2>
          <div v-if="selectedSubscriptions.length === 0" class="empty-state">当前设备没有订阅记录。</div>
          <article v-for="subscription in selectedSubscriptions" :key="subscription.id" class="subscription-summary">
            <div>
              <strong>{{ subscription.kind === 'catalog' ? '目录 Catalog' : subscription.kind === 'alarm' ? '报警 Alarm' : '移动位置' }}</strong>
              <span>{{ subscription.status === 'active' ? '已订阅' : '未订阅' }}</span>
            </div>
            <p>到期：{{ subscription.expiresAt ?? '—' }}</p>
            <ul v-if="subscription.catalogPreview.length > 0">
              <li v-for="item in subscription.catalogPreview" :key="item">{{ item }}</li>
            </ul>
          </article>
        </section>
      </NDrawerContent>
    </NDrawer>

    <NModal v-model:show="isDeviceModalOpen" preset="card" :title="editingDeviceId === null ? '新增设备' : '编辑设备'" style="width: 560px">
      <NForm label-placement="top">
        <NFormItem label="设备 ID">
          <NInput v-model:value="deviceDraft.id" :disabled="editingDeviceId !== null" placeholder="20 位数字" />
        </NFormItem>
        <NFormItem label="设备名称">
          <NInput v-model:value="deviceDraft.name" />
        </NFormItem>
        <NFormItem label="设备类型">
          <NSelect v-model:value="deviceDraft.type" :options="deviceTypeOptions" />
        </NFormItem>
        <NFormItem label="启用设备">
          <NSwitch v-model:value="deviceDraft.isEnabled" />
        </NFormItem>
      </NForm>
      <template #footer>
        <div class="modal-actions">
          <NButton @click="isDeviceModalOpen = false">取消</NButton>
          <NButton type="primary" @click="handleSaveDevice">保存</NButton>
        </div>
      </template>
    </NModal>

    <NModal v-model:show="isBatchModalOpen" preset="card" title="批量添加设备" style="width: 620px">
      <NForm label-placement="top">
        <div class="form-grid">
          <NFormItem label="设备数量（1-1000）">
            <NInputNumber v-model:value="batchDraft.count" :min="1" :max="1000" :show-button="false" />
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
        <NFormItem label="创建后立即启用">
          <NSwitch v-model:value="batchDraft.isEnabled" />
        </NFormItem>
      </NForm>
      <p class="form-hint">默认关闭；关闭时，新建设备不会进入运行状态。</p>
      <template #footer>
        <div class="modal-actions">
          <NButton @click="isBatchModalOpen = false">取消</NButton>
          <NButton type="primary" @click="handleBatchCreate">创建 {{ batchDraft.count }} 台设备</NButton>
        </div>
      </template>
    </NModal>
  </section>
</template>
