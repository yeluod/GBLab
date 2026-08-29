<script setup lang="ts">
  import { computed, ref, watch } from 'vue';
  import { NButton, NEmpty, NSelect, NSlider, NSpin, NSwitch, NTag } from 'naive-ui';

  import {
    AudioCodec,
    MediaSourceStatus,
    MediaSourceType,
    VideoCodec,
    type MediaProbeResult,
    type MediaRuntimeStatus,
  } from '@/features/media';
  import AppIcon from '@/shared/components/app-icon.vue';

  const props = defineProps<{
    runtimeStatus: MediaRuntimeStatus;
    probeResult: MediaProbeResult | null;
    isPreviewPending: boolean;
    canStartPreview: boolean;
    sourceType: MediaSourceType;
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
    audioMonitoringChange: [enabled: boolean];
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
  const displayedVideo = computed(() => props.probeResult?.video ?? props.runtimeStatus.video);
  const displayedAudio = computed(() =>
    props.probeResult === null ? props.runtimeStatus.audio : props.probeResult.audio,
  );
  const previewTitle = computed(() =>
    props.sourceType === MediaSourceType.Camera ? '摄像头预览' : 'MP4 预览',
  );
  const previewEmptyDescription = computed(() =>
    props.sourceType === MediaSourceType.Camera
      ? '选择摄像头和采集参数后可开始预览'
      : '选择并检测 MP4 后可开始预览',
  );
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

  function codecLabel(codec: VideoCodec | AudioCodec): string {
    const labels: Record<VideoCodec | AudioCodec, string> = {
      [VideoCodec.H264]: 'H.264',
      [VideoCodec.H265]: 'H.265',
      [AudioCodec.G711A]: 'G.711 A-law',
      [AudioCodec.G711U]: 'G.711 μ-law',
      [AudioCodec.Aac]: 'AAC',
    };
    return labels[codec];
  }

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
          sourceType === MediaSourceType.Mp4 &&
          (runtimeStatus.sourceStatus === MediaSourceStatus.Previewing ||
            runtimeStatus.sourceStatus === MediaSourceStatus.Paused)
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
        </div>
      </div>

      <div v-if="runtimeStatus.audio !== null" class="audio-controls">
        <span title="控制本地音频监听输出">静音状态</span>
        <NSwitch
          :value="runtimeStatus.muted"
          @update:value="emit('audioControlChange', $event, runtimeStatus.volume)"
        />
        <span>音量</span>
        <NSlider
          :value="runtimeStatus.volume"
          :min="0"
          :max="1"
          :step="0.05"
          @update:value="emit('audioControlChange', runtimeStatus.muted, $event)"
        />
      </div>
    </section>

    <div class="media-detail-grid">
      <section class="media-information-section media-detail-card">
        <div class="media-section-heading compact">
          <div>
            <span class="section-kicker">MEDIA INFORMATION</span>
            <h2>媒体信息</h2>
          </div>
        </div>
        <dl class="media-info-list">
          <template v-if="displayedVideo !== null">
            <dt>Video</dt>
            <dd>
              {{ codecLabel(displayedVideo.codec) }} · {{ displayedVideo.width }}×{{
                displayedVideo.height
              }}
              · {{ displayedVideo.framesPerSecond }} FPS
            </dd>
            <dt>Video Bitrate</dt>
            <dd>{{ displayedVideo.bitrateKbps }} Kbps</dd>
            <dt>Duration</dt>
            <dd>{{ durationLabel(displayedVideo.durationSeconds) }}</dd>
          </template>
          <template v-else>
            <dt>Video</dt>
            <dd>尚未检测</dd>
          </template>
          <template v-if="displayedAudio !== null">
            <dt>Audio</dt>
            <dd>
              {{ codecLabel(displayedAudio.codec) }} · {{ displayedAudio.sampleRate / 1000 }} kHz ·
              {{ displayedAudio.channels === 1 ? 'Mono' : 'Stereo' }}
            </dd>
            <dt>Audio Bitrate</dt>
            <dd>{{ displayedAudio.bitrateKbps }} Kbps</dd>
          </template>
          <template v-else>
            <dt>Audio</dt>
            <dd class="normal-empty-value">None（正常）</dd>
          </template>
        </dl>
      </section>

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
          <dt>Recorder consumers</dt>
          <dd>{{ runtimeStatus.activeRecorderConsumers }}</dd>
          <dt>Decoded Frames</dt>
          <dd>{{ runtimeStatus.decodedFrames }}</dd>
          <dt>Capture / Preview</dt>
          <dd>
            {{ runtimeStatus.metrics.videoPacketsCaptured }} /
            {{ runtimeStatus.metrics.videoPreviewFrames }}
          </dd>
          <dt>Encoded Video</dt>
          <dd>{{ runtimeStatus.metrics.videoPacketsEncoded }}</dd>
          <dt>Mic Packets / PCM</dt>
          <dd>
            {{ runtimeStatus.metrics.audioPacketsCaptured }} /
            {{ runtimeStatus.metrics.audioFramesDecoded }}
          </dd>
          <dt>Audio RMS / Peak</dt>
          <dd>
            {{ runtimeStatus.metrics.audioRms.toFixed(3) }} /
            {{ runtimeStatus.metrics.audioPeak.toFixed(3) }}
          </dd>
          <template v-if="sourceType === MediaSourceType.Camera && displayedAudio !== null">
            <dt>音频监听</dt>
            <dd>
              <NSwitch
                :value="runtimeStatus.audioMonitoring"
                @update:value="emit('audioMonitoringChange', $event)"
              />
            </dd>
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

  .audio-controls {
    display: grid;
    grid-template-columns: auto auto auto minmax(100px, 1fr);
    align-items: center;
    gap: 8px;
    margin-top: 10px;
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
