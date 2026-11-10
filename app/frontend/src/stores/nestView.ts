import { createSignal } from 'solid-js';

// Optional "nest" view of the voice channels in the sidebar: every room is a nest, with its birds inside.

const KEY = 'finchspeak.nestView';

function read(): boolean {
  try {
    return localStorage.getItem(KEY) === '1';
  } catch {
    return false;
  }
}

export const [nestView, setNestViewSignal] = createSignal(read());

export function setNestView(enabled: boolean) {
  setNestViewSignal(enabled);
  try { localStorage.setItem(KEY, enabled ? '1' : '0'); } catch { /* not remembered */ }
}

/** How warm a nest looks: 0 empty, 1-3 by headcount, one step warmer while somebody speaks. */
export function nestLevel(count: number, speaking: boolean): 0 | 1 | 2 | 3 {
  if (count <= 0) return 0;
  const base = count >= 5 ? 3 : count >= 3 ? 2 : 1;
  return Math.min(3, base + (speaking ? 1 : 0)) as 1 | 2 | 3;
}
