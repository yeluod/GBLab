/** 全局媒体源类型。 */
export enum MediaSourceType {
  Mp4 = 'mp4',
}

/** 可供媒体管线使用的视频编码。 */
export enum VideoCodec {
  H264 = 'h264',
  H265 = 'h265',
}

/** 可供媒体流展示和后续编码使用的音频编码。 */
export enum AudioCodec {
  G711A = 'g711a',
  G711U = 'g711u',
  Aac = 'aac',
}

export interface Mp4SourceConfig {
  filePath: string;
  isLooping: boolean;
}

export interface MediaSourceConfig {
  type: MediaSourceType;
  mp4: Mp4SourceConfig;
}

export interface MediaPreferences {
  shouldProbeAfterSelection: boolean;
}

/** 所有模拟设备和通道共享的唯一媒体配置。 */
export interface GlobalMediaConfig {
  source: MediaSourceConfig;
  preferences: MediaPreferences;
}
