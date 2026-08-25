<script setup lang="ts">
  import { onMounted, reactive, watch } from 'vue';
  import {
    NButton,
    NCard,
    NForm,
    NFormItem,
    NInput,
    NInputNumber,
    NSelect,
    useMessage,
  } from 'naive-ui';

  import { useSimulatorStore, type SipServiceConfig } from '@/features/simulator';

  const store = useSimulatorStore();
  const message = useMessage();
  const formModel = reactive<SipServiceConfig>({ ...store.sipService });
  const transportOptions = [
    { label: 'UDP', value: 'UDP' },
    { label: 'TCP', value: 'TCP' },
  ];

  watch(
    () => store.sipService,
    (config) => Object.assign(formModel, config),
  );

  onMounted(async () => {
    const result = await store.loadSipService();
    if (!result.ok) {
      message.error(`读取配置失败：${result.message}`);
    }
  });

  async function handleSave(): Promise<void> {
    const result = await store.saveSipService({ ...formModel });
    if (!result.ok) {
      message.error(result.message);
      return;
    }
    message.success('唯一 SIP 服务配置已写入 JSON 文件。');
  }
</script>

<template>
  <section class="page-shell" aria-labelledby="sip-service-title">
    <header class="page-header compact-header">
      <div>
        <p class="eyebrow">SHARED SIP SERVICE</p>
        <h1 id="sip-service-title">SIP 服务</h1>
        <p>应用内只能维护一份服务配置，所有模拟设备共同使用。</p>
      </div>
      <NButton
        type="primary"
        :loading="store.isSipServiceSaving"
        :disabled="store.isSipServiceLoading"
        @click="handleSave"
      >
        保存配置
      </NButton>
    </header>

    <NCard class="form-card">
      <NForm label-placement="top" :model="formModel" :disabled="store.isSipServiceLoading">
        <div class="form-grid">
          <NFormItem label="SIP 地址" path="uri">
            <NInput v-model:value="formModel.uri" placeholder="sip:192.168.1.100:5060" />
          </NFormItem>
          <NFormItem label="传输协议" path="transport">
            <NSelect v-model:value="formModel.transport" :options="transportOptions" />
          </NFormItem>
          <NFormItem label="平台 ID" path="platformId">
            <NInput v-model:value="formModel.platformId" maxlength="20" />
          </NFormItem>
          <NFormItem label="认证密码" path="password">
            <NInput
              v-model:value="formModel.password"
              :maxlength="128"
              placeholder="请输入 SIP Digest 认证密码"
            />
          </NFormItem>
          <NFormItem label="域" path="domain">
            <NInput v-model:value="formModel.domain" />
          </NFormItem>
          <NFormItem label="注册有效期（秒）" path="registerExpires">
            <NInputNumber v-model:value="formModel.registerExpires" :min="1" :show-button="false" />
          </NFormItem>
          <NFormItem label="心跳间隔（秒）" path="keepaliveInterval">
            <NInputNumber
              v-model:value="formModel.keepaliveInterval"
              :min="1"
              :show-button="false"
            />
          </NFormItem>
        </div>
      </NForm>
    </NCard>
  </section>
</template>
