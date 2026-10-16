import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { render } from 'solid-js/web';
import { App } from './App';
import { setCurrentUser } from './stores/authStore';
import { resetSessionState } from './stores/sessionStore';
import { setActiveChannel, setChannels } from './stores/chatStore';
import { clearToasts } from './ui';

const transport = vi.hoisted(() => ({
  callbacks: new Map<string, (event: { payload: unknown }) => void>(),
  invoke: vi.fn(),
  listen: vi.fn(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: transport.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: transport.listen }));

let dispose: (() => void) | undefined;
beforeEach(() => {
  document.body.innerHTML = '<div id="root"></div>';
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  resetSessionState();
  clearToasts();
  setCurrentUser(null);
  setActiveChannel(null);
  setChannels([]);
  transport.callbacks.clear();
  transport.invoke.mockReset().mockImplementation(async (command) => command === 'list_channels' ? [] : null);
  transport.listen.mockReset().mockImplementation(async (name, callback) => {
    transport.callbacks.set(name, callback);
    return () => transport.callbacks.delete(name);
  });
});
afterEach(() => { dispose?.(); dispose = undefined; vi.restoreAllMocks(); vi.unstubAllGlobals(); });
const mount = () => { dispose = render(() => <App />, document.getElementById('root')!); };
const emit = (event: string, payload: unknown) => transport.callbacks.get(event)!({ payload });

it('waits for session listeners before showing sign-in', async () => {
  let finish!: (dispose: () => void) => void;
  transport.listen.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  mount();
  expect(document.querySelector('form')).toBeNull();
  finish(() => {});
  await vi.waitFor(() => expect(document.querySelector('form')).not.toBeNull());
  expect(transport.callbacks.has('authenticated')).toBe(false); // first registration was held above
  expect(transport.callbacks.has('tofu-required')).toBe(true);
});

it('AUD-019 renders disconnection, retry and server rejection messages', async () => {
  mount();
  await vi.waitFor(() => expect(document.querySelector('form')).not.toBeNull());
  setCurrentUser({ id: 'me', username: 'me', display_name: 'Me', role: 'admin' });
  emit('connection-lost', 'network unavailable');
  expect(document.body.textContent).toContain('Connection lost');
  emit('reconnecting', null);
  expect(document.body.textContent).toContain('Reconnecting');
  emit('reconnected', null);
  expect(document.body.textContent).not.toContain('Connection lost');
  emit('server-error', { code: 400, message: 'Message too long' });
  await vi.waitFor(() => expect(document.querySelector('.toast[role="alert"]')?.textContent).toContain('Message too long'));
  (document.querySelector('.toast [aria-label="Dismiss"]') as HTMLButtonElement).click();
  expect(document.querySelector('.toast[role="alert"]')).toBeNull();
  emit('reconnect-attempt', { attempt: 2, retry_in_secs: 4 });
  emit('reconnecting', null);
  expect(document.body.textContent).toContain('try 2, next one in 4 s');
});

it('AUD-054 creates a text channel through the sidebar', async () => {
  let created = false;
  const channel = { id: 'new', name: 'new-channel', channel_type: 'text', short_id: 2, position: 1, bitrate_kbps: 64 };
  transport.invoke.mockImplementation(async (command) => {
    if (command === 'get_current_user') return { id: 'me', username: 'me', display_name: 'Me', role: 'admin' };
    if (command === 'list_channels') return created ? [channel] : [];
    if (command === 'create_channel') { created = true; return channel; }
    if (command === 'fetch_channel_messages') return { channel_id: 'new', messages: [], has_more: false };
    return null;
  });
  mount();
  await vi.waitFor(() => expect(document.querySelector('[aria-label="Create text channel"]')).not.toBeNull());
  (document.querySelector('[aria-label="Create text channel"]') as HTMLButtonElement).click();
  const nameInput = await vi.waitFor(() => { const el = document.querySelector('[role="dialog"] input') as HTMLInputElement; expect(el).not.toBeNull(); return el; });
  nameInput.value = 'new-channel';
  nameInput.dispatchEvent(new InputEvent('input', { bubbles: true }));
  (Array.from(document.querySelectorAll('[role="dialog"] button')).find((b) => b.textContent === 'Create') as HTMLButtonElement).click();
  await vi.waitFor(() => expect(transport.invoke).toHaveBeenCalledWith('create_channel', {
    name: 'new-channel', channelType: 'text', topic: undefined, bitrateKbps: undefined,
  }));
  await vi.waitFor(() => expect(document.querySelector('.channel-item')?.textContent).toContain('new-channel'));
  expect(document.querySelector('[role="alert"]')).toBeNull();
});

it('AUD-037 hides channel creation from non-admins', async () => {
  transport.invoke.mockImplementation(async (command) => command === 'get_current_user'
    ? { id: 'me', username: 'me', display_name: 'Me', role: 'member' } : []);
  mount();
  await vi.waitFor(() => expect(document.querySelector('.sidebar')).not.toBeNull());
  expect(document.querySelector('[aria-label="Create text channel"]')).toBeNull();
  expect(document.querySelector('[aria-label="Create voice channel"]')).toBeNull();
});

it('cleans up subscriptions that finish after unmount', async () => {
  let finish!: (dispose: () => void) => void;
  const lateDispose = vi.fn();
  transport.listen.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  mount();
  dispose!(); dispose = undefined;
  finish(lateDispose);
  await vi.waitFor(() => expect(lateDispose).toHaveBeenCalledOnce());
  expect(transport.callbacks.size).toBe(0);
});

it('AUD-019 restores the running Rust session after a frontend reload', async () => {
  const room = { id: 'voice', short_id: 2, name: 'Voice', channel_type: 'voice', position: 0, bitrate_kbps: 64 };
  transport.invoke.mockImplementation(async (command) => {
    if (command === 'get_current_user') return { id: 'me', username: 'me', display_name: 'Me', role: 'member' };
    if (command === 'list_channels') return [room];
    if (command === 'get_session_snapshot') return {
      connected: true, user_id: 'me', session_peer_id: 10, current_voice_channel: 2,
      muted: true, deafened: false, pending_tofu: null,
      initial_state: { channels: [room], online_users: [
        { id: 'alice', username: 'alice', display_name: 'Alice Display', role: 'member', status: 'online' },
      ], voice_states: [{ user_id: 'alice', peer_id: 20, username: 'alice', channel_id: 'voice', muted: false, deafened: false, sharing_screen: false }] },
    };
    return null;
  });
  mount();
  await vi.waitFor(() => expect(document.querySelector('.sidebar')).not.toBeNull());
  await vi.waitFor(() => expect(document.querySelector('.online-users')?.textContent).toContain('Alice Display'));
  expect(document.querySelector('.voice-banner')?.textContent).toContain('Voice connected');
  expect(document.querySelector('[title="Unmute"]')).not.toBeNull();
  expect(document.body.textContent).not.toContain('Connection lost');
});
