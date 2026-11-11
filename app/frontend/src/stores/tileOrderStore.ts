import { createSignal } from 'solid-js';

const KEY = 'finchspeak.tileOrder';
function readOrder(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    if (Array.isArray(value)) return [...new Set(value.filter((id): id is string =>
      typeof id === 'string' && /^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/i.test(id)))];
  } catch { /* default to join order */ }
  return [];
}
export const [tileOrder, setTileOrder] = createSignal<string[]>(readOrder());
export const [draggedTile, setDraggedTile] = createSignal<string | null>(null);

export function moveTile(userId: string, targetId: string, after: boolean, roomIds: string[]) {
  if (userId === targetId || !roomIds.includes(userId) || !roomIds.includes(targetId)) return;
  const next = [...new Set([...tileOrder(), ...roomIds])].filter((id) => id !== userId);
  next.splice(next.indexOf(targetId) + Number(after), 0, userId);
  setTileOrder(next);
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch { /* keep the order for this session */ }
}
