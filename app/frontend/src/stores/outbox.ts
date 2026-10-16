// Messages that could not be sent are kept here so they survive a restart and can be sent again
// with the same client id (the server ignores a second copy of an id it already stored).
export interface OutboxEntry {
  client_msg_id: string;
  channel_id: string;
  content: string;
  attachments: string[];
  reply_to?: string | null;
  timestamp: number;
}

const KEY = 'finchspeak.outbox';
const MAX_ENTRIES = 50;

export function readOutbox(): OutboxEntry[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    return Array.isArray(parsed) ? parsed.filter((entry) => entry && typeof entry.client_msg_id === 'string' && typeof entry.channel_id === 'string') : [];
  } catch {
    return [];
  }
}

function write(entries: OutboxEntry[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(entries.slice(-MAX_ENTRIES)));
  } catch { /* storage unavailable: the message just does not survive a restart */ }
}

export function saveToOutbox(entry: OutboxEntry) {
  write([...readOutbox().filter((existing) => existing.client_msg_id !== entry.client_msg_id), entry]);
}

export function removeFromOutbox(clientMsgId: string) {
  const entries = readOutbox();
  if (entries.some((entry) => entry.client_msg_id === clientMsgId)) write(entries.filter((entry) => entry.client_msg_id !== clientMsgId));
}

export function outboxForChannel(channelId: string): OutboxEntry[] {
  return readOutbox().filter((entry) => entry.channel_id === channelId);
}

export function clearOutbox() {
  try { localStorage.removeItem(KEY); } catch { /* nothing to clear */ }
}
