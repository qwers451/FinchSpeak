import { beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { autoPan, AUTO_SPREAD, commitSeats, dragSeat, panLayout, placeSeat, scheduleSpatialSync, resetSeats, roomOthers, seats, setSpatialEnabled, syncSpatial } from './spatialStore';
import { setLocalPeerId } from './sessionStore';
import { setActiveVoiceChannel, setVoiceParticipants } from './voiceStore';
import type { VoiceParticipant } from '../types';

const person = (name: string, peer: number, channel = 'room'): VoiceParticipant => ({ user_id: `u-${name}`, peer_id: peer, username: name, channel_id: channel });

beforeEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  resetSeats();
  setSpatialEnabled(false);
  setLocalPeerId(1);
  setActiveVoiceChannel({ id: 'room', short_id: 2, name: 'Room', channel_type: 'voice', position: 0, bitrate_kbps: 64 });
  setVoiceParticipants([person('me', 1), person('Bob', 3), person('Alice', 2), person('Zed', 9, 'elsewhere')]);
});

it('spreads people evenly from left to right; one person sits in the centre', () => {
  expect(autoPan(0, 1)).toBe(0);
  expect(autoPan(0, 3)).toBeCloseTo(-AUTO_SPREAD);
  expect(autoPan(1, 3)).toBeCloseTo(0);
  expect(autoPan(2, 3)).toBeCloseTo(AUTO_SPREAD);
});

it('places only the others in my room, in a stable order, and a drag pins someone', () => {
  const others = roomOthers([person('me', 1), person('Bob', 3), person('Alice', 2), person('Zed', 9, 'elsewhere')], 'room', 1);
  expect(others.map((p) => p.username)).toEqual(['Alice', 'Bob']);
  const layout = panLayout(others, { 'u-Bob': -0.3 });
  expect(layout.map((seat) => [seat.participant.username, seat.pan, seat.pinned])).toEqual([['Alice', -AUTO_SPREAD, false], ['Bob', -0.3, true]]);
});

it('remembers where people were put and forgets on request', () => {
  placeSeat('u-Bob', 5);
  expect(seats()['u-Bob']).toBe(1);
  expect(JSON.parse(localStorage.getItem('finchspeak.spatial')!).seats['u-Bob']).toBe(1);
  resetSeats('u-Bob');
  expect(seats()).toEqual({});
});

it('tells the mixer the switch and, when on, where everyone sits', async () => {
  const spatial = vi.spyOn(api, 'setSpatialAudio').mockResolvedValue();
  const pan = vi.spyOn(api, 'setSpeakerPan').mockResolvedValue();
  await syncSpatial();
  expect(spatial).toHaveBeenLastCalledWith(false);
  expect(pan).not.toHaveBeenCalled();
  setSpatialEnabled(true);
  placeSeat('u-Bob', 0.5);
  await syncSpatial();
  expect(spatial).toHaveBeenLastCalledWith(true);
  expect(pan).toHaveBeenCalledWith(2, -AUTO_SPREAD);
  expect(pan).toHaveBeenCalledWith(3, 0.5);
});

it('a drag only writes storage when it ends', () => {
  const write = vi.spyOn(localStorage, 'setItem');
  dragSeat('u-Bob', 0.1);
  dragSeat('u-Bob', 0.2);
  dragSeat('u-Bob', 0.3);
  expect(seats()['u-Bob']).toBe(0.3);
  expect(write).not.toHaveBeenCalled();
  commitSeats();
  expect(write).toHaveBeenCalledTimes(1);
  expect(JSON.parse(localStorage.getItem('finchspeak.spatial')!).seats['u-Bob']).toBe(0.3);
});

it('mixer sync during a drag runs at most every 50 ms with the latest value', async () => {
  await new Promise((resolve) => setTimeout(resolve, 80)); // let timers from setup run out
  vi.useFakeTimers();
  try {
    const pan = vi.spyOn(api, 'setSpeakerPan').mockResolvedValue();
    vi.spyOn(api, 'setSpatialAudio').mockResolvedValue();
    setSpatialEnabled(true);
    await vi.advanceTimersByTimeAsync(100);
    pan.mockClear();
    for (const value of [0.1, 0.2, 0.3, 0.4]) { dragSeat('u-Bob', value); scheduleSpatialSync(); await vi.advanceTimersByTimeAsync(10); }
    await vi.advanceTimersByTimeAsync(100);
    const bob = pan.mock.calls.filter(([peer]) => peer === 3);
    expect(bob.length).toBeLessThanOrEqual(3);
    expect(bob.at(-1)).toEqual([3, 0.4]);
  } finally {
    vi.useRealTimers();
  }
});
