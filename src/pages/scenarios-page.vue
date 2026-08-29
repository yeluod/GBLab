<script setup lang="ts">
  import { h, onBeforeUnmount, onMounted, reactive, ref } from 'vue';
  import {
    NButton,
    NCard,
    NDataTable,
    NForm,
    NFormItem,
    NInput,
    NInputNumber,
    NSelect,
    NSwitch,
    NTag,
    useMessage,
    type DataTableColumns,
  } from 'naive-ui';

  import {
    useRuntimeStore,
    type FaultProfile,
    type ScenarioRuntimeState,
  } from '@/features/runtime';

  const store = useRuntimeStore();
  const message = useMessage();
  const scenarioName = ref('设备离线恢复与报警');
  const deviceId = ref('');
  const channelId = ref('');
  const fault = reactive<FaultProfile>({
    delayMillis: 0,
    forceTimeout: false,
    packetLossPercent: 0,
    rejectStatus: null,
    forceDeviceOffline: false,
  });
  const columns: DataTableColumns<ScenarioRuntimeState> = [
    { title: '名称', key: 'name' },
    {
      title: '状态',
      key: 'status',
      width: 110,
      render: (row) =>
        h(
          NTag,
          {
            type:
              row.status === 'running' ? 'success' : row.status === 'failed' ? 'error' : 'default',
            size: 'small',
          },
          { default: () => row.status },
        ),
    },
    {
      title: '进度',
      key: 'progress',
      width: 100,
      render: (row) => `${row.currentStep}/${row.totalSteps}`,
    },
    { title: '错误', key: 'lastError', render: (row) => row.lastError ?? '—' },
    {
      title: '操作',
      key: 'actions',
      width: 250,
      render: (row) =>
        h('div', { class: 'table-actions' }, [
          h(
            NButton,
            { size: 'small', onClick: () => run(store.startScenario(row.id), '场景已启动。') },
            { default: () => '启动' },
          ),
          h(
            NButton,
            {
              size: 'small',
              onClick: () => run(store.setScenarioStatus(row.id, 'paused'), '场景已暂停。'),
            },
            { default: () => '暂停' },
          ),
          h(
            NButton,
            {
              size: 'small',
              onClick: () => run(store.setScenarioStatus(row.id, 'running'), '场景已继续。'),
            },
            { default: () => '继续' },
          ),
          h(
            NButton,
            {
              size: 'small',
              type: 'warning',
              onClick: () => run(store.setScenarioStatus(row.id, 'stopped'), '场景已停止。'),
            },
            { default: () => '停止' },
          ),
        ]),
    },
  ];
  onMounted(() => {
    store.startPolling();
    void store.refreshProjections();
    Object.assign(fault, store.snapshot.faultProfile);
  });
  onBeforeUnmount(store.stopPolling);
  async function run(operation: Promise<unknown | null>, success: string): Promise<void> {
    const result = await operation;
    if (result === null) message.error(store.errorMessage ?? '操作失败。');
    else message.success(success);
  }
  function createScenario(): void {
    if (!deviceId.value || !channelId.value) {
      message.warning('请选择设备和通道。');
      return;
    }
    void run(
      store.saveScenario({
        id: null,
        name: scenarioName.value,
        repeat: false,
        steps: [
          {
            name: '设备离线',
            deviceId: deviceId.value,
            channelId: null,
            action: { kind: 'deviceControl', command: { kind: 'setOffline' } },
          },
          {
            name: '等待',
            deviceId: deviceId.value,
            channelId: null,
            action: { kind: 'delay', durationMillis: 2000 },
          },
          {
            name: '设备上线',
            deviceId: deviceId.value,
            channelId: null,
            action: { kind: 'deviceControl', command: { kind: 'setOnline' } },
          },
          {
            name: '触发报警',
            deviceId: deviceId.value,
            channelId: channelId.value,
            action: {
              kind: 'alarm',
              command: {
                active: true,
                priority: '1',
                method: '2',
                alarmType: null,
                description: '场景报警',
                intervalSeconds: null,
              },
            },
          },
        ],
      }),
      '场景已保存。',
    );
  }
</script>

<template>
  <section class="page-shell scenarios-page" aria-labelledby="scenarios-title">
    <header class="page-header compact-header"
      ><div
        ><p class="eyebrow">SCENARIO ENGINE</p><h1 id="scenarios-title">场景与故障</h1
        ><p>通过集中式 Scheduler 编排状态变化，并注入延迟、超时、丢包和拒绝结果。</p></div
      ></header
    >
    <div class="scenario-grid">
      <NCard title="故障注入" :bordered="false"
        ><NForm label-placement="top"
          ><div class="form-grid"
            ><NFormItem label="延迟 ms"
              ><NInputNumber v-model:value="fault.delayMillis" :min="0" :max="60000" /></NFormItem
            ><NFormItem label="丢包 %"
              ><NInputNumber
                v-model:value="fault.packetLossPercent"
                :min="0"
                :max="100" /></NFormItem
            ><NFormItem label="拒绝状态码"
              ><NInputNumber
                v-model:value="fault.rejectStatus"
                clearable
                :min="400"
                :max="699" /></NFormItem></div
          ><NFormItem label="强制超时"><NSwitch v-model:value="fault.forceTimeout" /></NFormItem
          ><NFormItem label="强制设备离线"
            ><NSwitch v-model:value="fault.forceDeviceOffline" /></NFormItem></NForm
        ><NButton
          type="primary"
          @click="run(store.setFaultProfile({ ...fault }), '故障配置已应用。')"
          >应用故障配置</NButton
        ></NCard
      >
      <NCard title="快速创建场景" :bordered="false"
        ><NForm label-placement="top"
          ><NFormItem label="场景名称"><NInput v-model:value="scenarioName" /></NFormItem
          ><NFormItem label="设备"
            ><NSelect v-model:value="deviceId" :options="store.deviceOptions" /></NFormItem
          ><NFormItem label="通道"
            ><NSelect
              v-model:value="channelId"
              :options="
                store.snapshot.devices
                  .find((d) => d.deviceId === deviceId)
                  ?.channels.map((c) => ({ label: c.name, value: c.channelId })) ?? []
              " /></NFormItem></NForm
        ><NButton type="primary" @click="createScenario">创建标准场景</NButton></NCard
      >
    </div>
    <NCard title="场景运行" :bordered="false" class="scenario-table"
      ><NDataTable :columns="columns" :data="store.scenarios" :pagination="false"
    /></NCard>
  </section>
</template>

<style scoped>
  .scenario-grid {
    display: grid;
    grid-template-columns: 1.2fr 1fr;
    gap: 14px;
  }
  .scenario-table {
    margin-top: 14px;
    min-height: 0;
  }
</style>
