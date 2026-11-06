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
