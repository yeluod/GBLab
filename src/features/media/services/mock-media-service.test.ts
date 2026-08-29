import { describe, expect, it } from 'vitest';

import {
  AudioCodec,
  MediaSourceStatus,
  MediaSourceType,
  VideoCodec,
  createDefaultMediaConfig,
} from '@/features/media';
import { MockMediaService } from './mock-media-service';
import { MOCK_MP4_PATHS } from './mock-media-fixtures';

describe('MockMediaService', () => {
  it('加载全局唯一的默认 MP4 配置', async () => {
    const service = new MockMediaService();

    const config = await service.loadConfig();

    expect(config.source.type).toBe(MediaSourceType.Mp4);
    expect(config.source.mp4.isLooping).toBe(true);
  });

  it.each([
    ['H264 + AAC', MOCK_MP4_PATHS.h264WithAudio, VideoCodec.H264, AudioCodec.Aac],
    ['H265 + AAC', MOCK_MP4_PATHS.h265WithAudio, VideoCodec.H265, AudioCodec.Aac],
  ])('检测 %s MP4', async (_scenario, filePath, videoCodec, audioCodec) => {
    const result = await new MockMediaService().probeMp4(filePath);

    expect(result.video.codec).toBe(videoCodec);
    expect(result.audio?.codec).toBe(audioCodec);
  });

  it.each([
    ['H264 无音频', MOCK_MP4_PATHS.h264VideoOnly, VideoCodec.H264],
    ['H265 无音频', MOCK_MP4_PATHS.h265VideoOnly, VideoCodec.H265],
  ])('将 %s 识别为正常视频源', async (_scenario, filePath, videoCodec) => {
    const result = await new MockMediaService().probeMp4(filePath);

    expect(result.video.codec).toBe(videoCodec);
    expect(result.audio).toBeNull();
  });

  it('MP4 检测失败时返回明确错误', async () => {
    await expect(new MockMediaService().probeMp4(MOCK_MP4_PATHS.probeError)).rejects.toThrow(
      '无法解析所选 MP4 文件',
    );
  });

  it('从服务返回摄像头、麦克风和设备能力', async () => {
    const service = new MockMediaService();

    const [videoDevices, audioDevices, capabilities, encoderCapabilities] = await Promise.all([
      service.listVideoDevices(),
      service.listAudioDevices(),
      service.getVideoCapabilities('camera-integrated'),
      service.getVideoEncoderCapabilities(),
    ]);

    expect(videoDevices.map((device) => device.id)).toContain('camera-usb');
    expect(audioDevices.map((device) => device.id)).toContain('microphone-built-in');
    expect(capabilities.modes).toEqual(
      expect.arrayContaining([expect.objectContaining({ width: 1920, height: 1080 })]),
    );
    expect(encoderCapabilities.encoders.map((item) => item.codec)).toEqual([
      VideoCodec.H264,
      VideoCodec.H265,
    ]);
  });

  it('Camera only 应用后运行状态不包含音频', async () => {
    const service = new MockMediaService();
    const config = createDefaultMediaConfig();
    config.source.type = MediaSourceType.Camera;
    config.source.camera.audio.isEnabled = false;

    const status = await service.applyConfig(config);

    expect(status.sourceStatus).toBe(MediaSourceStatus.Ready);
    expect(status.video?.codec).toBe(VideoCodec.H265);
    expect(status.audio).toBeNull();
  });

  it('预览遵循 Ready 到 Previewing 再回到 Stopped 的状态机', async () => {
    const service = new MockMediaService();
    const config = createDefaultMediaConfig();

    const previewing = await service.startPreview(config);
    const ready = await service.stopPreview();

    expect(previewing.sourceStatus).toBe(MediaSourceStatus.Previewing);
    expect(ready.sourceStatus).toBe(MediaSourceStatus.Stopped);
  });

  it('支持暂停、跳转、单帧和倍速控制', async () => {
    const service = new MockMediaService();
    const config = createDefaultMediaConfig();

    await service.startPreview(config);
    expect((await service.pausePreview()).sourceStatus).toBe(MediaSourceStatus.Paused);
    expect((await service.seek(12.5)).positionSeconds).toBe(12.5);
    expect((await service.setPlaybackRate(2)).playbackRate).toBe(2);
    expect(await service.stepFrame()).not.toBeNull();
    expect((await service.resumePreview()).sourceStatus).toBe(MediaSourceStatus.Previewing);
  });

  it('可注入服务错误以覆盖后端失败路径', async () => {
    const service = new MockMediaService({ failures: ['listVideoDevices'] });

    await expect(service.listVideoDevices()).rejects.toThrow('listVideoDevices 失败');
  });
});
