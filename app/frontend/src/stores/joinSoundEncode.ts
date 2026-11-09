import { t } from '../i18n';

export const JOIN_SOUND_MAX_SECONDS = 2;
export const JOIN_SOUND_RATE = 24000;

/** 16-bit PCM mono WAV from float samples in -1..1. */
export function encodeWav(samples: Float32Array, sampleRate: number): Uint8Array {
  const dataLength = samples.length * 2;
  const out = new Uint8Array(44 + dataLength);
  const view = new DataView(out.buffer);
  const ascii = (at: number, text: string) => { for (let i = 0; i < text.length; i++) out[at + i] = text.charCodeAt(i); };
  const tag = (...codes: number[]) => String.fromCharCode(...codes);
  ascii(0, tag(0x52, 0x49, 0x46, 0x46));
  view.setUint32(4, 36 + dataLength, true);
  ascii(8, tag(0x57, 0x41, 0x56, 0x45));
  ascii(12, tag(0x66, 0x6d, 0x74, 0x20));
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  ascii(36, tag(0x64, 0x61, 0x74, 0x61));
  view.setUint32(40, dataLength, true);
  for (let i = 0; i < samples.length; i++) {
    const clamped = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(44 + i * 2, Math.round(clamped < 0 ? clamped * 0x8000 : clamped * 0x7fff), true);
  }
  return out;
}

/** Decode any audio file the platform understands into a mono 24 kHz 16-bit WAV of at most 2 seconds. */
export async function encodeJoinSound(file: File): Promise<Uint8Array> {
  const Audio = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Audio) throw new Error(t('This sound cannot be read'));
  const context = new Audio();
  try {
    let decoded: AudioBuffer;
    try {
      decoded = await context.decodeAudioData(await file.arrayBuffer());
    } catch {
      throw new Error(t('This sound cannot be read'));
    }
    if (decoded.duration > JOIN_SOUND_MAX_SECONDS) throw new Error(t('The sound is longer than 2 seconds'));
    const frames = Math.max(1, Math.ceil(decoded.duration * JOIN_SOUND_RATE));
    const offline = new OfflineAudioContext(1, frames, JOIN_SOUND_RATE);
    const source = offline.createBufferSource();
    source.buffer = decoded; // a mono destination downmixes the channels
    source.connect(offline.destination);
    source.start();
    const rendered = await offline.startRendering();
    return encodeWav(rendered.getChannelData(0), JOIN_SOUND_RATE);
  } finally {
    void context.close().catch(() => {});
  }
}
