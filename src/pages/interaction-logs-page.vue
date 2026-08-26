<script setup lang="ts">
  import { computed, h, nextTick, onMounted, ref, watch } from 'vue';
  import {
    NButton,
    NCard,
    NDataTable,
    NInput,
    NSelect,
    NTag,
    useMessage,
    type DataTableColumns,
    type DataTableInst,
  } from 'naive-ui';

  import { useSimulatorStore, type InteractionLog } from '@/features/simulator';

  const store = useSimulatorStore();
  const message = useMessage();
  const interactionLogTableRef = ref<DataTableInst | null>(null);
  const directionFilter = ref<'all' | InteractionLog['direction']>('all');
  const deviceKeyword = ref('');
  const channelKeyword = ref('');
  const messageKeyword = ref('');

  const directionOptions = [
    { label: '全部方向', value: 'all' },
    { label: '设备 → 服务', value: 'send' },
    { label: '服务 → 设备', value: 'receive' },
  ];

  const filteredLogs = computed(() => {
    const device = deviceKeyword.value.trim().toLowerCase();
    const channel = channelKeyword.value.trim().toLowerCase();
    const message = messageKeyword.value.trim().toLowerCase();
    return store.interactionLogs.filter((log) => {
      const matchesDirection =
        directionFilter.value === 'all' || log.direction === directionFilter.value;
      const matchesDevice = device.length === 0 || log.deviceId.toLowerCase().includes(device);
      const matchesChannel =
        channel.length === 0 || (log.channelId ?? '').toLowerCase().includes(channel);
      const matchesMessage = message.length === 0 || log.message.toLowerCase().includes(message);
      return matchesDirection && matchesDevice && matchesChannel && matchesMessage;
    });
  });

  async function scrollToLatest(): Promise<void> {
    await nextTick();
    const table = interactionLogTableRef.value;
    if (table !== null && typeof HTMLElement.prototype.scrollTo === 'function') {
      table.scrollTo({ top: Number.MAX_SAFE_INTEGER });
    }
  }

  watch(
    () => store.interactionLogs.length,
    () => {
      void scrollToLatest();
    },
  );

  onMounted(async () => {
    const result = await store.loadDevices();
    if (!result.ok) {
      message.error(result.message);
      return;
    }
    await scrollToLatest();
  });

  function formatTimestamp(timestamp: number): string {
    return new Date(timestamp).toLocaleString('zh-CN', { hour12: false });
  }

  function directionMeta(direction: InteractionLog['direction']): {
    label: string;
    type: 'info' | 'success';
  } {
    return direction === 'send'
      ? { label: '设备 → 服务', type: 'info' }
      : { label: '服务 → 设备', type: 'success' };
  }

  const columns: DataTableColumns<InteractionLog> = [
    {
      title: '时间',
      key: 'timestamp',
      width: 180,
      align: 'center',
      render: (log) => formatTimestamp(log.timestamp),
    },
    {
      title: '方向',
      key: 'direction',
      width: 150,
      align: 'center',
      render: (log) => {
        const meta = directionMeta(log.direction);
        return h(
          NTag,
          { type: meta.type, size: 'small', bordered: false },
          {
            default: () => meta.label,
          },
        );
      },
    },
    { title: '设备 ID', key: 'deviceId', minWidth: 220, align: 'center' },
    {
      title: '通道 ID',
      key: 'channelId',
      minWidth: 240,
      align: 'center',
      render: (log) => log.channelId ?? '—',
    },
    {
      title: '消息',
      key: 'message',
      minWidth: 520,
      render: (log) => h('code', { class: 'interaction-message' }, log.message),
    },
  ];
</script>

<template>
  <section class="page-shell interaction-logs-page" aria-labelledby="interaction-logs-title">
    <header class="page-header compact-header">
      <div>
        <p class="eyebrow">SIP INTERACTION LOGS</p>
        <h1 id="interaction-logs-title">交互日志</h1>
        <p>实时查看模拟设备与共享 SIP 服务之间的完整 SIP / GB28181 交互内容。</p>
      </div>
      <NButton secondary @click="scrollToLatest">回到底部</NButton>
    </header>

    <NCard class="data-surface interaction-logs-surface" :bordered="false">
      <div class="logs-toolbar">
        <NSelect v-model:value="directionFilter" :options="directionOptions" />
        <NInput v-model:value="deviceKeyword" clearable placeholder="设备 ID" />
        <NInput v-model:value="channelKeyword" clearable placeholder="通道 ID" />
        <NInput v-model:value="messageKeyword" clearable placeholder="消息关键字" />
      </div>
      <div class="interaction-log-scroll standalone-log-scroll">
        <NDataTable
          ref="interactionLogTableRef"
          class="interaction-log-table"
          flex-height
          :columns="columns"
          :data="filteredLogs"
          :pagination="false"
          :scroll-x="1350"
          :scrollbar-props="{ trigger: 'none', size: 10 }"
          :row-key="(log) => log.id"
        />
      </div>
    </NCard>
  </section>
</template>
