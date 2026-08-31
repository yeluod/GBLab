<script setup lang="ts">
  import { computed, h } from 'vue';
  import { NMenu, type MenuOption } from 'naive-ui';
  import { useRoute, useRouter } from 'vue-router';

  import AppIcon from '@/shared/components/app-icon.vue';

  const route = useRoute();
  const router = useRouter();
  const menuOptions: MenuOption[] = [
    { label: '运行总览', key: 'Overview', icon: () => h(AppIcon, { icon: 'gauge', size: 18 }) },
    { label: '设备管理', key: 'Devices', icon: () => h(AppIcon, { icon: 'server', size: 18 }) },
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
    </aside>

    <main class="app-main">
      <slot />
    </main>
  </div>
</template>
