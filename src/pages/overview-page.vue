<script setup lang="ts">
  import { NButton, NCard, NStatistic, NTag } from 'naive-ui';
  import { useRouter } from 'vue-router';

  import { useSimulatorStore } from '@/features/simulator';

  const router = useRouter();
  const store = useSimulatorStore();

  function navigateTo(name: 'Devices' | 'SipService'): void {
    void router.push({ name });
  }
</script>

<template>
  <section class="page-shell overview-page" aria-labelledby="overview-title">
    <header class="page-header">
      <div>
        <p class="eyebrow">GB28181 DEVICE SIMULATOR</p>
        <h1 id="overview-title">运行总览</h1>
        <p>单一 SIP 服务下的多设备注册与通道交互演示。</p>
      </div>
      <NTag type="success" round>静态演示模式</NTag>
    </header>

    <div class="metric-grid">
      <NCard>
        <NStatistic label="模拟设备" :value="store.devices.length" />
        <p class="metric-caption">批量新建设备默认未注册</p>
      </NCard>
      <NCard>
        <NStatistic label="已注册设备" :value="store.registeredDeviceCount" />
        <p class="metric-caption">运行时状态，不写入 JSON 配置</p>
      </NCard>
      <NCard>
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
        <NButton secondary @click="navigateTo('SipService')">查看 SIP 服务</NButton>
      </div>
    </NCard>

    <NCard title="开始模拟" class="overview-action-card">
      <p>通过设备管理页批量新增、编辑并注册模拟设备；运行时状态在刷新应用后恢复为演示数据。</p>
      <NButton type="primary" @click="navigateTo('Devices')">进入设备管理</NButton>
    </NCard>
  </section>
</template>
