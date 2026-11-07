import { api } from '../services/api';
import { currentUser, setCurrentUser } from './authStore';
import { myStatus, setPresence } from './sessionStore';
import type { UserStatus } from '../types';

export const AUTO_IDLE_AFTER_S = 600;
const POLL_MS = 30_000;

/**
 * Sends an automatic status change without an error banner: a failure is logged and
 * the caller retries on the next poll. On success the local user is updated too, so the
 * change does not depend on the presence echo from the server arriving.
 */
async function sendQuietly(status: UserStatus): Promise<boolean> {
  const me = currentUser();
  if (!me) return false;
  try {
    await api.setStatus(status, me.status_text || null);
    const now = currentUser();
    if (now) setCurrentUser({ ...now, status });
    setPresence((previous) => ({ ...previous, [me.id]: status }));
    return true;
  } catch (err) {
    console.warn('automatic status not saved', err);
    return false;
  }
}

/**
 * Sets the user to idle after AUTO_IDLE_AFTER_S seconds without keyboard or mouse input,
 * and back to online when input returns. A do-not-disturb or offline status chosen since
 * then is kept. A manual "idle" chosen after an automatic one is treated as automatic
 * (reverted on activity): an accepted trade-off.
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
      if (!autoIdled && myStatus() === 'online') {
        autoIdled = await sendQuietly('idle');
      }
      return;
    }
    if (!autoIdled) return;
    const status = myStatus();
    if (status === 'donotdisturb' || status === 'offline') {
      autoIdled = false;
      return;
    }
    // Stays set when the save fails, so the next poll retries it.
    autoIdled = !(await sendQuietly('online'));
  };

  const timer = setInterval(() => void poll(), POLL_MS);
  return () => clearInterval(timer);
}
