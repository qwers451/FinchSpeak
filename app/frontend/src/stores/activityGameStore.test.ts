import { afterEach, expect, it } from 'vitest';
import { gameOf } from './activityGameStore';
import { resetSessionState, sessionHandlers } from './sessionStore';
import { parsePrefs } from './settingsStore';

afterEach(resetSessionState);

it('enables game sharing for new and old preference files and validates the opt-out', () => {
  expect(parsePrefs(null).privacy.shareActivity).toBe(true);
  expect(parsePrefs('{"privacy":{"shareActivity":false}}').privacy.shareActivity).toBe(false);
  expect(parsePrefs('{"privacy":{"shareActivity":"false"}}').privacy.shareActivity).toBe(true);
});

it('restores activity from a snapshot, replaces it on events and clears it on disconnect', () => {
  sessionHandlers.authenticated({
    user_id: 'self', session_peer_id: 1, channels: [],
    initial_state: { channels: [], online_users: [], voice_states: [], features: ['activity'],
      activities: [{ user_id: 'alice', game: 'Portal' }] },
  });
  expect(gameOf('alice')).toBe('Portal');
  sessionHandlers['activity-changed']({ user_id: 'alice', game: 'Hades' });
  expect(gameOf('alice')).toBe('Hades');
  sessionHandlers['activity-changed']({ user_id: 'alice', game: null });
  expect(gameOf('alice')).toBeUndefined();
  sessionHandlers['activity-changed']({ user_id: 'alice', game: 'Portal' });
  sessionHandlers['connection-lost']('link lost');
  expect(gameOf('alice')).toBeUndefined();
});

it('drops previous server activities when a legacy server snapshot arrives', () => {
  sessionHandlers['activity-changed']({ user_id: 'alice', game: 'Portal' });
  sessionHandlers.authenticated({ user_id: 'self', session_peer_id: 1, channels: [],
    initial_state: { channels: [], online_users: [], voice_states: [] } });
  expect(gameOf('alice')).toBeUndefined();
});
