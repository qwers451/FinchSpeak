import { createRoot } from 'solid-js';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { setCurrentUser } from './authStore';
import { startGameActivitySync } from './activitySharing';
import { resetSessionState, sessionHandlers } from './sessionStore';
import { updatePrivacy } from './settingsStore';

let stop: (() => void) | undefined;
const state = { channels: [], online_users: [], voice_states: [] };
const sharing = vi.spyOn(api, 'setActivitySharing').mockResolvedValue();
const connect = (features: string[]) => sessionHandlers.authenticated({
  user_id: 'self', session_peer_id: 42, session_generation: 7, channels: [],
  initial_state: { ...state, features },
});

beforeEach(() => {
  sharing.mockClear();
  resetSessionState();
  setCurrentUser({ id: 'self', username: 'self', display_name: 'Self', role: 'member' });
  createRoot(() => { stop = startGameActivitySync(); });
});
afterEach(() => { stop?.(); resetSessionState(); });

it('sends a session-bound opt-out immediately and suspends sharing in DND', () => {
  connect(['activity']);
  expect(sharing.mock.calls.at(-1)?.slice(0, 3)).toEqual([7, 42, true]);
  updatePrivacy({ shareActivity: false });
  expect(sharing.mock.calls.at(-1)?.slice(0, 3)).toEqual([7, 42, false]);
  updatePrivacy({ shareActivity: true });
  sessionHandlers['presence-update']({ user_id: 'self', status: 'donotdisturb' });
  expect(sharing.mock.calls.at(-1)?.slice(0, 3)).toEqual([7, 42, false]);
  sessionHandlers['presence-update']({ user_id: 'self', status: 'online' });
  expect(sharing.mock.calls.at(-1)?.slice(0, 3)).toEqual([7, 42, true]);
  sessionHandlers['connection-lost']('lost');
  expect(sharing.mock.calls.at(-1)?.slice(0, 3)).toEqual([7, 42, false]);
  const revisions = sharing.mock.calls.map((call) => call[3]);
  expect(revisions.every((revision, i) => i === 0 || revision > revisions[i - 1])).toBe(true);
});

it('never enables polling when the server has no activity feature', () => {
  connect([]);
  expect(sharing.mock.calls.at(-1)?.slice(0, 3)).toEqual([7, 42, false]);
});
