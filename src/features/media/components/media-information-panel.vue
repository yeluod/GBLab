<script setup lang="ts">
  import { computed } from 'vue';

  import {
    AudioCodec,
    VideoCodec,
    type DetectedAudioCodec,
    type MediaProbeResult,
    type MediaRuntimeStatus,
  } from '@/features/media';

  const props = defineProps<{
    runtimeStatus: MediaRuntimeStatus;
    probeResult: MediaProbeResult | null;
  }>();

  const displayedVideo = computed(() => props.probeResult?.video ?? props.runtimeStatus.video);
  const displayedAudio = computed(() =>
    props.probeResult === null ? props.runtimeStatus.audio : props.probeResult.audio,
  );

  function codecLabel(codec: VideoCodec | AudioCodec | DetectedAudioCodec): string {
    const labels: Record<VideoCodec | AudioCodec | DetectedAudioCodec, string> = {
      [VideoCodec.H264]: 'H.264',
      [VideoCodec.H265]: 'H.265',
      [AudioCodec.G711A]: 'G.711 A-law',
      [AudioCodec.G711U]: 'G.711 μ-law',
      [AudioCodec.Aac]: 'AAC',
      mp3: 'MP3',
      other: '其它音频',
    };
    return labels[codec] ?? '未知编码';
  }

  function durationLabel(durationSeconds: number | null): string {
    if (durationSeconds === null) return '未知';
    const totalSeconds = Math.max(0, Math.floor(durationSeconds));
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    return `${minutes}:${String(seconds).padStart(2, '0')}`;
  }
</script>

<template>
  <section
    class="media-information-section media-detail-card"
    aria-labelledby="media-information-title"
  >
    <div class="media-section-heading compact">
      <div>
        <span class="section-kicker">MEDIA INFORMATION</span>
        <h2 id="media-information-title">媒体信息</h2>
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
</template>
