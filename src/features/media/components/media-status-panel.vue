<script setup lang="ts">
  import { computed, ref, watch } from 'vue';
  import { NButton, NDivider, NEmpty, NSpin, NTag } from 'naive-ui';

  import {
    AudioCodec,
    MediaSourceStatus,
    MediaSourceType,
    RecordingStatus,
    VideoCodec,
    type MediaProbeResult,
    type MediaRuntimeStatus,
  } from '@/features/media';

  const props = defineProps<{
    runtimeStatus: MediaRuntimeStatus;
    probeResult: MediaProbeResult | null;
    isPreviewPending: boolean;
    canStartPreview: boolean;
    sourceType: MediaSourceType;
    previewFrame?: { width: number; height: number; rgba: number[] } | null;
  }>();
  const emit = defineEmits<{
    startPreview: [];
    stopPreview: [];
  }>();

  const sourceStatusText: Record<MediaSourceStatus, string> = {
    [MediaSourceStatus.Unconfigured]: '未配置',
    [MediaSourceStatus.Loading]: '加载中',
    [MediaSourceStatus.Ready]: '就绪',
    [MediaSourceStatus.Previewing]: '预览中',
    [MediaSourceStatus.Error]: '错误',
    [MediaSourceStatus.Unavailable]: '不可用',
  };
  const recordingStatusText: Record<RecordingStatus, string> = {
    [RecordingStatus.Disabled]: '未启用',
    [RecordingStatus.Ready]: '就绪',
    [RecordingStatus.Recording]: '录制中',
    [RecordingStatus.Error]: '错误',
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
  function drawFrame(frame: { width: number; height: number; rgba: number[] } | null | undefined) {
    if (frame === null || frame === undefined || previewCanvas.value === null) return;
    const canvas = previewCanvas.value;
    canvas.width = frame.width;
    canvas.height = frame.height;
    const context = canvas.getContext('2d');
    if (context === null) return;
    context.putImageData(
      new ImageData(new Uint8ClampedArray(frame.rgba), frame.width, frame.height),
      0,
      0,
    );
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
      [VideoCodec.RawVideo]: '原始视频',
      [AudioCodec.G711A]: 'G.711 A-law',
      [AudioCodec.G711U]: 'G.711 μ-law',
      [AudioCodec.Aac]: 'AAC',
      [AudioCodec.Pcm]: 'PCM',
    };
    return labels[codec];
  }

  function durationLabel(durationSeconds: number | null): string {
    if (durationSeconds === null) return '实时';
    const minutes = Math.floor(durationSeconds / 60);
    const seconds = durationSeconds % 60;
    return `${minutes}:${String(seconds).padStart(2, '0')}`;
  }

  function byteLabel(bytes: number): string {
    if (bytes === 0) return '0 MB';
    return `${(bytes / 1_048_576).toFixed(1)} MB`;
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
        <template v-else-if="runtimeStatus.sourceStatus === MediaSourceStatus.Previewing">
          <canvas ref="previewCanvas" class="preview-canvas" aria-label="视频预览画面"></canvas>
          <strong>{{ previewTitle }}</strong>
          <span>{{ runtimeStatus.sourceLabel }}</span>
        </template>
        <NEmpty v-else :description="previewEmptyDescription" />
      </div>

      <div class="preview-actions">
        <NButton
          v-if="runtimeStatus.sourceStatus !== MediaSourceStatus.Previewing"
          type="primary"
          :disabled="!canStartPreview"
          :loading="isPreviewPending"
          data-testid="start-preview"
          @click="emit('startPreview')"
        >
          开始预览
        </NButton>
        <NButton
          v-else
          type="warning"
          :loading="isPreviewPending"
          data-testid="stop-preview"
          @click="emit('stopPreview')"
        >
          停止预览
        </NButton>
        <span>{{ sourceType === MediaSourceType.Camera ? '等待摄像头采集' : '等待文件播放' }}</span>
      </div>
    </section>

    <NDivider />

    <section class="media-information-section">
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

    <NDivider />

    <section class="runtime-section">
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
        <dd>{{ runtimeStatus.activeLiveSessions }}</dd>
        <dt>Playback</dt>
        <dd>{{ runtimeStatus.activePlaybackSessions }}</dd>
        <dt>Recording</dt>
        <dd>{{ recordingStatusText[runtimeStatus.recording.status] }}</dd>
        <dt>Current File</dt>
        <dd>{{ runtimeStatus.recording.currentFile ?? '—' }}</dd>
        <dt>Recorded</dt>
        <dd>{{ durationLabel(runtimeStatus.recording.recordedDurationSeconds) }}</dd>
        <dt>Used Space</dt>
        <dd>{{ byteLabel(runtimeStatus.recording.usedSpaceBytes) }}</dd>
      </dl>
    </section>
  </aside>
</template>
