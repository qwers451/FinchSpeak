import type { SessionEvents } from '../types';

const elements = new Map<number, Set<HTMLElement>>();
const active = new Map<number, { value: string; timeout: ReturnType<typeof setTimeout> }>();

export function bindLevel(el: HTMLElement, peerId: number): () => void {
  const bound = elements.get(peerId) ?? new Set<HTMLElement>();
  bound.add(el);
  elements.set(peerId, bound);
  el.style.setProperty('--lvl', active.get(peerId)?.value ?? '0');
  return () => {
    el.style.removeProperty('--lvl');
    bound.delete(el);
    if (bound.size === 0) elements.delete(peerId);
  };
}

function writeLevel(peerId: number, value: string) {
  const bound = elements.get(peerId);
  if (!bound) return;
  for (const el of bound) el.style.setProperty('--lvl', value);
}

function clearLevel(peerId: number) {
  clearTimeout(active.get(peerId)?.timeout);
  active.delete(peerId);
  writeLevel(peerId, '0');
}

/** Apply speech levels directly to the avatars, outside Solid's reactive graph. */
export function updateVoiceLevels({ levels }: SessionEvents['voice-levels']) {
  if (!levels.length) {
    for (const peerId of active.keys()) clearLevel(peerId);
    return;
  }
  for (const [peerId, level] of levels) {
    clearTimeout(active.get(peerId)?.timeout);
    const value = String(level / 100);
    active.set(peerId, { value, timeout: setTimeout(() => clearLevel(peerId), 200) });
    writeLevel(peerId, value);
  }
}
