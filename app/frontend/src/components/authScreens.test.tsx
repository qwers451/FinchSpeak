import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const transport = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: transport.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));

import { AuthModal, describeAuthError } from './AuthModal';
import { CertChangedModal, TofuModal, diffBytes, fingerprintGroups } from './TofuModal';
import { loginUser, retryPendingSignIn, setPendingMismatch, setPendingTofu } from '../stores/authStore';
import { passwordStrength } from '../ui/passwordStrength';

let dispose: (() => void) | undefined;
const mount = (view: () => any) => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(view, host);
  return host;
};

beforeEach(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  transport.invoke.mockReset().mockResolvedValue(null);
});
afterEach(() => {
  dispose?.();
  dispose = undefined;
  setPendingTofu(null);
  setPendingMismatch(null);
  document.body.innerHTML = '';
  localStorage.clear();
});

const FP = Array.from({ length: 32 }, (_, i) => i.toString(16).padStart(2, '0').toUpperCase()).join(':');

it('splits a fingerprint into rows of eight bytes', () => {
  const groups = fingerprintGroups(FP);
  expect(groups).toHaveLength(4);
  expect(groups[0].split(':')).toHaveLength(8);
});

it('marks the bytes that differ', () => {
  expect(diffBytes('AA:BB:CC', 'AA:00:CC').map((b) => b.differs)).toEqual([false, true, false]);
});

it('describes errors in plain words', () => {
  expect(describeAuthError('Error: Login failed: invalid username or password').title).toBe('Invalid username or password');
  expect(describeAuthError('This account is banned').icon).toBe('ban');
  const unreachable = describeAuthError('Cannot reach the server: connection refused');
  expect(unreachable.tone).toBe('amber');
  expect(unreachable.retry).toBe(true);
});

it('rates passwords by length and variety', () => {
  expect(passwordStrength('').score).toBe(0);
  expect(passwordStrength('abc').score).toBe(1);
  expect(passwordStrength('Tr0ub4dor&3-long-enough').score).toBe(4);
});

it('first-use dialog trusts the shown fingerprint only on the button', async () => {
  setPendingTofu({ hostname: 'chat.finch.example', fingerprint: FP });
  const host = mount(() => <TofuModal />);
  expect(host.textContent).toContain('chat.finch.example');
  expect(transport.invoke).not.toHaveBeenCalled();
  const trust = Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.includes('trust server'))!;
  trust.click();
  await vi.waitFor(() => expect(transport.invoke).toHaveBeenCalledWith('trust_fingerprint', { hostname: 'chat.finch.example', fingerprint: FP }));
});

it('changed certificate: trusting needs the typed server name and Esc does not close it', async () => {
  setPendingMismatch({ hostname: 'chat.finch.example', expected: FP, actual: FP.replace('00', 'FF') });
  const host = mount(() => <CertChangedModal />);
  const trustNew = Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.includes('Trust the new certificate'))!;
  expect(trustNew.disabled).toBe(true);
  host.querySelector('.overlay')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  expect(host.querySelector('[role="dialog"]')).not.toBeNull();

  const input = host.querySelector('input')!;
  input.value = 'chat.finch.example';
  input.dispatchEvent(new InputEvent('input', { bubbles: true }));
  expect(trustNew.disabled).toBe(false);
  trustNew.click();
  await vi.waitFor(() => expect(transport.invoke).toHaveBeenCalledWith('forget_server_fingerprint', { hostname: 'chat.finch.example' }));
});

it('sign-in form remembers address and login but never the password', async () => {
  transport.invoke.mockResolvedValue({ id: 'u', username: 'alice', display_name: 'Alice', role: 'member' });
  const host = mount(() => <AuthModal />);
  const [address, user, pass] = Array.from(host.querySelectorAll('input'));
  for (const [el, value] of [[address, 'https://x.example:8443'], [user, 'alice'], [pass, 'hunter2hunter2']] as const) {
    el.value = value;
    el.dispatchEvent(new InputEvent('input', { bubbles: true }));
  }
  host.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }));
  await vi.waitFor(() => expect(localStorage.getItem('finchspeak.login')).not.toBeNull());
  const stored = localStorage.getItem('finchspeak.login')!;
  expect(stored).toContain('alice');
  expect(stored).toContain('x.example');
  expect(stored).not.toContain('hunter2');
});

it('stay signed in is on by default and is passed to login', async () => {
  const { api } = await import('../services/api');
  const spy = vi.spyOn(api, 'login').mockResolvedValue({ id: 'u', username: 'alice', display_name: 'Alice', role: 'member' });
  const host = mount(() => <AuthModal />);
  const [, user, pass] = Array.from(host.querySelectorAll('input'));
  for (const [el, value] of [[user, 'alice'], [pass, 'hunter2hunter2']] as const) {
    el.value = value;
    el.dispatchEvent(new InputEvent('input', { bubbles: true }));
  }
  const toggle = host.querySelector('[role="switch"]')!;
  expect(toggle.getAttribute('aria-checked')).toBe('true');
  host.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }));
  await vi.waitFor(() => expect(spy).toHaveBeenCalled());
  expect(spy.mock.calls[0][3]).toBe(true);
  spy.mockRestore();
});

it('trusting the server retries the sign-in once', async () => {
  const user = { id: 'u', username: 'alice', display_name: 'Alice', role: 'member' };
  let logins = 0;
  transport.invoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'login') {
      logins += 1;
      if (logins === 1) {
        setPendingTofu({ hostname: 'h', fingerprint: FP });
        throw 'Server certificate of h is not trusted yet';
      }
      return user;
    }
    return null;
  });
  expect(await loginUser('alice', 'hunter2hunter2', false)).toBe(false);
  const host = mount(() => <TofuModal />);
  const trust = Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.includes('trust server'))!;
  trust.click();
  await vi.waitFor(() => expect(logins).toBe(2));
  const calls = transport.invoke.mock.calls.filter(([cmd]) => cmd === 'login');
  expect(calls[1][1]).toMatchObject({ username: 'alice', password: 'hunter2hunter2', remember: false });
});

it('a failed sign-in other than the certificate prompt forgets the password', async () => {
  transport.invoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'login') throw 'Login failed: invalid username or password';
    return null;
  });
  expect(await loginUser('alice', 'wrong-password')).toBe(false);
  expect(await retryPendingSignIn()).toBe(false);
  expect(transport.invoke.mock.calls.filter(([cmd]) => cmd === 'login')).toHaveLength(1);
});

it('dismissing the certificate prompt forgets the password', async () => {
  transport.invoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'login') {
      setPendingTofu({ hostname: 'h', fingerprint: FP });
      throw 'Server certificate of h is not trusted yet';
    }
    return null;
  });
  await loginUser('alice', 'hunter2hunter2');
  const host = mount(() => <TofuModal />);
  Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.includes('Cancel'))!.click();
  expect(await retryPendingSignIn()).toBe(false);
  expect(transport.invoke.mock.calls.filter(([cmd]) => cmd === 'login')).toHaveLength(1);
});

it('keeps the attempt when the certificate event arrives after the rejection', async () => {
  let logins = 0;
  transport.invoke.mockImplementation(async (cmd: string) => {
    if (cmd !== 'login') return null;
    logins += 1;
    if (logins === 1) throw 'Server certificate of h is not trusted yet';
    return { id: 'u', username: 'alice', display_name: 'Alice', role: 'member' };
  });
  await loginUser('alice', 'hunter2hunter2');
  setPendingTofu({ hostname: 'h', fingerprint: FP });
  const host = mount(() => <TofuModal />);
  Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.includes('trust server'))!.click();
  await vi.waitFor(() => expect(logins).toBe(2));
});

// ---- invite links: the link only prefills the form and pins its fingerprint; trust still goes through the dialog's confirm

const INVITE_FP = FP;

it('a matching invite link prefills server and code and opens register mode', async () => {
  const { setPendingInvite } = await import('../stores/inviteLink');
  const { setServerUrl, serverUrl } = await import('../stores/authStore');
  const before = serverUrl();
  setPendingInvite({ server: 'https://chat.finch.example:8443', code: 'K7M2QX9TPW4B', fp: INVITE_FP });
  const host = mount(() => <AuthModal />);
  expect(host.textContent).toContain('Join the flock');
  const inputs = Array.from(host.querySelectorAll('input')) as HTMLInputElement[];
  expect(inputs[0].value).toBe('https://chat.finch.example:8443');
  expect(inputs.some((el) => el.value === 'K7M2QX9TPW4B')).toBe(true);
  expect(transport.invoke).not.toHaveBeenCalled();
  setPendingInvite(null);
  setServerUrl(before);
});

it('submitting with an empty server address shows the plain error and does not sign in', async () => {
  const { setServerUrl, serverUrl } = await import('../stores/authStore');
  const before = serverUrl();
  setServerUrl('');
  const host = mount(() => <AuthModal />);
  host.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }));
  expect(host.textContent).toContain('Enter the server address');
  expect(transport.invoke.mock.calls.filter(([cmd]) => cmd === 'login' || cmd === 'register')).toHaveLength(0);
  setServerUrl(before);
});

it('mismatched_link_fingerprint_is_not_trusted', async () => {
  const { setPendingInvite } = await import('../stores/inviteLink');
  setPendingInvite({ server: 'https://h:8443', code: 'K7M2QX9TPW4B', fp: FP.replace('00', 'FF') });
  setPendingTofu({ hostname: 'h', fingerprint: FP });
  const host = mount(() => <TofuModal />);
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(transport.invoke.mock.calls.filter(([cmd]) => cmd === 'trust_fingerprint')).toHaveLength(0);
  expect(host.querySelector('[role="dialog"]')).not.toBeNull();
  expect(host.textContent).toContain('The fingerprint in the invite link does not match this server.');
  setPendingInvite(null);
});

it('a matching invite link trusts the server through the same confirm path, without the dialog', async () => {
  const { setPendingInvite } = await import('../stores/inviteLink');
  const user = { id: 'u', username: 'alice', display_name: 'Alice', role: 'member' };
  let logins = 0;
  transport.invoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'login') {
      logins += 1;
      if (logins === 1) {
        setPendingTofu({ hostname: 'h', fingerprint: FP });
        throw 'Server certificate of h is not trusted yet';
      }
      return user;
    }
    return null;
  });
  setPendingInvite({ server: 'https://h:8443', code: 'K7M2QX9TPW4B', fp: FP.toLowerCase().replace(/:/g, '') });
  mount(() => <TofuModal />);
  expect(await loginUser('alice', 'hunter2hunter2', false)).toBe(false);
  await vi.waitFor(() => expect(logins).toBe(2));
  expect(transport.invoke).toHaveBeenCalledWith('trust_fingerprint', { hostname: 'h', fingerprint: FP });
  setPendingInvite(null);
});

it('changing the server to another host forgets the invite link', async () => {
  const { setPendingInvite, pendingInvite } = await import('../stores/inviteLink');
  const { setServerUrl, serverUrl } = await import('../stores/authStore');
  const before = serverUrl();
  setPendingInvite({ server: 'https://chat.finch.example:8443', code: 'K7M2QX9TPW4B', fp: INVITE_FP });
  const host = mount(() => <AuthModal />);
  const address = host.querySelector('input') as HTMLInputElement;
  address.value = 'https://other.example:8443';
  address.dispatchEvent(new InputEvent('input', { bubbles: true }));
  expect(pendingInvite()).toBeNull();
  setServerUrl(before);
});

it('a matching invite link does not show the trust dialog while it trusts the server', async () => {
  const { setPendingInvite } = await import('../stores/inviteLink');
  transport.invoke.mockImplementation(async (cmd: string) => (cmd === 'trust_fingerprint' ? new Promise(() => {}) : null));
  setPendingInvite({ server: 'https://h:8443', code: 'K7M2QX9TPW4B', fp: FP });
  setPendingTofu({ hostname: 'h', fingerprint: FP });
  const host = mount(() => <TofuModal />);
  await vi.waitFor(() => expect(transport.invoke).toHaveBeenCalledWith('trust_fingerprint', { hostname: 'h', fingerprint: FP }));
  expect(host.querySelector('[role="dialog"]')).toBeNull();
  setPendingInvite(null);
});
