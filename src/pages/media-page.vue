<script setup lang="ts">
  import { onMounted } from 'vue';
  import { NAlert, NButton, NCard, NTag, useMessage } from 'naive-ui';

  import MediaSourceForm from '@/features/media/components/media-source-form.vue';
  import MediaStatusPanel from '@/features/media/components/media-status-panel.vue';
  import { MediaSourceStatus, MediaSourceType, useMediaStore } from '@/features/media';
  import AppIcon from '@/shared/components/app-icon.vue';

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
        <p class="eyebrow">GLOBAL MEDIA SOURCE</p>
        <h1 id="media-page-title">音视频源</h1>
        <p>所有模拟设备和通道共享当前唯一音视频源；MP4 探测和播放由 Rust 媒体服务提供。</p>
      </div>
      <div class="media-header-status">
        <span>媒体状态</span>
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
    <NAlert
      v-else-if="
        store.draftConfig.source.type === MediaSourceType.Camera &&
        !store.isInitializing &&
        !store.isRefreshingDevices &&
        store.videoDevices.length === 0 &&
        store.audioDevices.length === 0
      "
      type="warning"
      class="media-service-alert"
    >
      系统未发现可用的摄像头或麦克风，请检查设备连接和系统隐私权限。
    </NAlert>

    <NCard class="media-workbench" content-style="padding: 0;">
      <div class="media-workbench-body">
        <MediaSourceForm
          v-model:config="store.draftConfig"
          :video-devices="store.videoDevices"
          :audio-devices="store.audioDevices"
          :capabilities="store.videoCapabilities"
          :supported-video-codecs="store.supportedVideoCodecs"
          :video-encoder-capabilities="store.videoEncoderCapabilities"
          :field-errors="store.fieldErrors"
          :capability-error="store.capabilityError"
          :encoder-capability-error="store.encoderCapabilityError"
          :is-probing="store.isProbing"
          :is-refreshing-devices="store.isRefreshingDevices"
          :is-loading-video-capabilities="store.isLoadingVideoCapabilities"
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
          :source-type="store.draftConfig.source.type"
          :preview-frame="store.previewFrame"
          @start-preview="runOperation(store.startPreview, '预览已启动。')"
          @stop-preview="runOperation(store.stopPreview, '预览已停止。')"
          @pause-preview="runOperation(store.pausePreview, '预览已暂停。')"
          @resume-preview="runOperation(store.resumePreview, '预览已继续。')"
          @step-frame="runOperation(store.stepPreviewFrame)"
          @seek="runOperation(() => store.seekPreview($event), '播放位置已更新。')"
          @playback-rate-change="runOperation(() => store.setPlaybackRate($event))"
          @audio-control-change="
            (muted, volume) => runOperation(() => store.setAudioControl(muted, volume))
          "
          @audio-monitoring-change="
            (enabled) => runOperation(() => store.setAudioMonitoring(enabled))
          "
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
            <template #icon><AppIcon icon="reset" /></template>
            重置
          </NButton>
          <NButton
            type="info"
            :loading="store.isApplying"
            data-testid="apply-media"
            @click="runOperation(store.applyDraft, '当前配置已应用。')"
          >
            <template #icon><AppIcon icon="check" /></template>
            应用
          </NButton>
          <NButton
            type="primary"
            :loading="store.isSaving"
            data-testid="save-media"
            @click="runOperation(store.saveDraft, '全局媒体配置已保存。')"
          >
            <template #icon><AppIcon icon="save" /></template>
            保存
          </NButton>
        </div>
      </footer>
    </NCard>
  </section>
</template>
