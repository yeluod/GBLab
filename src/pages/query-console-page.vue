<script setup lang="ts">
  import { computed, h, onBeforeUnmount, onMounted, ref } from 'vue';
  import {
    NButton,
    NCard,
    NDataTable,
    NInput,
    NSelect,
    NTag,
    useMessage,
    type DataTableColumns,
  } from 'naive-ui';

  import { useRuntimeStore, type QueryKind, type QueryResult } from '@/features/runtime';

  const store = useRuntimeStore();
  const message = useMessage();
  const deviceId = ref('');
  const channelId = ref<string | null>(null);
  const kind = ref<QueryKind>('deviceInfo');
  const selectedResult = ref<QueryResult | null>(null);
  const parametersText = ref('{}');
  const selectedDevice = computed(() =>
    store.snapshot.devices.find((item) => item.deviceId === deviceId.value),
  );
  const channelOptions = computed(() => [
    { label: '设备级查询', value: '' },
    ...(selectedDevice.value?.channels ?? []).map((channel) => ({
      label: `${channel.name} · ${channel.channelId}`,
      value: channel.channelId,
    })),
  ]);
  const queryOptions: Array<{ label: string; value: QueryKind }> = [
    ['Catalog', 'catalog'],
    ['DeviceInfo', 'deviceInfo'],
    ['DeviceStatus', 'deviceStatus'],
    ['DeviceCapability', 'deviceCapability'],
    ['DeviceTime', 'deviceTime'],
    ['DeviceParameter', 'deviceParameter'],
    ['ConfigDownload', 'configDownload'],
    ['AlarmStatus', 'alarmStatus'],
    ['MobilePosition', 'mobilePosition'],
    ['PresetQuery', 'presetQuery'],
    ['RecordInfo', 'recordInfo'],
  ].map(([label, value]) => ({ label: label ?? '', value: value as QueryKind }));
  const columns: DataTableColumns<QueryResult> = [
    {
      title: '时间',
      key: 'startedAt',
      width: 180,
      render: (row) => new Date(row.startedAt).toLocaleString('zh-CN', { hour12: false }),
    },
    { title: '查询', key: 'kind', width: 150, render: (row) => row.request.kind },
    { title: '设备 ID', key: 'deviceId', width: 210, render: (row) => row.request.deviceId },
    {
      title: '通道 ID',
      key: 'channelId',
      width: 210,
      render: (row) => row.request.channelId ?? '—',
    },
    {
      title: '状态',
      key: 'status',
      width: 110,
      render: (row) =>
        h(
          NTag,
          { type: row.status === 'succeeded' ? 'success' : 'error', size: 'small' },
          { default: () => row.status },
        ),
    },
    {
      title: '耗时',
      key: 'durationMillis',
      width: 100,
      render: (row) => `${row.durationMillis} ms`,
    },
  ];
  onMounted(() => {
    store.startPolling();
    void store.refreshProjections();
  });
  onBeforeUnmount(store.stopPolling);
  async function execute(): Promise<void> {
    if (deviceId.value.length === 0) {
      message.warning('请选择设备。');
      return;
    }
    let parameters: Record<string, unknown>;
    try {
      const parsed: unknown = JSON.parse(parametersText.value);
      if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
        throw new Error('查询参数必须是 JSON 对象。');
      }
      parameters = parsed as Record<string, unknown>;
    } catch (error) {
      message.error(error instanceof Error ? error.message : '查询参数 JSON 无效。');
      return;
    }
    const result = await store.executeQuery({
      deviceId: deviceId.value,
      channelId: channelId.value || null,
      kind: kind.value,
      parameters,
      mode: 'localSimulation',
    });
    if (result === null) message.error(store.errorMessage ?? '查询失败。');
    else {
      selectedResult.value = result;
      message.success('查询完成。');
    }
  }
</script>

<template>
  <section class="page-shell query-page" aria-labelledby="query-title">
    <header class="page-header compact-header"
      ><div
        ><p class="eyebrow">QUERY / RESPONSE</p><h1 id="query-title">查询控制台</h1
        ><p
          >统一执行本地 Catalog、设备信息、状态、能力、参数、报警、位置、预置位和录像查询。</p
        ></div
      ></header
    >
    <NCard :bordered="false" class="query-toolbar-card">
      <div class="query-toolbar">
        <NSelect v-model:value="deviceId" :options="store.deviceOptions" placeholder="选择设备" />
        <NSelect
          v-model:value="channelId"
          :options="channelOptions"
          placeholder="设备级或通道"
          clearable
        />
        <NSelect v-model:value="kind" :options="queryOptions" />
        <NButton type="primary" :loading="store.isSubmitting" @click="execute">执行查询</NButton>
      </div>
      <NInput
        v-model:value="parametersText"
        type="textarea"
        :rows="2"
        placeholder='扩展参数 JSON，例如 {"startTime": 0, "endTime": 9999999999999}'
      />
    </NCard>
    <div class="query-workspace">
      <NCard title="查询历史" :bordered="false"
        ><NDataTable
          :columns="columns"
          :data="store.queries"
          :pagination="false"
          :scroll-x="1000"
          :row-props="
            (row) => ({ onClick: () => (selectedResult = row), style: 'cursor:pointer' })
          "
      /></NCard>
      <NCard title="结构化响应" :bordered="false">
        <NButton v-if="selectedResult" size="small" @click="execute">重试当前查询</NButton>
        <pre>{{
          JSON.stringify(
            selectedResult?.response ?? { message: '选择或执行一条查询查看响应' },
            null,
            2,
          )
        }}</pre
        ><p v-if="selectedResult?.error" class="query-error">{{ selectedResult.error }}</p></NCard
      >
    </div>
  </section>
</template>

<style scoped>
  .query-toolbar {
    display: grid;
    grid-template-columns: 1.2fr 1.4fr 1fr auto;
    gap: 10px;
  }
  .query-workspace {
    display: grid;
    grid-template-columns: minmax(0, 1.5fr) minmax(320px, 0.8fr);
    gap: 14px;
    min-height: 0;
    margin-top: 14px;
  }
  pre {
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 52vh;
    overflow: auto;
    background: #f8fafc;
    padding: 14px;
    border-radius: 6px;
  }
  .query-error {
    color: var(--n-error-color);
  }
</style>
