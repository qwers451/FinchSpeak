import { createSignal } from 'solid-js';

// Why the server sent us away (design I2). A leaf module: no imports of other stores.

export type SessionEndKind = 'kicked' | 'banned' | 'signed-out' | 'expired' | 'password-changed';

export const [sessionEnded, setSessionEnded] = createSignal<SessionEndKind | null>(null);

/** Reads the reason text of a lost connection or a failed reconnect; `null` when it is only a network problem. */
export function classifyEnd(reason: string | null | undefined): SessionEndKind | null {
  const text = (reason ?? '').toLowerCase();
  if (text.includes('banned')) return 'banned';
  if (text.includes('kicked')) return 'kicked';
  if (text.includes('session expired')) return 'expired';
  if (text.includes('password changed')) return 'password-changed';
  if (text.includes('token revoked') || text.includes('revoked') || text.includes('invalid or missing token') || text.includes('authentication rejected')) return 'signed-out';
  return null;
}

export const END_TEXT: Record<SessionEndKind, { headline: string; body: string }> = {
  kicked: { headline: 'You were disconnected by an admin', body: 'You can sign in again whenever you like.' },
  banned: { headline: 'You have been banned from this server', body: 'Contact the server admin if you think this is a mistake.' },
  'signed-out': { headline: 'You were signed out', body: 'Your session was ended on another device or by an admin. Sign in again to continue.' },
  expired: { headline: 'Your session has expired', body: 'You have not used FinchSpeak on this computer for a long time. Sign in again.' },
  'password-changed': { headline: 'Your password was changed', body: 'Sign in with the new password.' },
};
