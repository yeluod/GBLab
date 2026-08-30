import { describe, expect, it } from 'vitest';

import { AudioCodec } from './media-config';
import { normalizeDetectedAudioCodec } from './media-runtime';

describe('normalizeDetectedAudioCodec', () => {
  it('preserves supported detected codecs', () => {
    expect(normalizeDetectedAudioCodec(AudioCodec.Aac)).toBe(AudioCodec.Aac);
    expect(normalizeDetectedAudioCodec(AudioCodec.G711A)).toBe(AudioCodec.G711A);
  });

  it('maps unknown source codecs to other', () => {
    expect(normalizeDetectedAudioCodec('mp3')).toBe('other');
  });
});
