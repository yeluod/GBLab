<script setup lang="ts">
  import { computed, ref, watch } from 'vue';
  import { NButton, NEmpty, NSelect, NSlider, NSpin, NSwitch, NTag } from 'naive-ui';

  import { MediaSourceStatus, type MediaRuntimeStatus } from '@/features/media';
  import AppIcon from '@/shared/components/app-icon.vue';

  const props = defineProps<{
    runtimeStatus: MediaRuntimeStatus;
    isPreviewPending: boolean;
    canStartPreview: boolean;
    previewFrame?: { width: number; height: number; rgba: Uint8Array } | null;
  }>();
  const emit = defineEmits<{
    startPreview: [];
    stopPreview: [];
    pausePreview: [];
    resumePreview: [];
    stepFrame: [];
    seek: [positionSeconds: number];
    playbackRateChange: [rate: number];
    audioControlChange: [muted: boolean, volume: number];
  }>();

  const sourceStatusText: Record<MediaSourceStatus, string> = {
    [MediaSourceStatus.Unconfigured]: '未配置',
    [MediaSourceStatus.Loading]: '加载中',
    [MediaSourceStatus.Ready]: '就绪',
    [MediaSourceStatus.Previewing]: '预览中',
    [MediaSourceStatus.Paused]: '已暂停',
    [MediaSourceStatus.Stopped]: '已停止',
    [MediaSourceStatus.Error]: '错误',
    [MediaSourceStatus.Unavailable]: '不可用',
  };
  const statusTagType = computed(() => {
    if (props.runtimeStatus.sourceStatus === MediaSourceStatus.Error) return 'error';
    if (props.runtimeStatus.sourceStatus === MediaSourceStatus.Previewing) return 'success';
    if (props.runtimeStatus.sourceStatus === MediaSourceStatus.Loading) return 'warning';
    return 'info';
  });
  const audioSinkStatusText: Record<
    NonNullable<MediaRuntimeStatus['audioSink']>['status'],
    string
  > = {
    unavailable: '不可用',
    paused: '已暂停',
    playing: '播放中',
    error: '错误',
  };
  const previewTitle = 'MP4 预览';
  const previewEmptyDescription = '选择并检测 MP4 后可开始预览';
  const previewCanvas = ref<HTMLCanvasElement | null>(null);
  const seekPosition = ref(0);
  const isSeekEditing = ref(false);
  const rateOptions = [0.25, 0.5, 1, 1.5, 2, 4].map((value) => ({
    label: `${value}x`,
    value,
  }));
  watch(
    () => props.runtimeStatus.positionSeconds,
    (value) => {
      if (!isSeekEditing.value) {
        seekPosition.value = value;
      }
    },
  );

  function commitSeek(): void {
    isSeekEditing.value = false;
    emit('seek', seekPosition.value);
  }
  function drawFrame(
    frame: { width: number; height: number; rgba: Uint8Array } | null | undefined,
  ) {
    if (frame === null || frame === undefined || previewCanvas.value === null) return;
    const canvas = previewCanvas.value;
    canvas.width = frame.width;
    canvas.height = frame.height;
    const context = canvas.getContext('2d');
    if (context === null) return;
    const pixels = new Uint8ClampedArray(frame.rgba.byteLength);
    pixels.set(frame.rgba);
    context.putImageData(new ImageData(pixels, frame.width, frame.height), 0, 0);
  }
  watch(
    () => props.previewFrame,
    (frame) => drawFrame(frame),
  );
  watch(previewCanvas, () => drawFrame(props.previewFrame), { flush: 'post' });

  function durationLabel(durationSeconds: number | null): string {
    if (durationSeconds === null) return '实时';
    const totalSeconds = Math.max(0, Math.floor(durationSeconds));
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    return `${minutes}:${String(seconds).padStart(2, '0')}`;
  }
</script>

<template>
  <aside class="media-monitor-panel" aria-label="媒体预览与运行状态">
    <section class="preview-section">
      <div class="media-section-heading compact">
        <div>
          <span class="section-kicker">PREVIEW</span>
          <h2>预览</h2>
        </div>
        <NTag :type="statusTagType" size="small" round>
          {{ sourceStatusText[runtimeStatus.sourceStatus] }}
        </NTag>
      </div>

      <div class="mock-preview-surface" :class="`status-${runtimeStatus.sourceStatus}`">
        <NSpin
          v-if="isPreviewPending || runtimeStatus.sourceStatus === MediaSourceStatus.Loading"
        />
        <template
          v-else-if="
            runtimeStatus.sourceStatus === MediaSourceStatus.Previewing ||
            runtimeStatus.sourceStatus === MediaSourceStatus.Paused
          "
        >
          <canvas ref="previewCanvas" class="preview-canvas" aria-label="视频预览画面"></canvas>
          <div class="preview-caption">
            <strong>{{ previewTitle }}</strong>
            <span>{{ runtimeStatus.sourceLabel }}</span>
          </div>
        </template>
        <NEmpty v-else :description="previewEmptyDescription" />
      </div>

      <div class="preview-actions">
        <NButton
          v-if="
            runtimeStatus.sourceStatus !== MediaSourceStatus.Previewing &&
            runtimeStatus.sourceStatus !== MediaSourceStatus.Paused
          "
          type="primary"
          :disabled="!canStartPreview"
          :loading="isPreviewPending"
          data-testid="start-preview"
          @click="emit('startPreview')"
        >
          <template #icon><AppIcon icon="play" /></template>
          开始预览
        </NButton>
        <NButton
          v-if="runtimeStatus.sourceStatus === MediaSourceStatus.Previewing"
          :loading="isPreviewPending"
          @click="emit('pausePreview')"
        >
          暂停
        </NButton>
        <NButton
          v-if="runtimeStatus.sourceStatus === MediaSourceStatus.Paused"
          type="primary"
          :loading="isPreviewPending"
          @click="emit('resumePreview')"
        >
          继续
        </NButton>
        <NButton
          v-if="
            runtimeStatus.sourceStatus === MediaSourceStatus.Previewing ||
            runtimeStatus.sourceStatus === MediaSourceStatus.Paused
          "
          type="warning"
          :loading="isPreviewPending"
          data-testid="stop-preview"
          @click="emit('stopPreview')"
        >
          <template #icon><AppIcon icon="stop" /></template>
          停止预览
        </NButton>
      </div>

      <div
        v-if="
          runtimeStatus.sourceStatus === MediaSourceStatus.Previewing ||
          runtimeStatus.sourceStatus === MediaSourceStatus.Paused
        "
        class="playback-controls"
      >
        <div class="playback-seek-row">
          <NSlider
            v-model:value="seekPosition"
            :min="0"
            :max="runtimeStatus.durationSeconds ?? 0"
            :step="0.1"
            @dragstart="isSeekEditing = true"
            @dragend="commitSeek"
          />
          <span
            >{{ durationLabel(seekPosition) }} /
            {{ durationLabel(runtimeStatus.durationSeconds) }}</span
          >
        </div>
        <div class="playback-action-row">
          <NButton size="small" @click="emit('seek', Math.max(0, seekPosition - 10))"
            >-10 秒</NButton
          >
          <NButton size="small" type="primary" @click="emit('seek', seekPosition)">跳转</NButton>
          <NButton
            size="small"
            @click="
              emit(
                'seek',
                Math.min(runtimeStatus.durationSeconds ?? seekPosition, seekPosition + 10),
              )
            "
          >
            +10 秒
          </NButton>
          <NButton
            size="small"
            :disabled="runtimeStatus.sourceStatus !== MediaSourceStatus.Paused"
            @click="emit('stepFrame')"
          >
            单帧
          </NButton>
          <NSelect
            :value="runtimeStatus.playbackRate"
            :options="rateOptions"
            size="small"
            @update:value="emit('playbackRateChange', $event)"
          />
          <template v-if="runtimeStatus.audio !== null">
            <span class="audio-control-label" title="控制本地音频监听输出">静音</span>
            <NSwitch
              :value="runtimeStatus.muted"
              @update:value="emit('audioControlChange', $event, runtimeStatus.volume)"
            />
            <span class="audio-control-label">音量</span>
            <NSlider
              class="audio-volume-slider"
              :value="runtimeStatus.volume"
              :min="0"
              :max="1"
              :step="0.05"
              @update:value="emit('audioControlChange', runtimeStatus.muted, $event)"
            />
          </template>
        </div>
      </div>
    </section>

    <div class="media-detail-grid">
      <section class="runtime-section media-detail-card">
        <div class="media-section-heading compact">
          <div>
            <span class="section-kicker">RUNTIME STATUS</span>
            <h2>运行状态</h2>
          </div>
        </div>
        <dl class="runtime-status-grid">
          <dt>Source</dt>
          <dd>{{ sourceStatusText[runtimeStatus.sourceStatus] }}</dd>
          <dt>Live</dt>
          <dd>{{ runtimeStatus.activeLiveConsumers }}</dd>
          <dt>Decoded Frames</dt>
          <dd>{{ runtimeStatus.decodedFrames }}</dd>
          <dt>Read / Preview</dt>
          <dd>
            {{ runtimeStatus.metrics.videoPacketsRead }} /
            {{ runtimeStatus.metrics.videoPreviewFrames }}
          </dd>
          <dt>Encoded Video</dt>
          <dd>{{ runtimeStatus.metrics.videoPacketsEncoded }}</dd>
          <dt>Audio Packets / PCM</dt>
          <dd>
            {{ runtimeStatus.metrics.audioPacketsRead }} /
            {{ runtimeStatus.metrics.audioFramesDecoded }}
          </dd>
          <dt>Audio RMS / Peak</dt>
          <dd>
            {{ runtimeStatus.metrics.audioRms.toFixed(3) }} /
            {{ runtimeStatus.metrics.audioPeak.toFixed(3) }}
          </dd>
          <template v-if="runtimeStatus.audioSink !== null">
            <dt>Audio Sink</dt>
            <dd>
              {{ audioSinkStatusText[runtimeStatus.audioSink.status] }} · 队列
              {{ runtimeStatus.audioSink.queuedSamples }} · 已播放
              {{ runtimeStatus.audioSink.playedSamples }} · 欠载
              {{ runtimeStatus.audioSink.underruns }} · 丢弃
              {{ runtimeStatus.audioSink.droppedSamples }}
            </dd>
            <template v-if="runtimeStatus.audioSink.lastError !== null">
              <dt>Audio Sink Error</dt>
              <dd class="runtime-error">{{ runtimeStatus.audioSink.lastError }}</dd>
            </template>
          </template>
          <template v-if="runtimeStatus.pipelineErrorMessage !== null">
            <dt>Pipeline Error</dt>
            <dd class="runtime-error">{{ runtimeStatus.pipelineErrorMessage }}</dd>
          </template>
        </dl>
      </section>
    </div>
  </aside>
</template>

<style scoped>
  .playback-controls {
    display: grid;
    gap: 8px;
    margin-top: 10px;
  }

  .playback-seek-row,
  .playback-action-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .playback-seek-row .n-slider {
    flex: 1;
    min-width: 120px;
  }

  .playback-seek-row > span {
    flex: 0 0 auto;
    color: #64748b;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    font-size: 0.78rem;
    white-space: nowrap;
  }

  .playback-action-row .n-select {
    width: 78px;
  }

  .audio-control-label {
    flex: 0 0 auto;
    color: #64748b;
    font-size: 0.78rem;
    white-space: nowrap;
  }

  .audio-volume-slider {
    min-width: 100px;
    flex: 1 1 160px;
  }

  @media (max-width: 920px) {
    .playback-seek-row,
    .playback-action-row {
      flex-wrap: wrap;
    }

    .playback-seek-row .n-slider {
      flex-basis: 100%;
    }
  }
</style>
