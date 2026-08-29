<script setup lang="ts">
  import { onMounted } from 'vue';
  import { NButton, NCard, NStatistic, NTag } from 'naive-ui';
  import { useRouter } from 'vue-router';

  import { useSimulatorStore } from '@/features/simulator';
  import AppIcon from '@/shared/components/app-icon.vue';

  const router = useRouter();
  const store = useSimulatorStore();

  onMounted(async () => {
    await store.loadSipService();
    await store.loadDevices();
  });

  function navigateTo(name: 'Devices' | 'GlobalSettings'): void {
    void router.push({ name });
  }
</script>

<template>
  <section class="page-shell overview-page" aria-labelledby="overview-title">
    <header class="page-header">
      <div>
        <p class="eyebrow">SYSTEM OVERVIEW</p>
        <h1 id="overview-title">运行总览</h1>
        <p>集中查看设备注册、平台订阅与全局媒体源的当前运行状态。</p>
      </div>
      <NTag :type="store.isRegistrationActive ? 'success' : 'default'" :bordered="false">
        <template #icon
          ><AppIcon :icon="store.isRegistrationActive ? 'radio' : 'stop'" :size="14"
        /></template>
        {{ store.isRegistrationActive ? '注册运行中' : '未运行' }}
      </NTag>
    </header>

    <div class="metric-grid">
      <NCard class="overview-metric-card">
        <NStatistic label="模拟设备" :value="store.devices.length" />
        <p class="metric-caption">批量新建设备默认未注册</p>
      </NCard>
      <NCard class="overview-metric-card">
        <NStatistic label="已注册设备" :value="store.registeredDeviceCount" />
        <p class="metric-caption">运行时状态，不写入 JSON 配置</p>
      </NCard>
      <NCard class="overview-metric-card">
        <NStatistic label="活跃订阅" :value="store.activeSubscriptionCount" />
        <p class="metric-caption">目录、报警与移动位置</p>
      </NCard>
    </div>

    <NCard class="sip-summary-card" title="共享 SIP 服务">
      <div class="sip-summary-content">
        <div>
          <strong>{{ store.sipService.uri }}</strong>
          <span>{{ store.sipService.transport }} · 平台 {{ store.sipService.platformId }}</span>
        </div>
        <NButton secondary @click="navigateTo('GlobalSettings')">
          <template #icon><AppIcon icon="settings" /></template>
          查看全局配置
        </NButton>
      </div>
    </NCard>

    <NCard title="开始模拟" class="overview-action-card">
      <p>通过设备管理页批量新增设备并启动完整注册生命周期；注册状态与交互日志仅保存在运行内存。</p>
      <NButton type="primary" @click="navigateTo('Devices')">
        <template #icon><AppIcon icon="server" /></template>
        进入设备管理
      </NButton>
    </NCard>
  </section>
</template>
