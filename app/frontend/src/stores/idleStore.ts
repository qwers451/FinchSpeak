import { api } from '../services/api';
import { currentUser } from './authStore';
import { saveStatus } from './profileStore';

export const AUTO_IDLE_AFTER_S = 600;
const POLL_MS = 30_000;

/**
 * Sets the user to idle after AUTO_IDLE_AFTER_S seconds without keyboard or mouse input,
 * and back to online when input returns. Only the automatic idle is undone; a status the
 * user chose (do not disturb, or idle picked by hand) is never changed.
 * Returns a function that stops the polling.
 */
export function startAutoIdle(): () => void {
  let autoIdled = false;

  const poll = async () => {
    let idle: number;
    try {
      idle = await api.systemIdleSeconds();
    } catch (err) {
      console.warn('idle check failed', err);
      return;
    }
    const me = currentUser();
    if (!me) return;
    if (idle >= AUTO_IDLE_AFTER_S) {
      if (!autoIdled && me.status === 'online') {
        autoIdled = true;
        await saveStatus('idle', me.status_text ?? '');
      }
      return;
    }
    if (autoIdled) {
      autoIdled = false;
      if (me.status === 'idle') await saveStatus('online', me.status_text ?? '');
    }
  };

  const timer = setInterval(() => void poll(), POLL_MS);
  return () => clearInterval(timer);
}
