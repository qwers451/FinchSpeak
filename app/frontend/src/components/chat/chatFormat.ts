import type { Message } from '../../types';
import { locale, t } from '../../i18n';

/** One line of the message list. */
export type Row =
  | { kind: 'day'; key: string; label: string }
  | { kind: 'new'; key: string }
  | { kind: 'message'; key: string; message: Message; compact: boolean };

/** Messages of one author this close together share a single header. */
export const GROUP_WINDOW_SECONDS = 5 * 60;

const seconds = (timestamp: number) => (timestamp > 1e12 ? Math.floor(timestamp / 1000) : timestamp);

function dayStart(timestamp: number): number {
  const date = new Date(seconds(timestamp) * 1000);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

/** "Today", "Yesterday", or "Mon, 14 Oct" (with the year when it is not this year). */
export function dayLabel(timestamp: number, now = Date.now()): string {
  const days = Math.round((dayStart(now / 1000) - dayStart(timestamp)) / 86_400_000);
  if (days === 0) return t('Today');
  if (days === 1) return t('Yesterday');
  const date = new Date(seconds(timestamp) * 1000);
  const sameYear = date.getFullYear() === new Date(now).getFullYear();
  return date.toLocaleDateString(locale() === 'ru' ? 'ru-RU' : 'en-GB', { weekday: 'short', day: 'numeric', month: 'short', ...(sameYear ? {} : { year: 'numeric' }) });
}

export function clock(timestamp: number): string {
  return new Date(seconds(timestamp) * 1000).toLocaleTimeString(locale() === 'ru' ? 'ru-RU' : 'en-GB', { hour: '2-digit', minute: '2-digit' });
}

export interface RowInfo {
  /** Label of a day separator to draw before this message. */
  day: string | null;
  /** The "New messages" divider goes before this message. */
  divider: boolean;
  /** Same author as the message above, shortly after: no avatar and name. */
  compact: boolean;
}

/**
 * What goes around `message`, given the message above it: a day separator when the day changes,
 * the "New messages" divider before `firstUnreadId` (never as the very first row), and whether it
 * is a compact follow-up of the same author.
 */
export function rowInfo(previous: Message | undefined, message: Message, firstUnreadId: string | null, now = Date.now()): RowInfo {
  const newDay = !previous || dayStart(previous.timestamp) !== dayStart(message.timestamp);
  const divider = firstUnreadId !== null && message.id === firstUnreadId && previous !== undefined;
  const compact = !newDay && !divider && previous !== undefined
    && previous.author_id === message.author_id
    && seconds(message.timestamp) - seconds(previous.timestamp) <= GROUP_WINDOW_SECONDS
    && !message.reply_to
    && previous.state !== 'failed' && message.state !== 'failed';
  return { day: newDay ? dayLabel(message.timestamp, now) : null, divider, compact };
}

/** All rows at once (used by tests; the chat computes `rowInfo` per message to keep items stable). */
export function buildRows(messages: Message[], firstUnreadId: string | null, now = Date.now()): Row[] {
  const rows: Row[] = [];
  let previous: Message | undefined;
  for (const message of messages) {
    const info = rowInfo(previous, message, firstUnreadId, now);
    if (info.day !== null) rows.push({ kind: 'day', key: `day-${dayStart(message.timestamp)}`, label: info.day });
    if (info.divider) rows.push({ kind: 'new', key: 'new-messages' });
    rows.push({ kind: 'message', key: message.id, message, compact: info.compact });
    previous = message;
  }
  return rows;
}

export type Segment = { text: string; mention?: { login: string; userId?: string } };

/**
 * Splits text into plain parts and `@login` mentions (same word-boundary rule as the server);
 * a mention only counts when `lookup` knows the login.
 */
export function splitMentions(text: string, lookup: (login: string) => string | undefined): Segment[] {
  const segments: Segment[] = [];
  const pattern = /(^|[^\p{L}\p{N}])@([\p{L}\p{N}_.-]{1,32})/gu;
  let cursor = 0;
  for (const match of text.matchAll(pattern)) {
    const login = match[2].replace(/[.-]+$/u, '');
    if (!login) continue;
    const userId = lookup(login.toLowerCase());
    if (!userId) continue;
    const start = (match.index ?? 0) + match[1].length;
    if (start > cursor) segments.push({ text: text.slice(cursor, start) });
    segments.push({ text: `@${login}`, mention: { login, userId } });
    cursor = start + 1 + login.length;
  }
  if (cursor < text.length) segments.push({ text: text.slice(cursor) });
  return segments.length > 0 ? segments : [{ text }];
}
