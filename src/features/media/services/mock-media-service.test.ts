import { describe, expect, it } from 'vitest';
import { AudioCodec, MediaSourceStatus, MediaSourceType, VideoCodec } from '@/features/media';
import { MockMediaService } from './mock-media-service';
import { MOCK_MP4_PATHS } from './mock-media-fixtures';

describe('MockMediaService', () => {
  it('加载 MP4 配置并识别音视频编码', async () => {
    const service = new MockMediaService();
    const config = await service.loadConfig();
    expect(config.source.type).toBe(MediaSourceType.Mp4);
    const result = await service.probeMp4(MOCK_MP4_PATHS.h265WithAudio);
    expect(result.video.codec).toBe(VideoCodec.H265);
    expect(result.audio?.codec).toBe(AudioCodec.Aac);
  });
  it.each([
    [MOCK_MP4_PATHS.h264VideoOnly, VideoCodec.H264],
    [MOCK_MP4_PATHS.h265VideoOnly, VideoCodec.H265],
  ])('识别无音频 MP4', async (path, codec) => {
    const result = await new MockMediaService().probeMp4(path);
    expect(result.video.codec).toBe(codec);
    expect(result.audio).toBeNull();
  });
  it('MP4 检测失败时返回明确错误', async () => {
    await expect(new MockMediaService().probeMp4(MOCK_MP4_PATHS.probeError)).rejects.toThrow(
      '无法解析',
    );
  });
  it('预览和播放控制遵循状态机', async () => {
    const service = new MockMediaService();
    const config = await service.loadConfig();
    expect((await service.startPreview(config)).sourceStatus).toBe(MediaSourceStatus.Previewing);
    expect((await service.pausePreview()).sourceStatus).toBe(MediaSourceStatus.Paused);
    expect((await service.seek(12.5)).positionSeconds).toBe(12.5);
    expect((await service.setPlaybackRate(2)).playbackRate).toBe(2);
    expect(await service.stepFrame()).not.toBeNull();
    expect((await service.stopPreview()).sourceStatus).toBe(MediaSourceStatus.Stopped);
  });
});
