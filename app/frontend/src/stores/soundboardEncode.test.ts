import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { encodeBoardSound } from './joinSoundEncode';

// Records what the offline renderer was asked for; rendering itself is silence of the requested length.
const renders: [channels: number, length: number, rate: number][] = [];

class FakeOfflineContext {
  destination = {};
  constructor(channels: number, length: number, rate: number) {
    renders.push([channels, length, rate]);
  }
  createBufferSource() {
    return { buffer: null as unknown, connect: () => undefined, start: () => undefined };
  }
  async startRendering() {
    const [, length] = renders[renders.length - 1];
    return { getChannelData: () => new Float32Array(length) };
  }
}

// A 7-second clip, whatever bytes the file holds.
class FakeAudioContext {
  destination = {};
  async decodeAudioData() {
    return { duration: 7, numberOfChannels: 2 } as unknown as AudioBuffer;
  }
  async close() {}
}

const file = { arrayBuffer: async () => new ArrayBuffer(16) } as unknown as File;

beforeEach(() => {
  renders.length = 0;
  vi.stubGlobal('OfflineAudioContext', FakeOfflineContext);
  vi.stubGlobal('AudioContext', FakeAudioContext);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

it('trims a board sound to 5 seconds and encodes it as 24 kHz 16-bit mono WAV', async () => {
  const wav = await encodeBoardSound(file);
  expect(renders).toEqual([[1, 120_000, 24_000]]);
  const text = String.fromCharCode(...wav.slice(0, 4));
  expect(text).toBe('RIFF');
  const view = new DataView(wav.buffer, wav.byteOffset, wav.byteLength);
  expect(view.getUint16(22, true)).toBe(1); // mono
  expect(view.getUint32(24, true)).toBe(24_000);
  expect(view.getUint16(34, true)).toBe(16);
  expect(wav.length).toBe(44 + 240_000);
});
