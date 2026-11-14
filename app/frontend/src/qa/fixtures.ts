import type { Channel, Message, MusicQueueEntry, MusicTrack, SoundInfo, User, VoiceParticipant } from '../types';

// Fixture data for the visual audit (dev only). Long names are on purpose: they are what overflows.

const LONG = 'Очень-длинное-название-канала-для-проверки-усечения-текста';
export const ME = 'u-00';

const NAMES = [
  'Михаил Заметаев', 'Anastasia-Konstantinopolskaya-Wolfeschlegelstein', 'Ян', 'Екатерина Великолепная-Прекрасная', 'Bob',
  'Дмитрий Сергеевич Нехорошев', 'Zoë', 'Всеволод Иванович Мирошниченко-Задунайский', 'Al', 'Людмила Петровна', 'Игорь', 'Кристофер Робин',
];

export const users: User[] = NAMES.map((name, i) => ({
  id: `u-${String(i).padStart(2, '0')}`,
  username: name.split(' ')[0].toLowerCase().slice(0, 18),
  display_name: name,
  role: i === 0 ? 'admin' : i === 1 ? 'moderator' : 'member',
  status: i % 4 === 3 ? 'offline' : 'online',
}));

export const channels: Channel[] = [
  { id: 'c-1', short_id: 1, name: 'общий', channel_type: 'text', position: 0, topic: 'Тема канала, которая достаточно длинная, чтобы не поместиться в строку заголовка окна', bitrate_kbps: 0 },
  { id: 'c-2', short_id: 2, name: LONG, channel_type: 'text', position: 1, bitrate_kbps: 0 },
  { id: 'c-3', short_id: 3, name: 'новости', channel_type: 'text', position: 2, bitrate_kbps: 0 },
  { id: 'v-1', short_id: 4, name: 'Голосовая комната', channel_type: 'voice', position: 3, bitrate_kbps: 64 },
  { id: 'v-2', short_id: 5, name: 'Очень длинная голосовая комната для совещаний и прочих долгих разговоров', channel_type: 'voice', position: 4, bitrate_kbps: 64 },
  { id: 'v-3', short_id: 6, name: 'Тихая', channel_type: 'voice', position: 5, bitrate_kbps: 64 },
];

const longText = 'Это очень длинное сообщение без пробелов: ' + 'абвгдеёжзийклмнопрстуфхцчшщъыьэюя'.repeat(6) + ' и ещё обычный текст, который переносится по словам как положено, чтобы проверить, что строки не вылезают за края пузыря.';
const url = 'https://example.com/a/very/long/path/that/goes/on/and/on/and/on/and/on/and/on/and/on/and/on/and/on/file.html?query=' + 'x'.repeat(80);

export function messages(): Message[] {
  const base = Date.now() - 3600_000;
  const m = (i: number, author: number, content: string, extra: Partial<Message> = {}): Message => ({
    id: `m-${i}`, channel_id: 'c-1', author_id: users[author].id, author_name: users[author].display_name, content,
    attachments: [], timestamp: base + i * 60_000, ...extra,
  });
  return [
    m(1, 1, 'Привет всем!'),
    m(2, 7, longText),
    m(3, 3, url),
    m(4, 0, 'Ответ на длинное сообщение', { reply_to: { message_id: 'm-2', author_name: users[7].display_name, snippet: longText.slice(0, 200), author_id: users[7].id } }),
    m(5, 2, 'Вложения', { attachments: [
      { id: 'a1', filename: 'очень-длинное-имя-файла-с-отчётом-за-прошлый-квартал-финальная-версия-v12-исправленная.pdf', size_bytes: 2_400_000, content_type: 'application/pdf', url: 'about:blank' },
      { id: 'a2', filename: 'notes.txt', size_bytes: 1200, content_type: 'text/plain', url: 'about:blank' },
    ] }),
    m(6, 4, 'Реакции', { reactions: [
      { emoji: '👍', user_ids: users.slice(0, 5).map((u) => u.id) }, { emoji: '🎉', user_ids: ['u-01'] }, { emoji: '❤️', user_ids: ['u-02', 'u-03'] },
      { emoji: '😂', user_ids: ['u-02'] }, { emoji: '🔥', user_ids: ['u-02'] }, { emoji: '👀', user_ids: ['u-02'] }, { emoji: '🙏', user_ids: ['u-02'] }, { emoji: '✅', user_ids: ['u-02'] },
    ] }),
    m(7, 0, 'Мой ответ, отредактированный', { edited_at: base + 8 * 60_000 }),
    m(8, 5, 'Не отправлено', { state: 'failed', failure: 'Сервер недоступен, попробуйте ещё раз через несколько секунд или проверьте подключение' }),
    ...Array.from({ length: 30 }, (_, i) => m(10 + i, i % 6, `Сообщение номер ${i + 1}`)),
  ];
}

export function participants(count: number, room = 'v-1'): VoiceParticipant[] {
  return users.slice(0, count).map((u, i) => ({
    user_id: u.id, peer_id: i + 1, username: u.display_name, channel_id: room,
    is_muted: i % 3 === 1, is_deafened: i % 5 === 4, is_speaking: i === 2, is_sharing: i === 3, server_muted: false,
  }));
}

const track = (i: number, title: string, artist: string | null = 'Исполнитель'): MusicTrack => ({
  id: `t-${i}`, uploader_id: users[i % 3].id, title, artist, duration_ms: 90_000 + i * 17_000, size_bytes: 4_000_000, format: 'mp3', temporary: false, created_at: Date.now() - i * 1000,
});
const LONG_TITLE = 'Симфония номер девять ре минор, опус 125, финал: «Ода к радости» в исполнении оркестра и хора — запись 1963 года (ремастер)';

export function library(count: number): MusicTrack[] {
  return Array.from({ length: count }, (_, i) => track(i, i % 4 === 0 ? LONG_TITLE : `Трек ${i + 1}`, i % 3 === 0 ? null : 'Группа с очень-длинным-названием-коллектива-для-проверки'));
}

export const queueEntries = (count: number): MusicQueueEntry[] => Array.from({ length: count }, (_, i) => ({
  entry_id: `e-${i}`, added_by: users[i % 4].id, track: track(i, i === 0 ? LONG_TITLE : `Трек в очереди ${i + 1}`),
}));

export const sounds: SoundInfo[] = Array.from({ length: 24 }, (_, i) => ({
  id: `s-${i}`, name: i % 5 === 0 ? 'Очень длинное название звука для проверки' : `Звук ${i + 1}`, emoji: i % 2 ? '🔔' : null, uploader_id: users[i % 3].id, duration_ms: 2000,
}));
