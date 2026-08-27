<script setup lang="ts">
  import { computed } from 'vue';
  import {
    NButton,
    NDivider,
    NForm,
    NFormItem,
    NInput,
    NInputGroup,
    NInputNumber,
    NRadioButton,
    NRadioGroup,
    NSelect,
    NSwitch,
    type SelectOption,
  } from 'naive-ui';

  import {
    AudioCodec,
    CaptureDeviceStatus,
    EncoderBackend,
    MediaSourceType,
    VideoCodec,
    type CaptureDeviceCapabilities,
    type CaptureDeviceInfo,
    type GlobalMediaConfig,
    type MediaFieldErrors,
  } from '@/features/media';

  const config = defineModel<GlobalMediaConfig>('config', { required: true });
  const props = defineProps<{
    videoDevices: CaptureDeviceInfo[];
    audioDevices: CaptureDeviceInfo[];
    capabilities: CaptureDeviceCapabilities | null;
    fieldErrors: MediaFieldErrors;
    isProbing: boolean;
  }>();
  const emit = defineEmits<{
    sourceTypeChange: [sourceType: MediaSourceType];
    selectMp4: [];
    probeMp4: [];
    videoDeviceChange: [deviceId: string];
    videoResolutionChange: [width: number, height: number];
    selectRecordingDirectory: [];
  }>();

  const sourceTypeOptions = [
    { label: 'MP4 文件', value: MediaSourceType.Mp4 },
    { label: '摄像头', value: MediaSourceType.Camera },
  ];
  const videoCodecOptions: SelectOption[] = [
    { label: 'H.264', value: VideoCodec.H264 },
    { label: 'H.265', value: VideoCodec.H265 },
  ];
  const audioCodecOptions: SelectOption[] = [
    { label: 'G.711 A-law', value: AudioCodec.G711A },
    { label: 'G.711 μ-law', value: AudioCodec.G711U },
    { label: 'AAC', value: AudioCodec.Aac },
  ];
  const encoderOptions: SelectOption[] = [
    { label: 'Auto（自动选择）', value: EncoderBackend.Auto },
  ];
  const sampleRateOptions: SelectOption[] = [
    { label: '8 kHz', value: 8_000 },
    { label: '16 kHz', value: 16_000 },
    { label: '44.1 kHz', value: 44_100 },
    { label: '48 kHz', value: 48_000 },
  ];
  const channelOptions: SelectOption[] = [
    { label: '单声道', value: 1 },
    { label: '双声道', value: 2 },
  ];
  const segmentOptions: SelectOption[] = [5, 10, 30, 60].map((value) => ({
    label: `${value} 分钟`,
    value,
  }));

  const videoDeviceOptions = computed<SelectOption[]>(() =>
    props.videoDevices.map((device) => ({
      label: `${device.name}${device.status === CaptureDeviceStatus.Available ? '' : '（不可用）'}`,
      value: device.id,
      disabled: device.status !== CaptureDeviceStatus.Available,
    })),
  );
  const audioDeviceOptions = computed<SelectOption[]>(() =>
    props.audioDevices.map((device) => ({
      label: device.name,
      value: device.id,
      disabled: device.status !== CaptureDeviceStatus.Available,
    })),
  );
  const resolutionOptions = computed<SelectOption[]>(() =>
    (props.capabilities?.modes ?? []).map((mode) => ({
      label: `${mode.width} × ${mode.height}`,
      value: `${mode.width}x${mode.height}`,
    })),
  );
  const selectedMode = computed(() => {
    const { width, height } = config.value.source.camera.video;
    return props.capabilities?.modes.find((mode) => mode.width === width && mode.height === height);
  });
  const framesPerSecondOptions = computed<SelectOption[]>(() =>
    (selectedMode.value?.supportedFramesPerSecond ?? []).map((value) => ({
      label: `${value} FPS`,
      value,
    })),
  );
  const supportedVideoCodecOptions = computed<SelectOption[]>(() =>
    videoCodecOptions.map((option) => ({
      ...option,
      disabled: !(props.capabilities?.supportedCodecs ?? []).includes(option.value as VideoCodec),
    })),
  );
  const selectedResolution = computed({
    get: () => {
      const video = config.value.source.camera.video;
      return `${video.width}x${video.height}`;
    },
    set: (value: string) => {
      const [widthText, heightText] = value.split('x');
      const width = Number(widthText);
      const height = Number(heightText);
      if (Number.isFinite(width) && Number.isFinite(height)) {
        emit('videoResolutionChange', width, height);
      }
    },
  });

  function validationProps(
    path: string,
  ): Record<string, never> | { validationStatus: 'error'; feedback: string } {
    const error = props.fieldErrors[path];
    return error === undefined ? {} : { validationStatus: 'error', feedback: error };
  }

  function handleSourceType(value: MediaSourceType): void {
    emit('sourceTypeChange', value);
  }

  function handleVideoDevice(value: string): void {
    emit('videoDeviceChange', value);
  }
</script>

<template>
  <section class="media-config-panel" aria-labelledby="media-source-configuration-title">
    <div class="media-section-heading">
      <div>
        <span class="section-kicker">SOURCE CONFIGURATION</span>
        <h2 id="media-source-configuration-title">音视频源</h2>
      </div>
      <span class="shared-config-note">全局唯一配置</span>
    </div>

    <NForm label-placement="top" :model="config">
      <NFormItem label="媒体源类型">
        <NRadioGroup
          :value="config.source.type"
          name="media-source-type"
          @update:value="handleSourceType"
        >
          <NRadioButton
            v-for="option in sourceTypeOptions"
            :key="option.value"
            :value="option.value"
          >
            {{ option.label }}
          </NRadioButton>
        </NRadioGroup>
      </NFormItem>

      <template v-if="config.source.type === MediaSourceType.Mp4">
        <NDivider title-placement="left">MP4 文件</NDivider>
        <NFormItem label="文件" v-bind="validationProps('source.mp4.filePath')">
          <NInputGroup>
            <NInput
              v-model:value="config.source.mp4.filePath"
              placeholder="请选择 MP4 文件"
              readonly
            />
            <NButton data-testid="select-mp4" @click="emit('selectMp4')">选择文件</NButton>
            <NButton data-testid="probe-mp4" :loading="isProbing" @click="emit('probeMp4')">
              重新检测
            </NButton>
          </NInputGroup>
        </NFormItem>
        <NFormItem label="循环播放" :show-feedback="false">
          <NSwitch v-model:value="config.source.mp4.isLooping" />
        </NFormItem>
      </template>

      <template v-else>
        <NDivider title-placement="left">视频采集</NDivider>
        <div class="media-form-grid">
          <NFormItem label="摄像头" v-bind="validationProps('source.camera.video.deviceId')">
            <NSelect
              :value="config.source.camera.video.deviceId"
              :options="videoDeviceOptions"
              @update:value="handleVideoDevice"
            />
          </NFormItem>
          <NFormItem label="分辨率" v-bind="validationProps('source.camera.video.resolution')">
            <NSelect v-model:value="selectedResolution" :options="resolutionOptions" />
          </NFormItem>
          <NFormItem label="帧率" v-bind="validationProps('source.camera.video.framesPerSecond')">
            <NSelect
              v-model:value="config.source.camera.video.framesPerSecond"
              :options="framesPerSecondOptions"
            />
          </NFormItem>
          <NFormItem label="视频编码">
            <NSelect
              v-model:value="config.source.camera.video.codec"
              :options="supportedVideoCodecOptions"
            />
          </NFormItem>
          <NFormItem
            label="视频码率（Kbps）"
            v-bind="validationProps('source.camera.video.bitrateKbps')"
          >
            <NInputNumber
              v-model:value="config.source.camera.video.bitrateKbps"
              :min="128"
              :max="100000"
              :step="128"
            />
          </NFormItem>
          <NFormItem label="编码后端">
            <NSelect
              v-model:value="config.source.camera.video.encoderBackend"
              :options="encoderOptions"
            />
          </NFormItem>
        </div>

        <NDivider title-placement="left">音频采集</NDivider>
        <NFormItem label="启用音频" :show-feedback="false">
          <NSwitch v-model:value="config.source.camera.audio.isEnabled" />
        </NFormItem>
        <div
          class="media-form-grid"
          :class="{ 'is-disabled': !config.source.camera.audio.isEnabled }"
        >
          <NFormItem label="麦克风" v-bind="validationProps('source.camera.audio.deviceId')">
            <NSelect
              v-model:value="config.source.camera.audio.deviceId"
              :options="audioDeviceOptions"
              :disabled="!config.source.camera.audio.isEnabled"
            />
          </NFormItem>
          <NFormItem label="音频编码">
            <NSelect
              v-model:value="config.source.camera.audio.codec"
              :options="audioCodecOptions"
              :disabled="!config.source.camera.audio.isEnabled"
            />
          </NFormItem>
          <NFormItem label="采样率" v-bind="validationProps('source.camera.audio.sampleRate')">
            <NSelect
              v-model:value="config.source.camera.audio.sampleRate"
              :options="sampleRateOptions"
              :disabled="!config.source.camera.audio.isEnabled"
            />
          </NFormItem>
          <NFormItem label="声道" v-bind="validationProps('source.camera.audio.channels')">
            <NSelect
              v-model:value="config.source.camera.audio.channels"
              :options="channelOptions"
              :disabled="!config.source.camera.audio.isEnabled"
            />
          </NFormItem>
          <NFormItem
            label="音频码率（Kbps）"
            v-bind="validationProps('source.camera.audio.bitrateKbps')"
          >
            <NInputNumber
              v-model:value="config.source.camera.audio.bitrateKbps"
              :min="8"
              :max="512"
              :disabled="!config.source.camera.audio.isEnabled"
            />
          </NFormItem>
        </div>
      </template>

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
