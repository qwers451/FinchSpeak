import { t, type TranslationParams } from './index';

type Rule = { pattern: RegExp; key: string; params?: (match: RegExpMatchArray) => TranslationParams };

/** Backend errors arrive as English text. Order matters: the first matching rule wins. */
const RULES: readonly Rule[] = [
  { pattern: /^Cannot reach the server: ([\s\S]*)/i, key: 'Cannot reach the server: {reason}', params: (m) => ({ reason: m[1] }) },
  { pattern: /^Signed in, but the QUIC connection failed/i, key: 'Signed in, but the voice connection failed. Check that UDP port 4433 is open.' },
  { pattern: /^Registered, but the QUIC connection failed/i, key: 'Registered, but the voice connection failed. Check that UDP port 4433 is open.' },
  // The reason may itself be a backend error ("HTTP 400: ..."), so it is translated too.
  { pattern: /^Registration failed: ([\s\S]*)/i, key: 'Registration failed: {reason}', params: (m) => ({ reason: describeError(m[1]) }) },
  // Too many sign-in attempts: the server names the wait in seconds, shown here in whole minutes. The Tauri
  // client prefixes "HTTP 429: "; the browser client sends the raw JSON body. Registration is not matched.
  { pattern: /^(?:Login failed: )?(?:HTTP 429: )?(?:\{"error":")?Too many attempts\. Try again in (\d+) seconds/, key: 'Too many sign-in attempts. Try again in {minutes} min', params: (m) => ({ minutes: Math.max(1, Math.ceil(Number(m[1]) / 60)) }) },
  { pattern: /^Login failed: ([\s\S]*)/i, key: 'Sign-in failed: {reason}', params: (m) => ({ reason: describeError(m[1]) }) },
  { pattern: /is not trusted yet/i, key: 'Confirm the server certificate, then sign in.' },
  { pattern: /certificate of .* changed since you trusted it/i, key: 'The server certificate has changed. Do not sign in until the admin confirms the change.' },
  { pattern: /^Logged out locally, but could not revoke/i, key: 'Signed out on this computer. The server could not be reached to end the session.' },
  { pattern: /^Invalid server address/i, key: 'The server address is not valid' },
  // The server limits joins and screen shares; each refusal names its action and carries the seconds to wait.
  { pattern: /^429: Wait (\d+) s; too many room joins/, key: 'Too many room joins. Wait {seconds} s', params: (m) => ({ seconds: m[1] }) },
  { pattern: /^429: Wait (\d+) s; too many screen shares/, key: 'Too many screen shares. Wait {seconds} s', params: (m) => ({ seconds: m[1] }) },
  { pattern: /^Not authenticated$/i, key: 'Not authenticated' },
  // Room music errors from the server (and the client's own format probe).
  { pattern: /^Music file is too large/i, key: 'The music file is too large' },
  { pattern: /^Unsupported music format/i, key: 'This music format is not supported. Use MP3, OGG, FLAC or WAV.' },
  { pattern: /^(Music library|Temporary music) quota exceeded/i, key: 'The music quota is used up. Remove a track and try again.' },
  { pattern: /^Music queue is full/i, key: 'The music queue is full. Wait for a track to finish.' },
  { pattern: /^Music is (disabled|unavailable)/i, key: 'Music is not enabled on this server.' },
  { pattern: /^The password must be at least 8 characters$/i, key: 'The password must be at least 8 characters' },
  // A QUIC connection closed without a reason from the server (quinn's own text).
  { pattern: /^connection lost$/i, key: 'connection lost' },
  // Session end reasons from the server (shown in the connection banner).
  { pattern: /^Session revoked$/i, key: 'Session revoked' },
  { pattern: /^Invalid token$/i, key: 'Invalid token' },
];

/** The one place that turns a backend error into text in the interface language; unknown errors pass through unchanged. */
export function describeError(error: unknown): string {
  const text = String(error).replace(/^Error:\s*/, '');
  if (!text.trim()) return t('Something went wrong');
  // An HTTP status from the client ("HTTP 400: <server text>"): keep the code, translate the text.
  const http = text.match(/^(HTTP \d{3}: )([\s\S]*)$/);
  if (http) return http[1] + describeError(http[2]);
  for (const rule of RULES) {
    const match = text.match(rule.pattern);
    if (match) return t(rule.key, rule.params?.(match));
  }
  return text;
}
