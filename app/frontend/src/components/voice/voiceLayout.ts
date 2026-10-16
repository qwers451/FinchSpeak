import type { VoiceParticipant } from '../../types';

/** At most this many tiles are drawn as a grid; everyone else is in the roster. */
export const MAX_GRID_TILES = 12;

/** Columns of the grid for `count` tiles: 1, 2, 4, 6, 9 and 12 fill their rectangles exactly. */
export function gridColumns(count: number): number {
  if (count <= 1) return 1;
  if (count <= 4) return 2;
  if (count <= 9) return 3;
  return 4;
}

/** Rows of the grid for `count` tiles. */
export function gridRows(count: number): number {
  return Math.max(1, Math.ceil(Math.min(count, MAX_GRID_TILES) / gridColumns(count)));
}

/** Avatar size (px) that suits the number of tiles; the design's 128 → 56 steps. */
export function avatarSizeFor(count: number): 128 | 96 | 72 | 56 {
  if (count <= 2) return 128;
  if (count <= 4) return 96;
  if (count <= 9) return 72;
  return 56;
}

export interface Arranged {
  /** People who get a tile, in display order. */
  grid: VoiceParticipant[];
  /** The rest, listed compactly (only when the room is crowded). */
  roster: VoiceParticipant[];
  /** Sharers other than me: they get a wide LIVE card instead of a tile. */
  shares: VoiceParticipant[];
}

/**
 * Splits the room into LIVE cards, tiles and roster. I am always the first tile. When there are
 * more people than tiles, the ones who are speaking come first so nobody who talks is hidden.
 */
export function arrange(participants: VoiceParticipant[], localPeerId: number | null, speaking: ReadonlySet<number>): Arranged {
  const shares = participants.filter((participant) => participant.is_sharing && participant.peer_id !== localPeerId);
  const me = participants.find((participant) => participant.peer_id === localPeerId);
  const others = participants.filter((participant) => participant.peer_id !== localPeerId);
  // Sharers keep a tile as well (the LIVE card is about the picture, the tile about the person).
  const room = me ? [me, ...others] : others;
  if (room.length <= MAX_GRID_TILES) return { grid: room, roster: [], shares };

  const first = me ? [me] : [];
  const loud = others.filter((participant) => speaking.has(participant.peer_id));
  const quiet = others.filter((participant) => !speaking.has(participant.peer_id));
  const ordered = [...first, ...loud, ...quiet];
  return { grid: ordered.slice(0, MAX_GRID_TILES), roster: ordered.slice(MAX_GRID_TILES), shares };
}
