import { afterEach, expect, it } from 'vitest';
import type { SessionEvents } from '../types';
import { resetSessionState, restoreSessionSnapshot, serverFeatures, sessionHandlers } from './sessionStore';

const initialState = { channels: [], online_users: [], voice_states: [], unread: [] };
const authenticated: SessionEvents['authenticated'] = {
  user_id: 'self', session_peer_id: 1, channels: [], initial_state: initialState,
};

afterEach(resetSessionState);

it('keeps announced features through authentication and clears them when the connection ends', () => {
  sessionHandlers.authenticated({ ...authenticated, initial_state: { ...initialState, features: ['activity'] } });
  expect(serverFeatures()).toEqual(['activity']);
  sessionHandlers['connection-lost']('link lost');
  expect(serverFeatures()).toEqual([]);
});

it('replaces the previous server features when a legacy snapshot has no features field', () => {
  sessionHandlers.authenticated({ ...authenticated, initial_state: { ...initialState, features: ['activity'] } });
  restoreSessionSnapshot({
    connected: true, initial_state: initialState, user_id: 'self', session_peer_id: 1,
    current_voice_channel: null, muted: false, deafened: false, pending_tofu: null,
  });
  expect(serverFeatures()).toEqual([]);
});
