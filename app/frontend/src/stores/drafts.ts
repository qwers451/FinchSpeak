/**
 * The unsent text of each text channel. A draft is kept in memory and mirrored to localStorage under
 * `finchspeak.draft.<channelId>`, so it survives switching channels, opening a voice room and a reload.
 * Attachments are not part of a draft.
 */

const DEBOUNCE_MS = 300;
const KEY_PREFIX = 'finchspeak.draft.';

const cache = new Map<string, string>();
let pending: { channelId: string; text: string } | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;

const keyOf = (channelId: string) => `${KEY_PREFIX}${channelId}`;

/** localStorage can be full, disabled or blocked: the draft then lives in memory only. */
function persist(channelId: string, text: string) {
  if (text) cache.set(channelId, text);
  else cache.delete(channelId);
  try {
    if (text) localStorage.setItem(keyOf(channelId), text);
    else localStorage.removeItem(keyOf(channelId));
  } catch {
    // Keep the draft in memory; nothing else to do.
  }
}

/** The saved draft of a channel, or '' when there is none. */
export function readDraft(channelId: string): string {
  const known = cache.get(channelId);
  if (known !== undefined) return known;
  try {
    const stored = localStorage.getItem(keyOf(channelId)) ?? '';
    if (stored) cache.set(channelId, stored);
    return stored;
  } catch {
    return '';
  }
}

/** Remember a draft now and write it to localStorage after a short pause in typing. */
export function saveDraftSoon(channelId: string, text: string): void {
  // One write is pending at a time: a draft for another channel is written before this one waits.
  if (pending && pending.channelId !== channelId) flushDraft();
  if (text) cache.set(channelId, text);
  else cache.delete(channelId);
  pending = { channelId, text };
  clearTimeout(timer);
  timer = setTimeout(flushDraft, DEBOUNCE_MS);
}

/** Write a pending draft at once (on a channel switch, or when the chat unmounts). */
export function flushDraft(): void {
  clearTimeout(timer);
  timer = undefined;
  if (!pending) return;
  const { channelId, text } = pending;
  pending = null;
  persist(channelId, text);
}

/** A message was sent from this channel: its draft is gone, and any write still waiting for it is dropped. */
export function discardDraft(channelId: string): void {
  if (pending?.channelId === channelId) {
    pending = null;
    clearTimeout(timer);
    timer = undefined;
  }
  persist(channelId, '');
}

/** Forget what is held in memory, as after a reload. Only for tests. */
export function resetDraftCache(): void {
  cache.clear();
  pending = null;
  clearTimeout(timer);
  timer = undefined;
}
