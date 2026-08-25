<script setup lang="ts">
  import { computed, h, ref } from 'vue';
  import {
    NCard,
    NDataTable,
    NDrawer,
    NDrawerContent,
    NSelect,
    NTag,
    type DataTableColumns,
  } from 'naive-ui';

  import {
    useSimulatorStore,
    type DeviceSubscription,
    type SimulatedDevice,
    type SubscriptionKind,
  } from '@/features/simulator';

  interface SubscriptionRow extends DeviceSubscription {
    device: SimulatedDevice | null;
  }

  const store = useSimulatorStore();
  const kindFilter = ref<'all' | SubscriptionKind>('all');
  const statusFilter = ref<'all' | 'active' | 'inactive'>('all');
  const selectedSubscriptionId = ref<string | null>(null);
  const isDrawerOpen = ref(false);
  const kindOptions = [
    { label: '全部类型', value: 'all' },
    { label: '目录 Catalog', value: 'catalog' },
    { label: '报警 Alarm', value: 'alarm' },
    { label: '移动位置', value: 'mobile-position' },
  ];
  const statusOptions = [
    { label: '全部状态', value: 'all' },
    { label: '已订阅', value: 'active' },
    { label: '未订阅', value: 'inactive' },
  ];

  const subscriptionRows = computed<SubscriptionRow[]>(() =>
    store.subscriptions.map((subscription) => ({
      ...subscription,
      device: store.devices.find((device) => device.id === subscription.deviceId) ?? null,
    })),
  );
  const filteredSubscriptions = computed(() =>
    subscriptionRows.value.filter(
      (subscription) =>
        (kindFilter.value === 'all' || subscription.kind === kindFilter.value) &&
        (statusFilter.value === 'all' || subscription.status === statusFilter.value),
    ),
  );
  const selectedSubscription = computed(() =>
    selectedSubscriptionId.value === null
      ? null
      : subscriptionRows.value.find((subscription) => subscription.id === selectedSubscriptionId.value) ?? null,
  );

  function displayKind(kind: SubscriptionKind): string {
    if (kind === 'catalog') {
      return '目录 Catalog';
    }
    if (kind === 'alarm') {
      return '报警 Alarm';
    }
    return '移动位置';
  }

  function selectSubscription(subscription: SubscriptionRow): void {
    selectedSubscriptionId.value = subscription.id;
    isDrawerOpen.value = true;
  }

  const columns: DataTableColumns<SubscriptionRow> = [
    { title: '订阅类型', key: 'kind', width: 142, render: (subscription) => displayKind(subscription.kind) },
    { title: '设备名称', key: 'deviceName', minWidth: 150, render: (subscription) => subscription.device?.name ?? '设备已删除' },
    { title: '设备 ID', key: 'deviceId', minWidth: 210 },
    {
      title: '设备状态',
      key: 'deviceState',
      width: 100,
      render: (subscription) =>
        h(
          NTag,
          { size: 'small', type: subscription.device?.isEnabled === true ? 'success' : 'default' },
          { default: () => (subscription.device?.isEnabled === true ? '已启用' : '未启用') },
        ),
    },
    {
      title: '订阅状态',
      key: 'status',
      width: 104,
      render: (subscription) =>
        h(
          NTag,
          { size: 'small', type: subscription.status === 'active' ? 'success' : 'warning' },
          { default: () => (subscription.status === 'active' ? '已订阅' : '未订阅') },
        ),
    },
    { title: '到期时间', key: 'expiresAt', minWidth: 164, render: (subscription) => subscription.expiresAt ?? '—' },
    { title: '最近通知', key: 'lastNotifiedAt', minWidth: 164, render: (subscription) => subscription.lastNotifiedAt ?? '—' },
  ];
</script>

<template>
  <section class="page-shell subscription-page" aria-labelledby="subscriptions-title">
    <header class="page-header compact-header">
      <div>
        <p class="eyebrow">SERVER SUBSCRIPTIONS</p>
        <h1 id="subscriptions-title">服务订阅</h1>
        <p>展示 SIP 服务已订阅的内容；未启用设备仍保留其演示订阅记录。</p>
      </div>
      <span class="page-count">{{ store.activeSubscriptionCount }} 项活跃订阅</span>
    </header>

    <NCard class="data-surface" :bordered="false">
      <div class="toolbar-row subscription-toolbar">
        <p>共享服务：<strong>{{ store.sipService.uri }}</strong></p>
        <div class="toolbar-filters">
          <NSelect v-model:value="kindFilter" :options="kindOptions" />
          <NSelect v-model:value="statusFilter" :options="statusOptions" />
        </div>
      </div>
      <NDataTable
        :columns="columns"
        :data="filteredSubscriptions"
        :pagination="{ pageSize: 10 }"
        :row-key="(subscription) => subscription.id"
        :row-props="(subscription) => ({ onClick: () => selectSubscription(subscription), style: 'cursor: pointer' })"
      />
    </NCard>

    <NDrawer v-model:show="isDrawerOpen" :width="420" placement="right">
      <NDrawerContent v-if="selectedSubscription !== null" title="订阅详情" closable>
        <dl class="detail-list">
          <div><dt>订阅类型</dt><dd>{{ displayKind(selectedSubscription.kind) }}</dd></div>
          <div><dt>订阅状态</dt><dd><NTag :type="selectedSubscription.status === 'active' ? 'success' : 'warning'">{{ selectedSubscription.status === 'active' ? '已订阅' : '未订阅' }}</NTag></dd></div>
          <div><dt>设备名称</dt><dd>{{ selectedSubscription.device?.name ?? '设备已删除' }}</dd></div>
          <div><dt>设备 ID</dt><dd>{{ selectedSubscription.deviceId }}</dd></div>
          <div><dt>到期时间</dt><dd>{{ selectedSubscription.expiresAt ?? '—' }}</dd></div>
          <div><dt>最近通知</dt><dd>{{ selectedSubscription.lastNotifiedAt ?? '—' }}</dd></div>
        </dl>

        <section class="drawer-section">
          <h2>目录预览</h2>
          <p v-if="selectedSubscription.catalogPreview.length === 0" class="empty-state">该订阅暂无目录内容可预览。</p>
          <ul v-else class="catalog-preview">
            <li v-for="item in selectedSubscription.catalogPreview" :key="item">{{ item }}</li>
          </ul>
        </section>
      </NDrawerContent>
    </NDrawer>
  </section>
</template>
