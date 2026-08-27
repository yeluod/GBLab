<script setup lang="ts">
  import { computed } from 'vue';
  import { NButton, NMenu, type MenuOption } from 'naive-ui';
  import { useRoute, useRouter } from 'vue-router';

  import { useSimulatorStore } from '@/features/simulator';

  const route = useRoute();
  const router = useRouter();
  const store = useSimulatorStore();
  const menuOptions: MenuOption[] = [
    { label: '运行总览', key: 'Overview' },
    { label: '设备管理', key: 'Devices' },
    { label: '交互日志', key: 'InteractionLogs' },
    { label: '全局配置', key: 'GlobalSettings' },
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
        <span class="brand-mark" aria-hidden="true">GB</span>
        <div>
          <strong>GBLab</strong>
          <span>Device Simulator</span>
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
          >{{ store.hasCompletedBatchAdd ? '设备已批量添加' : '批量添加设备' }}</NButton
        >
        <span>JSON 配置 · SIP 运行状态仅驻留内存</span>
      </div>
    </aside>

    <main class="app-main">
      <slot />
    </main>
  </div>
</template>
