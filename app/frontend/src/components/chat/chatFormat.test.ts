import { beforeEach, describe, expect, it } from 'vitest';
import type { Message } from '../../types';
import { buildRows, dayLabel, splitMentions } from './chatFormat';
import { DEFAULT_PREFS, setPrefs } from '../../stores/settingsStore';

beforeEach(() => setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'en' } }));

const at = (iso: string) => Math.floor(new Date(iso).getTime() / 1000);
const msg = (id: string, author: string, iso: string, extra: Partial<Message> = {}): Message => ({
  id, channel_id: 'c', author_id: author, author_name: author, content: id, attachments: [], timestamp: at(iso), ...extra,
});
const NOW = new Date('2026-10-16T12:00:00').getTime();

describe('buildRows', () => {
  it('puts a day separator before the first message and whenever the day changes', () => {
    const rows = buildRows([msg('a', 'x', '2026-10-15T23:50:00'), msg('b', 'x', '2026-10-16T00:10:00')], null, NOW);
    expect(rows.map((row) => row.kind)).toEqual(['day', 'message', 'day', 'message']);
    expect(rows.filter((row) => row.kind === 'day').map((row) => (row as { label: string }).label)).toEqual(['Yesterday', 'Today']);
  });

  it('shares one header between close messages of the same author only', () => {
    const rows = buildRows([
      msg('a', 'bob', '2026-10-16T10:00:00'),
      msg('b', 'bob', '2026-10-16T10:02:00'),
      msg('c', 'bob', '2026-10-16T10:30:00'),
      msg('d', 'carol', '2026-10-16T10:31:00'),
    ], null, NOW);
    const compact = rows.filter((row) => row.kind === 'message').map((row) => (row as { compact: boolean }).compact);
    expect(compact).toEqual([false, true, false, false]);
  });

  it('a reply and a failed message always show their header', () => {
    const rows = buildRows([
      msg('a', 'bob', '2026-10-16T10:00:00'),
      msg('b', 'bob', '2026-10-16T10:01:00', { reply_to: { message_id: 'a', author_name: 'bob', snippet: 'a' } }),
      msg('c', 'bob', '2026-10-16T10:02:00', { state: 'failed' }),
    ], null, NOW);
    expect(rows.filter((row) => row.kind === 'message').map((row) => (row as { compact: boolean }).compact)).toEqual([false, false, false]);
  });

  it('draws the "New messages" divider before the first unread, and breaks the group there', () => {
    const rows = buildRows([
      msg('a', 'bob', '2026-10-16T10:00:00'),
      msg('b', 'bob', '2026-10-16T10:01:00'),
      msg('c', 'bob', '2026-10-16T10:02:00'),
    ], 'b', NOW);
    expect(rows.map((row) => row.kind)).toEqual(['day', 'message', 'new', 'message', 'message']);
    expect((rows[3] as { compact: boolean }).compact).toBe(false);
    expect((rows[4] as { compact: boolean }).compact).toBe(true);
  });

  it('draws no divider when everything on screen is unread or the id is not on screen', () => {
    const list = [msg('a', 'bob', '2026-10-16T10:00:00')];
    expect(buildRows(list, 'a', NOW).some((row) => row.kind === 'new')).toBe(false);
    expect(buildRows(list, 'gone', NOW).some((row) => row.kind === 'new')).toBe(false);
  });
});

describe('dayLabel', () => {
  it('says Today and Yesterday, otherwise the date', () => {
    expect(dayLabel(at('2026-10-16T01:00:00'), NOW)).toBe('Today');
    expect(dayLabel(at('2026-10-15T23:00:00'), NOW)).toBe('Yesterday');
    expect(dayLabel(at('2026-10-02T12:00:00'), NOW)).toMatch(/2 Oct/);
    expect(dayLabel(at('2025-10-02T12:00:00'), NOW)).toMatch(/2025/);
  });
});

describe('splitMentions', () => {
  const lookup = (login: string) => ({ alice: 'u1', bob: 'u2' } as Record<string, string>)[login];

  it('marks known logins and leaves everything else as text', () => {
    expect(splitMentions('hi @Bob, meet @nobody and mail a@alice.example', lookup)).toEqual([
      { text: 'hi ' },
      { text: '@Bob', mention: { login: 'Bob', userId: 'u2' } },
      { text: ', meet @nobody and mail a@alice.example' },
    ]);
  });

  it('leaves the full stop of a sentence outside the mention', () => {
    expect(splitMentions('thanks @alice.', lookup)).toEqual([
      { text: 'thanks ' },
      { text: '@alice', mention: { login: 'alice', userId: 'u1' } },
      { text: '.' },
    ]);
  });

  it('returns the text unchanged when nothing matches, including odd input', () => {
    expect(splitMentions('nothing here', lookup)).toEqual([{ text: 'nothing here' }]);
    expect(splitMentions('@@@ @', lookup)).toEqual([{ text: '@@@ @' }]);
    expect(splitMentions('', lookup)).toEqual([{ text: '' }]);
  });
});
