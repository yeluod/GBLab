<script setup lang="ts">
  import { onMounted, reactive, watch } from 'vue';
  import {
    NAlert,
    NButton,
    NCard,
    NForm,
    NFormItem,
    NInput,
    NInputNumber,
    NSelect,
    NTabPane,
    NTabs,
    type SelectOption,
    useMessage,
  } from 'naive-ui';

  import { useSimulatorStore, type SipServiceConfig } from '@/features/simulator';
  import AppIcon from '@/shared/components/app-icon.vue';

  const store = useSimulatorStore();
  const message = useMessage();
  const signalCharsetOptions: SelectOption[] = [
    { label: 'GB2312（默认）', value: 'GB2312' },
    { label: 'GBK', value: 'GBK' },
    { label: 'UTF-8', value: 'UTF-8' },
  ];
  const formModel = reactive<SipServiceConfig>({ ...store.sipService });
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
    message.success('全局配置已写入 JSON 文件。');
  }
</script>

<template>
  <section class="page-shell global-settings-page" aria-labelledby="global-settings-title">
    <header class="page-header compact-header">
      <div>
        <p class="eyebrow">SHARED RUNTIME CONFIGURATION</p>
        <h1 id="global-settings-title">全局配置</h1>
        <p>统一维护平台连接与设备运行参数，所有模拟设备共同使用。</p>
      </div>
      <NButton
        type="primary"
        :loading="store.isSipServiceSaving"
        :disabled="store.isSipServiceLoading || store.isRegistrationActive"
        @click="handleSave"
      >
        <template #icon><AppIcon icon="save" /></template>
        保存配置
      </NButton>
    </header>

    <NCard class="form-card global-settings-card">
      <NAlert v-if="store.isRegistrationActive" type="warning" class="settings-warning">
        请先完成全量停止注册，再修改全局配置。
      </NAlert>
      <NForm
        label-placement="top"
        :model="formModel"
        :disabled="store.isSipServiceLoading || store.isRegistrationActive"
      >
        <NTabs type="line" animated pane-class="settings-tab-pane">
          <NTabPane name="platform">
            <template #tab><AppIcon icon="globe" :size="15" /> 平台配置</template>
            <p class="settings-section-description">
              配置模拟设备连接的唯一 GB28181 平台、认证信息与本地网络参数。
            </p>
            <div class="form-grid">
              <NFormItem label="SIP 地址" path="uri">
                <NInput v-model:value="formModel.uri" placeholder="sip:192.168.1.100:5060" />
              </NFormItem>
              <NFormItem label="传输协议">
                <NInput value="UDP（当前唯一支持）" readonly />
              </NFormItem>
              <NFormItem label="平台 ID" path="platformId">
                <NInput v-model:value="formModel.platformId" maxlength="20" />
              </NFormItem>
              <NFormItem label="本地监听地址" path="localBindAddress">
                <NInput v-model:value="formModel.localBindAddress" placeholder="0.0.0.0" />
              </NFormItem>
              <NFormItem label="对外通信地址" path="advertisedAddress">
                <NInput
                  v-model:value="formModel.advertisedAddress"
                  placeholder="留空时根据到平台的路由自动探测"
                />
              </NFormItem>
              <NFormItem label="本地 SIP 端口" path="localPort">
                <NInputNumber
                  v-model:value="formModel.localPort"
                  :min="1"
                  :max="65535"
                  :show-button="false"
                />
              </NFormItem>
              <NFormItem label="域" path="domain">
                <NInput v-model:value="formModel.domain" />
              </NFormItem>
            </div>
          </NTabPane>

          <NTabPane name="device">
            <template #tab><AppIcon icon="settings" :size="15" /> 设备配置</template>
            <p class="settings-section-description">
              配置全部模拟设备共享的认证密码、XML 信令字符集、注册周期和心跳行为。
            </p>
            <div class="form-grid">
              <NFormItem label="认证密码（可选）" path="password">
                <NInput
                  v-model:value="formModel.password"
                  :maxlength="128"
                  placeholder="留空表示平台无需 Digest 认证"
                />
                <template #feedback>仅在平台启用 SIP Digest 认证时填写。</template>
              </NFormItem>
              <NFormItem label="信令字符集" path="signalCharset">
                <NSelect v-model:value="formModel.signalCharset" :options="signalCharsetOptions" />
                <template #feedback>
                  XML 声明、Content-Type 与实际报文字节统一使用该字符集。
                </template>
              </NFormItem>
              <NFormItem label="注册有效期（秒）" path="registerExpires">
                <NInputNumber
                  v-model:value="formModel.registerExpires"
                  :min="1"
                  :max="86400"
                  :show-button="false"
                />
              </NFormItem>
              <NFormItem label="心跳间隔（秒）" path="keepaliveInterval">
                <NInputNumber
                  v-model:value="formModel.keepaliveInterval"
                  :min="1"
                  :max="3600"
                  :show-button="false"
                />
              </NFormItem>
            </div>
          </NTabPane>
        </NTabs>
      </NForm>
    </NCard>
  </section>
</template>
