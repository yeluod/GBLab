<script setup lang="ts">
  import { h, onBeforeUnmount, onMounted, ref } from 'vue';
  import { NCard, NDataTable, NTabPane, NTabs, NTag, type DataTableColumns } from 'naive-ui';
  import {
    useRuntimeStore,
    type OperationRecord,
    type RuntimeEventRecord,
    type TransactionRecord,
  } from '@/features/runtime';

  const store = useRuntimeStore();
  const activeTab = ref('events');
  const eventColumns: DataTableColumns<RuntimeEventRecord> = [
    {
      title: '时间',
      key: 'timestamp',
      width: 180,
      render: (row) => new Date(row.timestamp).toLocaleString('zh-CN', { hour12: false }),
    },
    {
      title: '级别',
      key: 'level',
      width: 90,
      render: (row) =>
        h(
          NTag,
          {
            size: 'small',
            type: row.level === 'error' ? 'error' : row.level === 'warning' ? 'warning' : 'info',
          },
          { default: () => row.level },
        ),
    },
    { title: '事件', key: 'kind', width: 170 },
    { title: '设备 ID', key: 'deviceId', width: 210 },
    { title: '通道 ID', key: 'channelId', width: 210 },
    { title: '消息', key: 'message' },
  ];
  const operationColumns: DataTableColumns<OperationRecord> = [
    {
      title: '开始时间',
      key: 'startedAt',
      width: 180,
      render: (row) => new Date(row.startedAt).toLocaleString('zh-CN', { hour12: false }),
    },
    { title: '操作', key: 'kind', width: 160 },
    { title: '状态', key: 'status', width: 110 },
    { title: '设备 ID', key: 'deviceId', width: 210, render: (row) => row.target.deviceId ?? '—' },
    {
      title: '通道 ID',
      key: 'channelId',
      width: 210,
      render: (row) => row.target.channelId ?? '—',
    },
    {
      title: '耗时',
      key: 'durationMillis',
      width: 100,
      render: (row) => (row.durationMillis === null ? '—' : `${row.durationMillis} ms`),
    },
    { title: '错误', key: 'errorMessage', render: (row) => row.errorMessage ?? '—' },
  ];
  const transactionColumns: DataTableColumns<TransactionRecord> = [
    { title: '事务 ID', key: 'id', width: 210 },
    { title: 'Call-ID', key: 'callId', width: 240 },
    { title: 'CSeq', key: 'cseq', width: 90 },
    { title: 'Method', key: 'method', width: 110 },
    { title: 'Via branch', key: 'viaBranch', width: 230 },
    { title: '状态', key: 'status', width: 120 },
    { title: '响应码', key: 'responseStatus', width: 100 },
    { title: '错误', key: 'error' },
  ];
  onMounted(() => {
    store.startPolling();
    void store.refreshProjections();
  });
  onBeforeUnmount(store.stopPolling);
</script>

<template>
  <section class="page-shell observability-page" aria-labelledby="observability-title">
    <header class="page-header compact-header"
      ><div
        ><p class="eyebrow">RUNTIME OBSERVABILITY</p><h1 id="observability-title">运行观测</h1
        ><p>关联查看状态事件、操作结果、业务错误与 SIP 事务。</p></div
      ><NTag>{{ store.snapshot.revision }} revision</NTag></header
    >
    <NCard :bordered="false" class="observability-card"
      ><NTabs v-model:value="activeTab" type="line"
        ><NTabPane name="events" tab="运行事件"
          ><NDataTable
            flex-height
            :columns="eventColumns"
            :data="store.events"
            :pagination="false"
            :scroll-x="1200" /></NTabPane
        ><NTabPane name="operations" tab="操作记录"
          ><NDataTable
            flex-height
            :columns="operationColumns"
            :data="store.operations"
            :pagination="false"
            :scroll-x="1300" /></NTabPane
        ><NTabPane name="transactions" tab="SIP 事务"
          ><NDataTable
            flex-height
            :columns="transactionColumns"
            :data="store.transactions"
            :pagination="false"
            :scroll-x="1300" /></NTabPane></NTabs
    ></NCard>
  </section>
</template>

<style scoped>
  .observability-card {
    flex: 1;
    min-height: 0;
  }
  .observability-card :deep(.n-card__content),
  .observability-card :deep(.n-tabs),
  .observability-card :deep(.n-tab-pane) {
    height: 100%;
    min-height: 0;
  }
</style>
