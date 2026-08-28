<script setup lang="ts">
  import { onMounted } from 'vue';
  import { NAlert, NButton, NCard, NTag, useMessage } from 'naive-ui';

  import MediaSourceForm from '@/features/media/components/media-source-form.vue';
  import MediaStatusPanel from '@/features/media/components/media-status-panel.vue';
  import { MediaSourceStatus, useMediaStore } from '@/features/media';

  const store = useMediaStore();
  const message = useMessage();

  onMounted(async () => {
    const result = await store.initialize();
    if (!result.ok) {
      message.error(result.message);
    }
  });

  async function runOperation(
    operation: () => Promise<{ ok: true } | { ok: false; message: string }>,
    successMessage?: string,
  ): Promise<void> {
    const result = await operation();
    if (!result.ok) {
      message.error(result.message);
      return;
    }
    if (successMessage !== undefined) {
      message.success(successMessage);
    }
  }
</script>

<template>
  <section class="page-shell media-page" aria-labelledby="media-page-title">
    <header class="page-header compact-header media-page-header">
      <div>
        <p class="eyebrow">GLOBAL MEDIA</p>
        <h1 id="media-page-title">音视频源</h1>
        <p>所有模拟设备和通道共享当前唯一音视频源；MP4 探测和播放由 Rust 媒体服务提供。</p>
      </div>
      <div class="media-header-status">
        <span>Source Status</span>
        <NTag
          :type="store.runtimeStatus.sourceStatus === MediaSourceStatus.Error ? 'error' : 'success'"
          round
        >
          {{ store.runtimeStatus.sourceStatus }}
        </NTag>
      </div>
    </header>

    <NAlert v-if="store.serviceError !== null" type="error" class="media-service-alert">
      {{ store.serviceError }}
    </NAlert>

    <NCard class="media-workbench" content-style="padding: 0;">
      <div class="media-workbench-body">
        <MediaSourceForm
          v-model:config="store.draftConfig"
          :video-devices="store.videoDevices"
          :audio-devices="store.audioDevices"
          :capabilities="store.videoCapabilities"
          :field-errors="store.fieldErrors"
          :is-probing="store.isProbing"
          :is-refreshing-devices="store.isRefreshingDevices"
          @source-type-change="store.setSourceType"
          @select-mp4="runOperation(store.selectMp4)"
          @probe-mp4="runOperation(store.probeCurrentMp4, '媒体信息检测完成。')"
          @refresh-devices="runOperation(store.refreshCaptureDevices, '设备列表已刷新。')"
          @video-device-change="store.setVideoDevice"
          @video-resolution-change="store.setVideoResolution"
          @select-recording-directory="runOperation(store.selectRecordingDirectory)"
        />
        <MediaStatusPanel
          :runtime-status="store.runtimeStatus"
          :probe-result="store.probeResult"
          :is-preview-pending="store.isPreviewPending"
          :can-start-preview="store.canStartPreview"
          @start-preview="runOperation(store.startPreview, '预览已启动。')"
          @stop-preview="runOperation(store.stopPreview, '预览已停止。')"
        />
      </div>

      <footer class="media-action-bar">
        <span>{{
          store.hasUnsavedChanges ? '当前草稿包含未保存修改' : '配置已与保存版本同步'
        }}</span>
        <div>
          <NButton
            data-testid="reset-media"
            @click="runOperation(store.resetDraft, '已恢复保存配置。')"
          >
            重置
          </NButton>
          <NButton
            type="info"
            :loading="store.isApplying"
            data-testid="apply-media"
            @click="runOperation(store.applyDraft, '当前配置已应用到 Mock Runtime。')"
          >
            应用
          </NButton>
          <NButton
            type="primary"
            :loading="store.isSaving"
            data-testid="save-media"
            @click="runOperation(store.saveDraft, '全局媒体配置已保存。')"
          >
            保存
          </NButton>
        </div>
      </footer>
    </NCard>
  </section>
</template>
