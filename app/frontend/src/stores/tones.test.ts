import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { DEFAULT_PREFS, parsePrefs, setPrefs, type SoundSet } from './settingsStore';
import { playTone, TONE_SETS, TONES, type Tone } from './tones';

// A recording stand-in for the Web Audio graph: every gain schedule, oscillator and filter is kept.
type Gain = { events: { op: 'set' | 'linear' | 'exp'; value: number; at: number }[] };
type Osc = { hz: number; sweepTo?: number };
const log = { oscillators: [] as Osc[], gains: [] as Gain[], filters: [] as { type: string; frequency: { value: number } }[] };

class FakeAudioContext {
  currentTime = 0;
  destination = {};
  createOscillator() {
    const osc: Osc = { hz: 0 };
    log.oscillators.push(osc);
    const frequency = {
      set value(hz: number) { osc.hz = hz; },
      get value() { return osc.hz; },
      exponentialRampToValueAtTime(value: number) { osc.sweepTo = value; },
    };
    return { type: '', frequency, connect: (node: unknown) => node, start() {}, stop() {} };
  }
  createGain() {
    const gain: Gain = { events: [] };
    log.gains.push(gain);
    return {
      gain: {
        value: 1,
        setValueAtTime: (value: number, at: number) => gain.events.push({ op: 'set', value, at }),
        linearRampToValueAtTime: (value: number, at: number) => gain.events.push({ op: 'linear', value, at }),
        exponentialRampToValueAtTime: (value: number, at: number) => gain.events.push({ op: 'exp', value, at }),
      },
      connect: (node: unknown) => node,
    };
  }
  createBiquadFilter() {
    const filter = { type: '', frequency: { value: 0 }, connect: (node: unknown) => node };
    log.filters.push(filter);
    return filter;
  }
}
vi.stubGlobal('AudioContext', FakeAudioContext);

/** Play one tone with the given set and return the notes it scheduled (gains that were actually used). */
function play(set: SoundSet, tone: Tone, volume = 1) {
  setPrefs({ ...DEFAULT_PREFS, notifications: { ...DEFAULT_PREFS.notifications, soundSet: set } });
  log.oscillators.length = 0;
  log.gains.length = 0;
  log.filters.length = 0;
  playTone(tone, volume);
  return log.gains.filter((gain) => gain.events.length > 0);
}

beforeEach(() => {
  setPrefs(DEFAULT_PREFS);
});

afterEach(() => vi.restoreAllMocks());

const TONE_NAMES = Object.keys(TONE_SETS.soft) as Tone[];

it('every tone of both sets schedules one gain per note, with a soft attack of at least 25 ms', () => {
  for (const set of ['soft', 'birds'] as const) {
    for (const tone of TONE_NAMES) {
      const notes = play(set, tone);
      expect(notes, `${set} ${tone}`).toHaveLength(TONE_SETS[set][tone].notes.length);
      for (const note of notes) {
        const [silence, peak] = note.events;
        expect(silence.op).toBe('set');
        expect(peak.op).toBe('linear');
        expect(peak.at - silence.at, `${set} ${tone} attack`).toBeGreaterThanOrEqual(0.025 - 1e-9);
      }
    }
  }
});

it('every tone peaks at no more than 0.18 of the volume', () => {
  for (const set of ['soft', 'birds'] as const) {
    for (const tone of TONE_NAMES) {
      for (const note of play(set, tone, 1)) {
        expect(note.events[1].value, `${set} ${tone}`).toBeLessThanOrEqual(0.18 + 1e-9);
      }
    }
  }
  const [note] = play('birds', 'mention', 0.5);
  expect(note.events[1].value).toBeCloseTo(0.09, 6);
});

it('every soft tone stays at or below 2400 Hz and goes through a 2400 Hz lowpass', () => {
  for (const tone of TONE_NAMES) {
    play('soft', tone);
    expect(log.oscillators.every((osc) => osc.hz <= 2400), tone).toBe(true);
    expect(log.filters).toHaveLength(1);
    expect(log.filters[0].type).toBe('lowpass');
    expect(log.filters[0].frequency.value).toBe(2400);
  }
  for (const spec of Object.values(TONE_SETS.soft)) {
    expect(spec.notes.every((hz) => hz <= 2400)).toBe(true);
    expect(spec.sweep).toBeUndefined();
  }
});

it('the soft set is the old tones a fourth lower', () => {
  expect(TONES).toBe(TONE_SETS.soft);
  expect(TONE_SETS.soft.mute).toEqual({ notes: [390, 294], noteMs: 70 });
  expect(TONE_SETS.soft['ptt-on']).toEqual({ notes: [555], noteMs: 45 });
});

it('every birds tone glides: its sweep is set and is not 1', () => {
  for (const tone of TONE_NAMES) {
    const spec = TONE_SETS.birds[tone];
    expect(spec.sweep, tone).toBeDefined();
    expect(spec.sweep, tone).not.toBe(1);
    expect(spec.notes.every((hz) => hz >= 1500 && hz <= 3000), tone).toBe(true);
  }
});

it('a birds chirp glides from its start frequency by its sweep, through a 3200 Hz lowpass', () => {
  play('birds', 'mention');
  expect(log.oscillators[0]).toMatchObject({ hz: 2000, sweepTo: 2700 });
  expect(log.filters[0].type).toBe('lowpass');
  expect(log.filters[0].frequency.value).toBe(3200);
});

it('the birds mention is two chirps and the join is rising', () => {
  expect(TONE_SETS.birds.mention.notes).toHaveLength(2);
  expect(TONE_SETS.birds.join.sweep).toBeGreaterThan(1);
  expect(TONE_SETS.birds.leave.sweep).toBeLessThan(1);
});

it('parsePrefs defaults the sound set to birds and repairs an unknown one', () => {
  expect(DEFAULT_PREFS.notifications.soundSet).toBe('birds');
  expect(parsePrefs(null).notifications.soundSet).toBe('birds');
  expect(parsePrefs(JSON.stringify({ notifications: { soundSet: 'loud' } })).notifications.soundSet).toBe('birds');
  expect(parsePrefs(JSON.stringify({ notifications: { soundSet: 'soft' } })).notifications.soundSet).toBe('soft');
});

it('no tone plays when sounds are off', () => {
  setPrefs({ ...DEFAULT_PREFS, notifications: { ...DEFAULT_PREFS.notifications, sounds: false } });
  log.gains.length = 0;
  playTone('mention', 1);
  expect(log.gains).toEqual([]);
});
