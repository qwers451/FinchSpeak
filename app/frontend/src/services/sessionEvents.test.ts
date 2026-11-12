import { beforeEach, expect, it, vi } from 'vitest';
import * as events from './sessionEvents';
import * as session from '../stores/sessionStore';
import { allVoiceParticipants, setActiveVoiceChannel, voiceParticipants, isLocalSpeaking, setIsLocalSpeaking, micTestPhase, setMicTestPhase, micLevelDb, setMicLevelDb } from '../stores/voiceStore';
import { channels } from '../stores/chatStore';
import { api } from './api';
import { messages, setActiveChannel, setMessages } from '../stores/chatStore';

const transport = vi.hoisted(() => ({ callbacks: new Map<string, (event: { payload: unknown }) => void>(), disposers: [] as ReturnType<typeof vi.fn>[], invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: transport.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (name, callback) => {
  transport.callbacks.set(name, callback);
  const dispose = vi.fn(() => transport.callbacks.delete(name));
  transport.disposers.push(dispose);
  return dispose;
}) }));

beforeEach(() => {
  vi.restoreAllMocks();
  session.resetSessionState();
  transport.invoke.mockReset().mockResolvedValue(null);
  setActiveChannel(null);
  vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
  transport.callbacks.clear();
  transport.disposers.length = 0;
});

it('AUD-019 fetches profiles for users who connect after our initial snapshot', async () => {
  const { dispose } = await events.subscribeSessionEvents();
  emit('authenticated', { session_peer_id: 10, user_id: 'me', channels: [], initial_state: { channels: [], online_users: [], voice_states: [] } });
  transport.invoke.mockResolvedValue({ connected: true, user_id: 'me', session_peer_id: 10,
    current_voice_channel: null, muted: false, deafened: false, pending_tofu: null,
    initial_state: { channels: [], voice_states: [], online_users: [
      { id: 'alice', username: 'alice', display_name: 'Alice Display', role: 'member', status: 'online' },
    ] },
  });
  emit('presence-update', { user_id: 'alice', status: 'online' });
  await vi.waitFor(() => expect(session.onlineUsers()).toMatchObject([{ id: 'alice', display_name: 'Alice Display' }]));
  dispose();
});

it('keeps offline profiles and presence from the authenticated snapshot', async () => {
  const { dispose } = await events.subscribeSessionEvents();
  emit('authenticated', { session_peer_id: 10, user_id: 'me', channels: [], initial_state: {
    channels: [], voice_states: [], online_users: [],
    offline_users: [{ id: 'alice', username: 'alice', display_name: 'Alice', role: 'member', status: 'offline' }],
  } });
  expect(session.knownUsers()).toMatchObject([{ id: 'alice', status: 'offline' }]);
  expect(session.presence()).toEqual({ alice: 'offline' });
  expect(session.onlineUsers()).toEqual([]);
  dispose();
});

it('requests a missing profile once and keeps existing users and channels', async () => {
  const { dispose } = await events.subscribeSessionEvents();
  const room = { id: 'room', short_id: 1, name: 'room', channel_type: 'voice', position: 0, bitrate_kbps: 64 };
  const alice = { id: 'alice', username: 'alice', display_name: 'Alice', role: 'member', status: 'online' };
  emit('authenticated', { session_peer_id: 10, user_id: 'me', channels: [], initial_state: {
    channels: [room], voice_states: [], online_users: [alice],
  } });
  transport.invoke.mockResolvedValue({ connected: true, user_id: 'me', session_peer_id: 10,
    current_voice_channel: null, muted: false, deafened: false, pending_tofu: null,
    initial_state: { channels: [], voice_states: [], online_users: [
      { ...alice, display_name: 'Stale Alice' },
    ], offline_users: [{ ...alice, id: 'bob', username: 'bob', display_name: 'Bob', status: 'offline' }] },
  });
  emit('presence-update', { user_id: 'bob', status: 'online' });
  emit('presence-update', { user_id: 'bob', status: 'online' });
  await vi.waitFor(() => expect(session.knownUsers().map((user) => user.id)).toEqual(['alice', 'bob']));
  expect(transport.invoke.mock.calls.filter(([command]) => command === 'get_session_snapshot')).toHaveLength(1);
  expect(session.knownUsers()[0].display_name).toBe('Alice');
  expect(session.presence()).toEqual({ alice: 'online', bob: 'online' });
  expect(channels().map((channel) => channel.id)).toEqual(['room']);
  dispose();
});

it('fetches a second profile when the pending snapshot predates its presence update', async () => {
  const { dispose } = await events.subscribeSessionEvents();
  const initial_state = { channels: [], voice_states: [], online_users: [] };
  emit('authenticated', { session_peer_id: 10, user_id: 'me', channels: [], initial_state });
  const bob = { id: 'bob', username: 'bob', display_name: 'Bob', role: 'member', status: 'online' };
  const snapshot = { connected: true, user_id: 'me', session_peer_id: 10, current_voice_channel: null,
    muted: false, deafened: false, pending_tofu: null, initial_state: { ...initial_state, online_users: [bob] },
  };
  let finish!: (value: unknown) => void;
  transport.invoke.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  transport.invoke.mockResolvedValue({ ...snapshot, initial_state: { ...snapshot.initial_state,
    offline_users: [{ id: 'cara', username: 'cara', display_name: 'Cara', role: 'member', status: 'offline' }],
  } });
  emit('presence-update', { user_id: 'bob', status: 'online' });
  emit('presence-update', { user_id: 'cara', status: 'offline' });
  const requests = () => transport.invoke.mock.calls.filter(([command]) => command === 'get_session_snapshot');
  expect(requests()).toHaveLength(1);
  finish(snapshot);
  await vi.waitFor(() => expect(session.knownUsers().map((user) => user.id)).toEqual(['bob', 'cara']));
  expect(requests()).toHaveLength(2);
  dispose();
});

it('drops an older profile response when the same account signs in again', async () => {
  const { dispose } = await events.subscribeSessionEvents();
  const initial_state = { channels: [], voice_states: [], online_users: [] };
  const auth = { session_peer_id: 10, user_id: 'me', channels: [], initial_state };
  emit('authenticated', auth);
  const bob = { id: 'bob', username: 'bob', display_name: 'Bob', role: 'member', status: 'online' };
  const snapshot = { connected: true, user_id: 'me', session_peer_id: 10, current_voice_channel: null,
    muted: false, deafened: false, pending_tofu: null, initial_state: { ...initial_state, online_users: [bob] },
  };
  let finish!: (value: unknown) => void;
  transport.invoke.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  transport.invoke.mockResolvedValue(snapshot);
  emit('presence-update', { user_id: 'bob', status: 'online' });
  session.resetSessionState();
  emit('authenticated', auth);
  emit('presence-update', { user_id: 'bob', status: 'online' });
  finish({ ...snapshot, initial_state: { ...initial_state, online_users: [{ ...bob, display_name: 'Old Bob' }] } });
  await vi.waitFor(() => expect(session.knownUsers()).toMatchObject([{ id: 'bob', display_name: 'Bob' }]));
  dispose();
});

it('waits ten seconds before retrying a missing profile', async () => {
  vi.useFakeTimers();
  const { dispose } = await events.subscribeSessionEvents();
  try {
    emit('authenticated', { session_peer_id: 10, user_id: 'me', channels: [], initial_state: { channels: [], voice_states: [], online_users: [] } });
    const update = () => emit('presence-update', { user_id: 'bob', status: 'online' });
    const requests = () => transport.invoke.mock.calls.filter(([command]) => command === 'get_session_snapshot');
    update();
    await vi.advanceTimersByTimeAsync(9999);
    update();
    expect(requests()).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    update();
    expect(requests()).toHaveLength(2);
  } finally {
    dispose();
    vi.useRealTimers();
  }
});

it('replays voice and presence changes received while a snapshot is in flight', async () => {
  const { dispose, refresh } = await events.subscribeSessionEvents();
  const alice = { id: 'alice', username: 'alice', display_name: 'Alice', role: 'member', status: 'online' };
  const voice = { user_id: 'alice', peer_id: 20, username: 'alice', channel_id: 'room', muted: false, deafened: false, sharing_screen: false };
  emit('authenticated', { session_peer_id: 10, user_id: 'me', channels: [], initial_state: { channels: [], online_users: [alice], voice_states: [voice] } });
  let finish!: (value: unknown) => void;
  transport.invoke.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  emit('presence-update', { user_id: 'bob', status: 'online' });
  const refreshed = refresh();
  emit('presence-update', { user_id: 'alice', status: 'offline' });
  emit('user-left-voice', { user_id: 'alice', peer_id: 20, channel_id: 'room' });
  const room = { id: 'room', short_id: 1, name: 'room', channel_type: 'voice', position: 0, bitrate_kbps: 64 };
  finish({ connected: true, user_id: 'me', session_peer_id: 10, current_voice_channel: null,
    muted: false, deafened: false, pending_tofu: null,
    initial_state: { channels: [room], voice_states: [voice], online_users: [alice,
      { id: 'bob', username: 'bob', display_name: 'Bob', role: 'member', status: 'online' },
    ] },
  });
  await refreshed;
  expect(transport.invoke.mock.calls.filter(([command]) => command === 'get_session_snapshot')).toHaveLength(1);
  expect(channels()).toMatchObject([{ id: 'room' }]);
  expect(session.onlineUsers().map((user) => user.id)).toEqual(['bob']);
  expect(allVoiceParticipants()).toEqual([]);
  dispose();
});

it('refreshes active chat after reconnect so messages sent during an outage appear', async () => {
  vi.spyOn(api, 'fetchChannelMessages').mockResolvedValue({ channel_id: 'chat', has_more: false, messages: [
    { id: 'missed', author_id: 'alice', author_name: 'Alice', content: 'sent while offline', attachments: [], timestamp: 2 },
  ] });
  const { dispose } = await events.subscribeSessionEvents();
  setActiveChannel({ id: 'chat', short_id: 1, name: 'chat', channel_type: 'text', position: 0, bitrate_kbps: 64 });
  setMessages([]);
  emit('reconnected', null);
  await vi.waitFor(() => expect(messages().map((message) => message.id)).toEqual(['missed']));
  dispose();
});

const emit = (name: string, payload: unknown) => {
  const callback = transport.callbacks.get(name);
  expect(callback, `missing listener: ${name}`).toBeDefined();
  callback!({ payload });
};

it('AUD-019 restores voice, channels and online users from authenticated state', async () => {
  const { dispose } = await events.subscribeSessionEvents();
  emit('authenticated', {
    session_peer_id: 10, user_id: 'me', channels: [],
    initial_state: {
      channels: [{ id: 'room', short_id: 1, name: 'room', channel_type: 'voice', position: 0, bitrate_kbps: 64 }],
      online_users: [{ id: 'alice', username: 'alice', display_name: 'Alice', role: 'member', status: 'online', avatar_url: null }],
      voice_states: [{ user_id: 'alice', peer_id: 20, username: 'alice', channel_id: 'room', muted: true, deafened: false, sharing_screen: false }],
    },
  });
  expect(channels().map((channel) => channel.id)).toEqual(['room']);
  expect(allVoiceParticipants()).toMatchObject([{ peer_id: 20, is_muted: true }]);
  expect(session.onlineUsers()).toMatchObject([{ id: 'alice', status: 'online' }]);
  emit('presence-update', { user_id: 'alice', status: 'offline' });
  expect(session.onlineUsers()).toEqual([]);
  dispose();
});

it('AUD-019 handles remote voice changes and moves between rooms', async () => {
  const { dispose } = await events.subscribeSessionEvents();
  setActiveVoiceChannel({ id: 'room', short_id: 1, name: 'room', channel_type: 'voice', position: 0, bitrate_kbps: 64 });
  emit('user-joined-voice', { user_id: 'bob', peer_id: 30, username: 'bob', channel_id: 'room' });
  emit('user-voice-state-changed', { peer_id: 30, muted: true, deafened: true });
  expect(voiceParticipants().find((participant) => participant.peer_id === 30)).toMatchObject({ is_muted: true, is_deafened: true });
  emit('user-joined-voice', { user_id: 'bob', peer_id: 30, username: 'bob', channel_id: 'another' });
  expect(voiceParticipants().some((participant) => participant.peer_id === 30)).toBe(false);
  emit('user-left-voice', { user_id: 'bob', peer_id: 30, channel_id: 'room' });
  expect(allVoiceParticipants().find((participant) => participant.peer_id === 30)?.channel_id).toBe('another');
  dispose();
});

it('AUD-019 exposes connection failures and server errors and disposes listeners', async () => {
  const { dispose } = await events.subscribeSessionEvents();
  emit('connection-lost', 'network interrupted');
  expect(session.connectionStatus()).toBe('disconnected');
  emit('reconnecting', null);
  expect(session.connectionStatus()).toBe('reconnecting');
  emit('reconnected', null);
  expect(session.connectionStatus()).toBe('connected');
  emit('reconnect-failed', 'token rejected');
  expect(session.connectionStatus()).toBe('failed');
  expect(session.connectionError()).toBe('token rejected');
  emit('server-error', { code: 400, message: 'Message too long' });
  expect(session.appError()).toContain('Message too long');
  dispose();
  expect(transport.callbacks.size).toBe(0);
  expect(transport.disposers.every((unlisten) => unlisten.mock.calls.length === 1)).toBe(true);
});


it('reports native processing failure, clears local audio activity, and unsubscribes', async () => {
  const { dispose } = await events.subscribeSessionEvents();
  setIsLocalSpeaking(true);
  setMicTestPhase('recording');
  setMicLevelDb(-12);
  emit('audio-processing-failed', { message: 'WebRTC capture failed (status -1001)' });
  expect(session.appError()).toContain('WebRTC capture failed');
  expect(session.appError()).toContain('Rejoin');
  expect(isLocalSpeaking()).toBe(false);
  expect(micTestPhase()).toBe('idle');
  expect(micLevelDb()).toBeLessThanOrEqual(-70);
  dispose();
  expect(transport.callbacks.has('audio-processing-failed')).toBe(false);
});
