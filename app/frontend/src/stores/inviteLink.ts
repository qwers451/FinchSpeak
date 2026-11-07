import { createSignal } from 'solid-js';

// Invite links look like `finchspeak://join?server=<urlencoded https URL>&code=<invite code>&fp=<sha256 fingerprint>`.
// Everything in a link is untrusted input: it only prefills the register form and supplies the fingerprint
// that the certificate dialog compares against. Nothing is submitted or trusted from here.
//
// A leaf module on purpose (no imports of other stores), like tofuStore.ts.

export type InviteLink = { server: string; code: string; fp: string | null };

/** The link received from the OS (or from the app's own admin page), waiting for the register form. */
export const [pendingInvite, setPendingInvite] = createSignal<InviteLink | null>(null);

const MAX_SERVER_LENGTH = 2048;
const CODE_PATTERN = /^[A-Za-z0-9_-]{1,64}$/;
const FINGERPRINT_PATTERN = /^[0-9A-F]{64}$/;

/** Fingerprints compare without case, spaces and colons: `AA:BB` equals `aabb` and `AA BB`. */
export function normalizeFingerprint(fingerprint: string): string {
  return fingerprint.replace(/[\s:]/g, '').toUpperCase();
}

export function fingerprintsMatch(expected: string | null, actual: string): boolean {
  if (!expected) return false;
  const left = normalizeFingerprint(expected);
  return left.length > 0 && left === normalizeFingerprint(actual);
}

/** Returns the parts of a `finchspeak://join` link, or null for anything else (other schemes, http://, bad values). */
export function parseInviteLink(url: string): InviteLink | null {
  if (typeof url !== 'string' || url.length > 4096) return null;
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.protocol !== 'finchspeak:') return null;
  // `finchspeak://join?...` keeps `join` in the host; `finchspeak:join?...` in the path.
  if ((parsed.host + parsed.pathname).replace(/\/+$/, '') !== 'join') return null;

  const server = (parsed.searchParams.get('server') ?? '').trim();
  if (!server || server.length > MAX_SERVER_LENGTH) return null;
  let serverUrl: URL;
  try {
    serverUrl = new URL(server);
  } catch {
    return null;
  }
  if (serverUrl.protocol !== 'https:') return null;
  // Only the origin is kept: user name, password, path and query of the address are dropped.
  const origin = serverUrl.origin;

  const code = (parsed.searchParams.get('code') ?? '').trim();
  if (!CODE_PATTERN.test(code)) return null;

  // A malformed fingerprint does not make the link useless: it simply pins nothing, so the normal dialog shows.
  const normalized = normalizeFingerprint(parsed.searchParams.get('fp') ?? '');
  const fp = FINGERPRINT_PATTERN.test(normalized) ? normalized.match(/.{2}/g)!.join(':') : null;
  return { server: origin, code, fp };
}

/** True when a request for `hostname` at `address` is the very server the link names: same host and same port. */
export function linkCoversServer(link: InviteLink, hostname: string, address: string): boolean {
  try {
    const target = new URL(address.trim());
    return target.origin === link.server && target.hostname === hostname;
  } catch {
    return false;
  }
}

/** True when a typed server address points at another host than the link, so the link's invite no longer applies. */
export function linkServerDiffers(link: InviteLink | null, server: string): boolean {
  if (!link) return false;
  try {
    return new URL(server.trim()).host !== new URL(link.server).host;
  } catch {
    return true;
  }
}

/** The link the admin copies: the exact format parseInviteLink reads back. */
export function buildInviteLink(server: string, code: string, fp: string): string {
  return `finchspeak://join?server=${encodeURIComponent(server)}&code=${code}&fp=${fp}`;
}

/** Stores a link the app received; a link that does not parse is ignored. */
export function receiveInviteLink(url: string): InviteLink | null {
  const link = parseInviteLink(url);
  if (link) setPendingInvite(link);
  return link;
}
