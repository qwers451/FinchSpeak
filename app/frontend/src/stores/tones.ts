// Feedback tones, made in code (no sound files to ship). Kept apart from desktop.ts so the voice
// stores can play them without importing the desktop module, which imports the session store.

import { prefs } from './settingsStore';

export type Tone = 'mention' | 'join' | 'leave' | 'mute' | 'unmute' | 'deafen' | 'undeafen' | 'ptt-on' | 'ptt-off';

/** Notes (Hz) and length (ms) of each tone. */
export const TONES: Record<Tone, { notes: number[]; noteMs: number }> = {
  mention: { notes: [988, 1319], noteMs: 110 },
  join: { notes: [660, 880], noteMs: 90 },
  leave: { notes: [880, 660], noteMs: 90 },
  mute: { notes: [520, 392], noteMs: 70 },
  unmute: { notes: [392, 520], noteMs: 70 },
  deafen: { notes: [440, 294], noteMs: 80 },
  undeafen: { notes: [294, 440], noteMs: 80 },
  'ptt-on': { notes: [740], noteMs: 45 },
  'ptt-off': { notes: [520], noteMs: 45 },
};

let context: AudioContext | null = null;

/** Play a tone at the chosen volume; silently does nothing when sounds are off or unavailable. */
export function playTone(tone: Tone, volume = prefs().notifications.soundVolume) {
  if (!prefs().notifications.sounds || volume <= 0) return;
  try {
    const Audio = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!Audio) return;
    context ??= new Audio();
    const { notes, noteMs } = TONES[tone];
    const start = context.currentTime + 0.01;
    notes.forEach((frequency, index) => {
      const oscillator = context!.createOscillator();
      const gain = context!.createGain();
      const at = start + (index * noteMs) / 1000;
      oscillator.type = 'sine';
      oscillator.frequency.value = frequency;
      gain.gain.setValueAtTime(0, at);
      gain.gain.linearRampToValueAtTime(Math.min(1, volume) * 0.3, at + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.0001, at + noteMs / 1000);
      oscillator.connect(gain).connect(context!.destination);
      oscillator.start(at);
      oscillator.stop(at + noteMs / 1000 + 0.02);
    });
  } catch { /* no sound is not an error */ }
}
