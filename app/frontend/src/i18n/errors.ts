import { t, type TranslationParams } from './index';

type Rule = { pattern: RegExp; key: string; params?: (match: RegExpMatchArray) => TranslationParams };

/** Backend errors arrive as English text. Order matters: the first matching rule wins. */
const RULES: readonly Rule[] = [
  { pattern: /^Cannot reach the server: ([\s\S]*)/i, key: 'Cannot reach the server: {reason}', params: (m) => ({ reason: m[1] }) },
  { pattern: /^Signed in, but the QUIC connection failed/i, key: 'Signed in, but the voice connection failed. Check that UDP port 4433 is open.' },
  { pattern: /^Registered, but the QUIC connection failed/i, key: 'Registered, but the voice connection failed. Check that UDP port 4433 is open.' },
  { pattern: /^Registration failed: ([\s\S]*)/i, key: 'Registration failed: {reason}', params: (m) => ({ reason: m[1] }) },
  { pattern: /^Login failed: ([\s\S]*)/i, key: 'Sign-in failed: {reason}', params: (m) => ({ reason: m[1] }) },
  { pattern: /is not trusted yet/i, key: 'Confirm the server certificate, then sign in.' },
  { pattern: /certificate of .* changed since you trusted it/i, key: 'The server certificate has changed. Do not sign in until the admin confirms the change.' },
  { pattern: /^Logged out locally, but could not revoke/i, key: 'Signed out on this computer. The server could not be reached to end the session.' },
  { pattern: /^Invalid server address/i, key: 'The server address is not valid' },
  { pattern: /^Not authenticated$/i, key: 'Not authenticated' },
];

/** The one place that turns a backend error into text in the interface language; unknown errors pass through unchanged. */
export function describeError(error: unknown): string {
  const text = String(error).replace(/^Error:\s*/, '');
  for (const rule of RULES) {
    const match = text.match(rule.pattern);
    if (match) return t(rule.key, rule.params?.(match));
  }
  return text;
}
