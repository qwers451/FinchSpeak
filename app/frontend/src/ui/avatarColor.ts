/** Eight avatar colours (see the design's palette). */
export const AVATAR_COLORS = 8;

/** The same id always gives the same colour index (1..8). */
export function avatarColor(id: string): number {
  let hash = 2166136261;
  for (let i = 0; i < id.length; i++) {
    hash ^= id.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return ((hash >>> 0) % AVATAR_COLORS) + 1;
}

/** First visible letter of a name, upper-cased; "?" for nothing. */
export function initialOf(name: string | undefined | null): string {
  const first = Array.from((name ?? '').trim())[0];
  return first ? first.toUpperCase() : '?';
}
