import { createSignal } from 'solid-js';

export type ToastKind = 'info' | 'success' | 'warn' | 'error';

export interface Toast {
  id: number;
  kind: ToastKind;
  title: string;
  text?: string;
  /** Optional button, e.g. "Retry". */
  action?: { label: string; run: () => void };
  /** Milliseconds until it closes by itself; errors stay until dismissed. */
  ttl: number | null;
}

/** At most this many are visible; the oldest give way. */
export const MAX_TOASTS = 3;
const DEFAULT_TTL_MS = 6000;

export const [toasts, setToasts] = createSignal<Toast[]>([]);
let nextId = 1;
const timers = new Map<number, ReturnType<typeof setTimeout>>();

export function dismissToast(id: number) {
  clearTimeout(timers.get(id));
  timers.delete(id);
  setToasts((list) => list.filter((toast) => toast.id !== id));
}

export function pushToast(input: {
  kind?: ToastKind;
  title: string;
  text?: string;
  action?: Toast['action'];
  sticky?: boolean;
}): number {
  const kind = input.kind ?? 'info';
  const ttl = input.sticky || kind === 'error' ? null : DEFAULT_TTL_MS;
  // The same message twice in a row is one toast, not two.
  const duplicate = toasts().find((toast) => toast.kind === kind && toast.title === input.title && toast.text === input.text);
  if (duplicate) return duplicate.id;

  const toast: Toast = { id: nextId++, kind, title: input.title, text: input.text, action: input.action, ttl };
  const next = [...toasts(), toast];
  for (const dropped of next.slice(0, Math.max(0, next.length - MAX_TOASTS))) {
    clearTimeout(timers.get(dropped.id));
    timers.delete(dropped.id);
  }
  setToasts(next.slice(-MAX_TOASTS));
  if (ttl !== null) timers.set(toast.id, setTimeout(() => dismissToast(toast.id), ttl));
  return toast.id;
}

export function clearToasts() {
  for (const timer of timers.values()) clearTimeout(timer);
  timers.clear();
  setToasts([]);
}
