import { describe, expect, it } from 'vitest';
import type { VoiceParticipant } from '../../types';
import { arrange, avatarSizeFor, gridColumns, gridRows, MAX_GRID_TILES } from './voiceLayout';

const person = (peer: number, extra: Partial<VoiceParticipant> = {}): VoiceParticipant =>
  ({ user_id: `u${peer}`, peer_id: peer, username: `user${peer}`, channel_id: 'room', ...extra });
const room = (n: number) => Array.from({ length: n }, (_, i) => person(i + 1));

describe('grid shape', () => {
  it('uses 1, 2, 2x2, 3x2, 3x3, 4x3 for the sizes of the design', () => {
    const shape = (n: number) => `${gridColumns(n)}x${gridRows(n)}`;
    expect([1, 2, 3, 4, 5, 6, 7, 9, 10, 12].map(shape)).toEqual(['1x1', '2x1', '2x2', '2x2', '3x2', '3x2', '3x3', '3x3', '4x3', '4x3']);
  });

  it('never has more than 12 tiles worth of rows, however crowded', () => {
    expect(gridRows(40)).toBe(3);
  });

  it('shrinks avatars as the room fills up', () => {
    expect([1, 2, 3, 4, 5, 9, 10, 12].map(avatarSizeFor)).toEqual([128, 128, 96, 96, 72, 72, 56, 56]);
  });
});

describe('arrange', () => {
  it('puts me first and keeps everyone in a small room on the grid', () => {
    const result = arrange(room(5), 3, new Set());
    expect(result.grid.map((p) => p.peer_id)).toEqual([3, 1, 2, 4, 5]);
    expect(result.roster).toEqual([]);
  });

  it('moves the overflow to the roster, but never hides someone who is speaking', () => {
    const participants = room(16);
    const result = arrange(participants, 1, new Set([15, 16]));
    expect(result.grid).toHaveLength(MAX_GRID_TILES);
    expect(result.grid.map((p) => p.peer_id).slice(0, 3)).toEqual([1, 15, 16]);
    expect(result.roster).toHaveLength(4);
    expect(result.grid.length + result.roster.length).toBe(16);
  });

  it('lists other people who share as LIVE cards, not me', () => {
    const participants = [person(1, { is_sharing: true }), person(2, { is_sharing: true }), person(3)];
    const result = arrange(participants, 1, new Set());
    expect(result.shares.map((p) => p.peer_id)).toEqual([2]);
    expect(result.grid.map((p) => p.peer_id)).toEqual([1, 2, 3]);
  });

  it('works before my own peer id is known and with an empty room', () => {
    expect(arrange(room(3), null, new Set()).grid.map((p) => p.peer_id)).toEqual([1, 2, 3]);
    expect(arrange([], 1, new Set())).toEqual({ grid: [], roster: [], shares: [] });
  });
});
