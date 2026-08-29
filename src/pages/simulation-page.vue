<script setup lang="ts">
  import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
  import {
    NButton,
    NCard,
    NForm,
    NFormItem,
    NInput,
    NInputNumber,
    NSelect,
    NSwitch,
    NTag,
    useMessage,
  } from 'naive-ui';

  import {
    useRuntimeStore,
    type AlarmCommand,
    type PositionCommand,
    type PositionSimulationMode,
    type PtzMotion,
  } from '@/features/runtime';
  import {
    alarmMethodOptions,
    alarmPriorityOptions,
    getAlarmTypeOptions,
  } from '@/features/simulator';

  const store = useRuntimeStore();
  const message = useMessage();
  const deviceId = ref('');
  const channelId = ref('');
  const ptzSpeed = ref(32);
  const presetId = ref(1);
  const presetName = ref('预置位 1');
  const recordingName = ref('模拟录像');
  const subscriptionKind = ref('Catalog');
  const subscriptionExpires = ref(3600);
  const subscriptionError = ref('模拟订阅失败');
  const alarm = reactive<AlarmCommand>({
    active: true,
    priority: '1',
    method: '2',
    alarmType: null,
    description: '模拟报警',
    intervalSeconds: null,
  });
  const position = reactive<PositionCommand>({
    longitude: 116.397,
    latitude: 39.908,
    speed: 0,
    direction: 0,
    altitude: 0,
    mode: 'fixed',
    running: false,
    intervalSeconds: null,
  });

  const selectedDevice = computed(() =>
    store.snapshot.devices.find((device) => device.deviceId === deviceId.value),
  );
  const selectedChannel = computed(() =>
    selectedDevice.value?.channels.find((channel) => channel.channelId === channelId.value),
  );
  const channelOptions = computed(() =>
    (selectedDevice.value?.channels ?? []).map((channel) => ({
      label: `${channel.name} · ${channel.channelId}`,
      value: channel.channelId,
    })),
  );
  const alarmTypeOptions = computed(() => getAlarmTypeOptions(alarm.method));
  const positionModeOptions: Array<{ label: string; value: PositionSimulationMode }> = [
    { label: '固定坐标', value: 'fixed' },
    { label: '按方向移动', value: 'route' },
    { label: '随机游走', value: 'randomWalk' },
  ];
  const subscriptionKindOptions = ['Catalog', 'Alarm', 'MobilePosition'].map((value) => ({
    label: value,
    value,
  }));

  watch(
    () => store.snapshot.devices,
    (devices) => {
      if (!devices.some((device) => device.deviceId === deviceId.value)) {
        deviceId.value = devices[0]?.deviceId ?? '';
      }
    },
    { immediate: true },
  );
  watch([deviceId, channelOptions], () => {
    if (!channelOptions.value.some((channel) => channel.value === channelId.value)) {
      channelId.value = channelOptions.value[0]?.value ?? '';
    }
  });
  watch(
    () => alarm.method,
    () => {
      if (!alarmTypeOptions.value.some((option) => option.value === alarm.alarmType)) {
        alarm.alarmType = alarmTypeOptions.value[0]?.value || null;
      }
    },
  );

  onMounted(() => {
    store.startPolling();
    void store.refreshProjections();
  });
  onBeforeUnmount(store.stopPolling);

  async function run(operation: Promise<unknown | null>, success: string): Promise<void> {
    const result = await operation;
    if (result === null) message.error(store.errorMessage ?? '操作失败。');
    else message.success(success);
  }

  function control(kind: 'guard' | 'unguard' | 'alarmReset' | 'setOnline' | 'setOffline') {
    void run(store.controlDevice(deviceId.value, { kind }), '设备状态已更新。');
  }

  function restart(): void {
    void run(
      store.controlDevice(deviceId.value, { kind: 'restart', durationSeconds: 3 }),
      '设备正在模拟重启。',
    );
  }

  function move(motion: PtzMotion): void {
    if (channelId.value.length === 0) return;
    void run(
      store.controlPtz(deviceId.value, channelId.value, {
        kind: 'move',
        motion,
        speed: ptzSpeed.value,
      }),
      'PTZ 状态已更新。',
    );
  }

  function stopPtz(): void {
    void run(store.controlPtz(deviceId.value, channelId.value, { kind: 'stop' }), 'PTZ 已停止。');
  }
</script>

<template>
  <section class="page-shell simulation-page" aria-labelledby="simulation-title">
    <header class="page-header compact-header">
      <div>
        <p class="eyebrow">LOCAL SIMULATION</p>
        <h1 id="simulation-title">模拟控制</h1>
        <p>在不连接平台、不依赖注册和订阅的情况下操作设备及通道运行态。</p>
      </div>
      <NTag :type="selectedDevice?.connectivity === 'online' ? 'success' : 'warning'">
        {{ selectedDevice?.connectivity ?? '未选择设备' }}
      </NTag>
    </header>

    <div class="simulation-selector">
      <NSelect v-model:value="deviceId" :options="store.deviceOptions" placeholder="选择设备" />
      <NSelect v-model:value="channelId" :options="channelOptions" placeholder="选择通道" />
    </div>

    <div class="simulation-grid">
      <NCard title="设备控制" :bordered="false">
        <div class="button-grid">
          <NButton @click="control('setOnline')">上线</NButton>
          <NButton @click="control('setOffline')">离线</NButton>
          <NButton @click="restart">重启 3 秒</NButton>
          <NButton @click="control('guard')">布防</NButton>
          <NButton @click="control('unguard')">撤防</NButton>
          <NButton @click="control('alarmReset')">报警复位</NButton>
        </div>
        <dl v-if="selectedDevice" class="compact-state-list">
          <dt>连通状态</dt><dd>{{ selectedDevice.connectivity }}</dd> <dt>布防状态</dt
          ><dd>{{ selectedDevice.guarded ? '已布防' : '未布防' }}</dd> <dt>时钟偏移</dt
          ><dd>{{ selectedDevice.clockOffsetMillis }} ms</dd>
        </dl>
      </NCard>

      <NCard title="PTZ 与预置位" :bordered="false">
        <NForm label-placement="left" label-width="76">
          <NFormItem label="速度"
            ><NInputNumber v-model:value="ptzSpeed" :min="1" :max="255"
          /></NFormItem>
        </NForm>
        <div class="ptz-grid">
          <NButton @click="move('up')">上</NButton>
          <NButton @click="move('down')">下</NButton>
          <NButton @click="move('left')">左</NButton>
          <NButton @click="move('right')">右</NButton>
          <NButton @click="move('zoomIn')">放大</NButton>
          <NButton @click="move('zoomOut')">缩小</NButton>
          <NButton @click="move('focusNear')">近焦</NButton>
          <NButton @click="move('focusFar')">远焦</NButton>
          <NButton @click="move('irisOpen')">光圈+</NButton>
          <NButton @click="move('irisClose')">光圈-</NButton>
          <NButton type="warning" @click="stopPtz">停止</NButton>
        </div>
        <div class="preset-row">
          <NInputNumber v-model:value="presetId" :min="1" :max="255" />
          <NInput v-model:value="presetName" />
          <NButton
            @click="
              run(
                store.controlPtz(deviceId, channelId, {
                  kind: 'setPreset',
                  id: presetId,
                  name: presetName,
                }),
                '预置位已保存。',
              )
            "
            >保存</NButton
          >
          <NButton
            @click="
              run(
                store.controlPtz(deviceId, channelId, { kind: 'callPreset', id: presetId }),
                '预置位已调用。',
              )
            "
            >调用</NButton
          >
          <NButton
            type="error"
            @click="
              run(
                store.controlPtz(deviceId, channelId, { kind: 'deletePreset', id: presetId }),
                '预置位已删除。',
              )
            "
            >删除</NButton
          >
        </div>
        <p class="state-line"
          >当前：{{ selectedChannel?.ptz.motion ?? 'stop' }} · Pan
          {{ selectedChannel?.ptz.pan ?? 0 }} · Tilt {{ selectedChannel?.ptz.tilt ?? 0 }} · Zoom
          {{ selectedChannel?.ptz.zoom ?? 0 }}</p
        >
      </NCard>

      <NCard title="Alarm 模拟" :bordered="false">
        <NForm label-placement="top">
          <div class="form-grid">
            <NFormItem label="报警级别"
              ><NSelect v-model:value="alarm.priority" :options="alarmPriorityOptions"
            /></NFormItem>
            <NFormItem label="报警方式"
              ><NSelect v-model:value="alarm.method" :options="alarmMethodOptions"
            /></NFormItem>
            <NFormItem v-if="alarmTypeOptions.length > 0" label="报警类型"
              ><NSelect v-model:value="alarm.alarmType" :options="alarmTypeOptions"
            /></NFormItem>
          </div>
          <NFormItem label="描述"><NInput v-model:value="alarm.description" /></NFormItem>
          <NFormItem label="周期（秒，留空为单次）"
            ><NInputNumber v-model:value="alarm.intervalSeconds" clearable :min="1" :max="86400"
          /></NFormItem>
        </NForm>
        <div class="button-grid two">
          <NButton
            type="error"
            @click="
              run(
                store.updateAlarm(deviceId, channelId, { ...alarm, active: true }),
                '报警已发生。',
              )
            "
            >报警发生</NButton
          >
          <NButton
            @click="
              run(
                store.updateAlarm(deviceId, channelId, {
                  ...alarm,
                  active: false,
                  intervalSeconds: null,
                }),
                '报警已恢复。',
              )
            "
            >报警恢复</NButton
          >
        </div>
      </NCard>

      <NCard title="Mobile Position 模拟" :bordered="false">
        <NForm label-placement="top">
          <div class="form-grid">
            <NFormItem label="经度"
              ><NInputNumber v-model:value="position.longitude" :min="-180" :max="180"
            /></NFormItem>
            <NFormItem label="纬度"
              ><NInputNumber v-model:value="position.latitude" :min="-90" :max="90"
            /></NFormItem>
            <NFormItem label="速度 km/h"
              ><NInputNumber v-model:value="position.speed" :min="0"
            /></NFormItem>
            <NFormItem label="方向"
              ><NInputNumber v-model:value="position.direction" :min="0" :max="360"
            /></NFormItem>
            <NFormItem label="模式"
              ><NSelect v-model:value="position.mode" :options="positionModeOptions"
            /></NFormItem>
            <NFormItem label="周期（秒）"
              ><NInputNumber v-model:value="position.intervalSeconds" clearable :min="1"
            /></NFormItem>
          </div>
          <NFormItem label="周期运行"><NSwitch v-model:value="position.running" /></NFormItem>
        </NForm>
        <NButton
          type="primary"
          @click="
            run(store.updatePosition(deviceId, channelId, { ...position }), '位置状态已更新。')
          "
          >应用位置</NButton
        >
      </NCard>

      <NCard title="录像与回放索引" :bordered="false">
        <NForm label-placement="top">
          <NFormItem label="录像名称"><NInput v-model:value="recordingName" /></NFormItem>
        </NForm>
        <div class="button-grid">
          <NButton
            type="primary"
            @click="
              run(
                store.controlRecording(deviceId, channelId, {
                  kind: 'start',
                  name: recordingName,
                }),
                '录像已开始。',
              )
            "
            >开始</NButton
          >
          <NButton
            @click="
              run(store.controlRecording(deviceId, channelId, { kind: 'pause' }), '录像已暂停。')
            "
            >暂停</NButton
          >
          <NButton
            @click="
              run(store.controlRecording(deviceId, channelId, { kind: 'resume' }), '录像已继续。')
            "
            >继续</NButton
          >
          <NButton
            type="warning"
            @click="
              run(
                store.controlRecording(deviceId, channelId, { kind: 'stop' }),
                '录像已停止并写入索引。',
              )
            "
            >停止</NButton
          >
        </div>
        <dl class="compact-state-list">
          <dt>当前状态</dt><dd>{{ selectedChannel?.recording.status ?? 'idle' }}</dd>
          <dt>当前录像</dt><dd>{{ selectedChannel?.recording.currentFile ?? '—' }}</dd>
          <dt>已录时长</dt><dd>{{ selectedChannel?.recording.durationMillis ?? 0 }} ms</dd>
          <dt>历史数量</dt><dd>{{ store.recordings.length }}</dd>
        </dl>
      </NCard>

      <NCard title="订阅生命周期" :bordered="false">
        <NForm label-placement="top">
          <div class="form-grid">
            <NFormItem label="订阅类型">
              <NSelect v-model:value="subscriptionKind" :options="subscriptionKindOptions" />
            </NFormItem>
            <NFormItem label="有效期（秒）">
              <NInputNumber v-model:value="subscriptionExpires" :min="1" :max="86400" />
            </NFormItem>
          </div>
          <NFormItem label="失败原因"><NInput v-model:value="subscriptionError" /></NFormItem>
        </NForm>
        <div class="button-grid">
          <NButton
            type="primary"
            @click="
              run(
                store.controlSubscription(deviceId, channelId, {
                  kind: 'upsert',
                  subscriptionKind,
                  expiresSeconds: subscriptionExpires,
                }),
                '订阅已建立或刷新。',
              )
            "
            >建立/刷新</NButton
          >
          <NButton
            @click="
              run(
                store.controlSubscription(deviceId, channelId, {
                  kind: 'cancel',
                  subscriptionKind,
                }),
                '订阅已取消。',
              )
            "
            >取消</NButton
          >
          <NButton
            type="error"
            @click="
              run(
                store.controlSubscription(deviceId, channelId, {
                  kind: 'fail',
                  subscriptionKind,
                  error: subscriptionError,
                }),
                '订阅失败状态已注入。',
              )
            "
            >模拟失败</NButton
          >
        </div>
        <div class="subscription-list">
          <NTag
            v-for="subscription in selectedChannel?.subscriptions ?? []"
            :key="subscription.kind"
            :type="subscription.status === 'active' ? 'success' : 'warning'"
          >
            {{ subscription.kind }} · {{ subscription.status }}
          </NTag>
        </div>
      </NCard>
    </div>
  </section>
</template>

<style scoped>
  .simulation-selector,
  .preset-row {
    display: flex;
    gap: 10px;
  }
  .simulation-selector {
    margin-bottom: 14px;
  }
  .simulation-selector > * {
    min-width: 280px;
  }
  .simulation-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px;
    overflow: auto;
    min-height: 0;
  }
  .button-grid,
  .ptz-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
  }
  .button-grid.two {
    grid-template-columns: repeat(2, 1fr);
  }
  .ptz-grid {
    grid-template-columns: repeat(4, 1fr);
  }
  .preset-row {
    margin-top: 12px;
  }
  .preset-row .n-input-number {
    width: 90px;
  }
  .compact-state-list {
    display: grid;
    grid-template-columns: 100px 1fr;
    gap: 8px;
    margin: 18px 0 0;
  }
  .compact-state-list dt {
    color: var(--text-secondary);
  }
  .compact-state-list dd {
    margin: 0;
  }
  .state-line {
    color: var(--text-secondary);
  }
  .subscription-list {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 14px;
  }
  @media (max-width: 1100px) {
    .simulation-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
