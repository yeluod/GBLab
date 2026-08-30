<script setup lang="ts">
  import { computed, h, nextTick, onMounted, ref, watch } from 'vue';
  import {
    NButton,
    NCard,
    NCheckbox,
    NDataTable,
    NInput,
    NModal,
    NSelect,
    NTag,
    useDialog,
    useMessage,
    type DataTableColumns,
    type DataTableInst,
  } from 'naive-ui';

  import {
    classifyInteractionMessage,
    directionLabel,
    formatLogsAsTsv,
    formatTimestamp,
    useSimulatorStore,
    type InteractionLog,
  } from '@/features/simulator';
  import AppIcon from '@/shared/components/app-icon.vue';

  const store = useSimulatorStore();
  const message = useMessage();
  const dialog = useDialog();
  const interactionLogTableRef = ref<DataTableInst | null>(null);
  const directionFilter = ref<'all' | InteractionLog['direction']>('all');
  const deviceKeyword = ref('');
  const channelKeyword = ref('');
  const messageKeyword = ref('');
  const selectedLogIds = ref<Set<string>>(new Set());
  const activeLog = ref<InteractionLog | null>(null);
  const isMessageModalVisible = ref(false);

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

  const selectedLogs = computed(() =>
    store.interactionLogs.filter((log) => selectedLogIds.value.has(log.id)),
  );
  const isAllFilteredSelected = computed(
    () =>
      filteredLogs.value.length > 0 &&
      filteredLogs.value.every((log) => selectedLogIds.value.has(log.id)),
  );
  const isSomeFilteredSelected = computed(
    () =>
      filteredLogs.value.some((log) => selectedLogIds.value.has(log.id)) &&
      !isAllFilteredSelected.value,
  );

  function updateSelectedLog(logId: string, checked: boolean): void {
    const next = new Set(selectedLogIds.value);
    if (checked) {
      next.add(logId);
    } else {
      next.delete(logId);
    }
    selectedLogIds.value = next;
  }

  function updateFilteredSelection(checked: boolean): void {
    const next = new Set(selectedLogIds.value);
    filteredLogs.value.forEach((log) => {
      if (checked) {
        next.add(log.id);
      } else {
        next.delete(log.id);
      }
    });
    selectedLogIds.value = next;
  }

  function clearSelection(): void {
    selectedLogIds.value = new Set();
  }

  async function copyText(text: string): Promise<void> {
    if (navigator.clipboard !== undefined && typeof navigator.clipboard.writeText === 'function') {
      await navigator.clipboard.writeText(text);
      return;
    }
    const textarea = document.createElement('textarea');
    textarea.value = text;
    textarea.setAttribute('readonly', '');
    textarea.style.position = 'fixed';
    textarea.style.opacity = '0';
    document.body.appendChild(textarea);
    textarea.select();
    const copied = document.execCommand('copy');
    textarea.remove();
    if (!copied) {
      throw new Error('clipboard unavailable');
    }
  }

  async function copySelectedLogs(): Promise<void> {
    if (selectedLogs.value.length === 0) {
      return;
    }
    const content = formatLogsAsTsv(selectedLogs.value);
    try {
      await copyText(content);
      message.success(`已复制 ${selectedLogs.value.length} 条日志。`);
    } catch {
      message.error('系统剪贴板不可用，复制失败。');
    }
  }

  function confirmClearLogs(): void {
    dialog.warning({
      title: '清空交互日志',
      content: '确定清空当前运行时内的全部交互日志吗？不会影响设备、SIP 配置或注册生命周期。',
      positiveText: '清空日志',
      negativeText: '取消',
      onPositiveClick: () => {
        store.clearInteractionLogs();
        clearSelection();
        message.success('交互日志已清空。');
      },
    });
  }

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

  watch(
    () => store.interactionLogs.map((log) => log.id),
    (ids) => {
      const validIds = new Set(ids);
      selectedLogIds.value = new Set([...selectedLogIds.value].filter((id) => validIds.has(id)));
      if (activeLog.value !== null && !validIds.has(activeLog.value.id)) {
        closeMessage();
      }
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

  function directionMeta(direction: InteractionLog['direction']): {
    label: string;
    type: 'info' | 'success';
  } {
    return direction === 'send'
      ? { label: '设备 → 服务', type: 'info' }
      : { label: '服务 → 设备', type: 'success' };
  }

  function messageTypeMeta(log: InteractionLog): {
    label: string;
    type: 'default' | 'info' | 'success' | 'warning' | 'error';
  } {
    const messageType = classifyInteractionMessage(log.message);
    if (messageType.kind === 'sip-response') {
      return {
        label: messageType.label,
        type:
          messageType.status >= 400 ? 'error' : messageType.status >= 300 ? 'warning' : 'success',
      };
    }
    if (messageType.kind === 'gb-command') {
      return { label: messageType.label, type: 'info' };
    }
    if (messageType.kind === 'sip-request') {
      return { label: messageType.label, type: 'default' };
    }
    return { label: messageType.label, type: 'default' };
  }

  function openMessage(log: InteractionLog): void {
    activeLog.value = log;
    isMessageModalVisible.value = true;
  }

  function closeMessage(): void {
    isMessageModalVisible.value = false;
    activeLog.value = null;
  }

  const columns: DataTableColumns<InteractionLog> = [
    {
      title: () =>
        h(NCheckbox, {
          checked: isAllFilteredSelected.value,
          indeterminate: isSomeFilteredSelected.value,
          disabled: filteredLogs.value.length === 0,
          'onUpdate:checked': updateFilteredSelection,
        }),
      key: 'selection',
      width: 58,
      align: 'center',
      render: (log) =>
        h(NCheckbox, {
          checked: selectedLogIds.value.has(log.id),
          'onUpdate:checked': (checked: boolean) => updateSelectedLog(log.id, checked),
          onClick: (event: MouseEvent) => event.stopPropagation(),
        }),
    },
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
    {
      title: '消息类型',
      key: 'messageType',
      width: 170,
      align: 'center',
      render: (log) => {
        const meta = messageTypeMeta(log);
        return h(
          NTag,
          { type: meta.type, size: 'small', bordered: false },
          { default: () => meta.label },
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
      title: '操作',
      key: 'actions',
      width: 72,
      align: 'center',
      render: (log) =>
        h(
          NButton,
          {
            quaternary: true,
            circle: true,
            size: 'small',
            'aria-label': '查看完整消息',
            title: '查看完整消息',
            onClick: () => openMessage(log),
          },
          { icon: () => h(AppIcon, { icon: 'message', size: 16 }) },
        ),
    },
  ];
</script>

<template>
  <section class="page-shell interaction-logs-page" aria-labelledby="interaction-logs-title">
    <header class="page-header compact-header">
      <div>
        <p class="eyebrow">SIP EVENT STREAM</p>
        <h1 id="interaction-logs-title">交互日志</h1>
        <p>实时查看模拟设备与共享 SIP 服务之间的完整 SIP / GB28181 交互内容。</p>
      </div>
      <div class="log-header-actions">
        <NButton secondary :disabled="selectedLogs.length === 0" @click="copySelectedLogs">
          <template #icon><AppIcon icon="copy" /></template>
          复制选中
        </NButton>
        <NButton
          secondary
          type="error"
          :disabled="store.interactionLogs.length === 0"
          @click="confirmClearLogs"
        >
          <template #icon><AppIcon icon="trash" /></template>
          清空日志
        </NButton>
        <NButton secondary @click="scrollToLatest">
          <template #icon><AppIcon icon="arrowDown" /></template>
          回到底部
        </NButton>
      </div>
    </header>

    <NCard class="data-surface interaction-logs-surface" :bordered="false">
      <div class="logs-toolbar">
        <NSelect v-model:value="directionFilter" :options="directionOptions">
          <template #arrow><AppIcon icon="filter" :size="14" /></template>
        </NSelect>
        <NInput v-model:value="deviceKeyword" clearable placeholder="设备 ID">
          <template #prefix><AppIcon icon="server" :size="15" /></template>
        </NInput>
        <NInput v-model:value="channelKeyword" clearable placeholder="通道 ID">
          <template #prefix><AppIcon icon="rows" :size="15" /></template>
        </NInput>
        <NInput v-model:value="messageKeyword" clearable placeholder="消息关键字">
          <template #prefix><AppIcon icon="search" :size="15" /></template>
        </NInput>
      </div>
      <div class="interaction-log-scroll standalone-log-scroll">
        <NDataTable
          ref="interactionLogTableRef"
          class="interaction-log-table"
          flex-height
          :columns="columns"
          :data="filteredLogs"
          :pagination="false"
          :scroll-x="1100"
          :scrollbar-props="{ trigger: 'none', size: 10 }"
          :row-key="(log) => log.id"
        />
      </div>
    </NCard>

    <NModal
      v-model:show="isMessageModalVisible"
      preset="card"
      title="完整消息"
      :style="{ width: 'min(900px, calc(100vw - 48px))' }"
      :mask-closable="true"
      @after-leave="closeMessage"
    >
      <template v-if="activeLog !== null">
        <div class="interaction-message-meta">
          <NTag size="small" :bordered="false">{{ messageTypeMeta(activeLog).label }}</NTag>
          <span>{{ formatTimestamp(activeLog.timestamp) }}</span>
          <span>{{ directionLabel(activeLog.direction) }}</span>
          <span>设备 {{ activeLog.deviceId }}</span>
          <span>通道 {{ activeLog.channelId ?? '—' }}</span>
        </div>
        <pre class="interaction-message-content">{{ activeLog.message }}</pre>
      </template>
    </NModal>
  </section>
</template>
