<script setup lang="ts">
  import {
    NButton,
    NDivider,
    NForm,
    NFormItem,
    NInput,
    NInputGroup,
    NSelect,
    NSwitch,
  } from 'naive-ui';

  import type { GlobalMediaConfig } from '../types/media-config';
  import type { MediaFieldErrors } from '../stores/media-store';
  import AppIcon from '@/shared/components/app-icon.vue';

  const config = defineModel<GlobalMediaConfig>('config', { required: true });
  const props = defineProps<{
    fieldErrors: MediaFieldErrors;
    isProbing: boolean;
  }>();
  const emit = defineEmits<{
    selectMp4: [];
    probeMp4: [];
    selectRecordingDirectory: [];
  }>();

  const segmentOptions = [5, 10, 30, 60].map((value) => ({
    label: `${value} 分钟`,
    value,
  }));

  function validationProps(
    path: string,
  ): Record<string, never> | { validationStatus: 'error'; feedback: string } {
    const error = props.fieldErrors[path];
    return error === undefined ? {} : { validationStatus: 'error', feedback: error };
  }
</script>

<template>
  <section class="media-config-panel" aria-labelledby="media-source-configuration-title">
    <div class="media-section-heading">
      <div>
        <span class="section-kicker">SOURCE CONFIGURATION</span>
        <h2 id="media-source-configuration-title">MP4 媒体源</h2>
      </div>
      <span class="shared-config-note">全局唯一配置</span>
    </div>

    <NForm label-placement="top" :model="config">
      <NDivider title-placement="left">MP4 文件</NDivider>
      <NFormItem label="文件" v-bind="validationProps('source.mp4.filePath')">
        <NInputGroup>
          <NInput
            v-model:value="config.source.mp4.filePath"
            placeholder="请选择 MP4 文件"
            readonly
          />
          <NButton data-testid="select-mp4" @click="emit('selectMp4')">
            <template #icon><AppIcon icon="file" /></template>
            选择文件
          </NButton>
          <NButton data-testid="probe-mp4" :loading="isProbing" @click="emit('probeMp4')">
            <template #icon><AppIcon icon="refresh" /></template>
            重新检测
          </NButton>
        </NInputGroup>
      </NFormItem>
      <NFormItem label="循环播放" :show-feedback="false">
        <NSwitch v-model:value="config.source.mp4.isLooping" />
      </NFormItem>

      <NDivider title-placement="left">本地录像</NDivider>
      <NFormItem label="启用录像" :show-feedback="false">
        <NSwitch v-model:value="config.recording.isEnabled" />
      </NFormItem>
      <div class="media-form-grid recording-grid">
        <NFormItem label="录像目录" v-bind="validationProps('recording.directory')">
          <NInputGroup>
            <NInput
              v-model:value="config.recording.directory"
              placeholder="请选择录像目录"
              :disabled="!config.recording.isEnabled"
              readonly
            />
            <NButton
              data-testid="select-recording-directory"
              :disabled="!config.recording.isEnabled"
              @click="emit('selectRecordingDirectory')"
            >
              <template #icon><AppIcon icon="file" /></template>
              选择
            </NButton>
          </NInputGroup>
        </NFormItem>
        <NFormItem label="分段时长">
          <NSelect
            v-model:value="config.recording.segmentDurationMinutes"
            :options="segmentOptions"
            :disabled="!config.recording.isEnabled"
          />
        </NFormItem>
      </div>
    </NForm>
  </section>
</template>
