<script setup lang="ts">
  import { computed, h } from 'vue';
  import { NButton, NMenu, type MenuOption } from 'naive-ui';
  import { useRoute, useRouter } from 'vue-router';

  import { useSimulatorStore } from '@/features/simulator';
  import AppIcon from '@/shared/components/app-icon.vue';

  const route = useRoute();
  const router = useRouter();
  const store = useSimulatorStore();
  const menuOptions: MenuOption[] = [
    { label: '运行总览', key: 'Overview', icon: () => h(AppIcon, { icon: 'gauge', size: 18 }) },
    { label: '设备管理', key: 'Devices', icon: () => h(AppIcon, { icon: 'server', size: 18 }) },
    {
      label: '设备模拟',
      key: 'Simulation',
      icon: () => h(AppIcon, { icon: 'crosshair', size: 18 }),
    },
    {
      label: '查询控制台',
      key: 'QueryConsole',
      icon: () => h(AppIcon, { icon: 'search', size: 18 }),
    },
    {
      label: '场景与故障',
      key: 'Scenarios',
      icon: () => h(AppIcon, { icon: 'clock', size: 18 }),
    },
    {
      label: '运行观测',
      key: 'RuntimeObservability',
      icon: () => h(AppIcon, { icon: 'rows', size: 18 }),
    },
    {
      label: '交互日志',
      key: 'InteractionLogs',
      icon: () => h(AppIcon, { icon: 'logs', size: 18 }),
    },
    { label: '音视频源', key: 'Media', icon: () => h(AppIcon, { icon: 'video', size: 18 }) },
    {
      label: '全局配置',
      key: 'GlobalSettings',
      icon: () => h(AppIcon, { icon: 'settings', size: 18 }),
    },
  ];
  const activeMenuKey = computed(() => String(route.name));

  function handleMenuUpdate(key: string): void {
    void router.push({ name: key });
  }

  function openDeviceManagement(): void {
    void router.push({ name: 'Devices' });
  }
</script>

<template>
  <div class="app-shell">
    <aside class="app-sidebar" aria-label="主导航">
      <div class="brand">
        <span class="brand-mark" aria-hidden="true"><span>GB</span><i></i></span>
        <div>
          <strong>GBLab</strong>
          <span>GB28181 Simulator</span>
        </div>
      </div>

      <nav class="main-nav">
        <NMenu :value="activeMenuKey" :options="menuOptions" @update:value="handleMenuUpdate" />
      </nav>

      <div class="sidebar-footer">
        <NButton
          block
          type="primary"
          :disabled="store.hasCompletedBatchAdd"
          @click="openDeviceManagement"
        >
          <template #icon><AppIcon icon="plus" /></template>
          {{ store.hasCompletedBatchAdd ? '设备已批量添加' : '批量添加设备' }}
        </NButton>
        <div class="sidebar-runtime">
          <span class="runtime-dot" :class="{ 'is-active': store.isRegistrationActive }"></span>
          <span>{{ store.isRegistrationActive ? '注册运行中' : '注册未启动' }}</span>
        </div>
        <span>JSON 配置 · SIP 运行状态仅驻留内存</span>
      </div>
    </aside>

    <main class="app-main">
      <slot />
    </main>
  </div>
</template>
