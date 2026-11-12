// Feedback tones, made in code (no sound files to ship). Kept apart from desktop.ts so the voice
// stores can play them without importing the desktop module, which imports the session store.

import { prefs, type SoundSet } from './settingsStore';

export type { SoundSet } from './settingsStore';
export type Tone = 'mention' | 'join' | 'leave' | 'mute' | 'unmute' | 'deafen' | 'undeafen' | 'ptt-on' | 'ptt-off';

/**
 * One tone: `notes` are start frequencies (Hz), each `noteMs` long. `sweep` is how many times the
 * frequency changes over one note (a chirp); `vibratoHz` wobbles the pitch by about 3 %.
 */
export interface ToneSpec {
  notes: number[];
  noteMs: number;
  sweep?: number;
  vibratoHz?: number;
}

/** Soft set: the first sounds, a fourth lower (x 0.75), through a lowpass at SOFT_CUTOFF_HZ. */
const SOFT: Record<Tone, ToneSpec> = {
  mention: { notes: [741, 989], noteMs: 110 },
  join: { notes: [495, 660], noteMs: 90 },
  leave: { notes: [660, 495], noteMs: 90 },
  mute: { notes: [390, 294], noteMs: 70 },
  unmute: { notes: [294, 390], noteMs: 70 },
  deafen: { notes: [330, 221], noteMs: 80 },
  undeafen: { notes: [221, 330], noteMs: 80 },
  'ptt-on': { notes: [555], noteMs: 45 },
  'ptt-off': { notes: [390], noteMs: 45 },
};

/** Birds: short synthesised chirps. Starts are 1600-2800 Hz; every tone glides (sweep up or down). */
const BIRDS: Record<Tone, ToneSpec> = {
  mention: { notes: [2000, 2000], noteMs: 70, sweep: 1.35, vibratoHz: 30 },
  join: { notes: [1800, 2400], noteMs: 90, sweep: 1.3 },
  leave: { notes: [2600, 2000], noteMs: 90, sweep: 0.75 },
  mute: { notes: [2200], noteMs: 70, sweep: 0.7 },
  unmute: { notes: [1900], noteMs: 80, sweep: 1.5 },
  deafen: { notes: [2200, 1600], noteMs: 80, sweep: 0.85 },
  undeafen: { notes: [1800], noteMs: 80, sweep: 1.4 },
  'ptt-on': { notes: [2600], noteMs: 40, sweep: 1.1 },
  'ptt-off': { notes: [2200], noteMs: 40, sweep: 0.93 },
};

export const TONE_SETS: Record<SoundSet, Record<Tone, ToneSpec>> = { birds: BIRDS, soft: SOFT };

/** The soft set under its old name; desktop.ts and the tests still read it. */
export const TONES = SOFT;

const SOFT_CUTOFF_HZ = 2400;
const BIRDS_CUTOFF_HZ = 3200;
const ATTACK_S = 0.025;
const PEAK = 0.18;
const VIBRATO_DEPTH = 0.03;

let context: AudioContext | null = null;

/** The shared audio context (made on first use), or null when the platform has none. */
export function audioContext(): AudioContext | null {
  try {
    const Audio = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!Audio) return null;
    context ??= new Audio();
    return context;
  } catch { return null; }
}

/** Play a tone at the chosen volume and sound set; silently does nothing when sounds are off or unavailable. */
export function playTone(tone: Tone, volume = prefs().notifications.soundVolume) {
  if (!prefs().notifications.sounds || volume <= 0) return;
  try {
    const Audio = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!Audio) return;
    context ??= new Audio();
    const audio = context;
    const set = prefs().notifications.soundSet === 'soft' ? 'soft' : 'birds';
    const { notes, noteMs, sweep, vibratoHz } = TONE_SETS[set][tone];
    const cutoff = set === 'soft' ? SOFT_CUTOFF_HZ : BIRDS_CUTOFF_HZ;
    const filter = audio.createBiquadFilter();
    filter.type = 'lowpass';
    filter.frequency.value = cutoff;
    filter.connect(audio.destination);
    const start = audio.currentTime + 0.01;
    const length = noteMs / 1000;
    notes.forEach((frequency, index) => {
      try {
        const at = start + index * length;
        const oscillator = audio.createOscillator();
        const gain = audio.createGain();
        oscillator.type = 'sine';
        oscillator.frequency.value = frequency;
        if (sweep !== undefined) oscillator.frequency.exponentialRampToValueAtTime(frequency * sweep, at + length);
        if (vibratoHz !== undefined) {
          const lfo = audio.createOscillator();
          const depth = audio.createGain();
          lfo.frequency.value = vibratoHz;
          depth.gain.value = frequency * VIBRATO_DEPTH;
          lfo.connect(depth).connect(oscillator.frequency);
          lfo.start(at);
          lfo.stop(at + length + 0.02);
        }
        gain.gain.setValueAtTime(0, at);
        gain.gain.linearRampToValueAtTime(Math.min(1, volume) * PEAK, at + ATTACK_S);
        gain.gain.exponentialRampToValueAtTime(0.0001, at + length);
        oscillator.connect(gain).connect(filter);
        oscillator.start(at);
        oscillator.stop(at + length + 0.02);
      } catch { /* one note that cannot be scheduled does not stop the others */ }
    });
  } catch { /* no sound is not an error */ }
}
