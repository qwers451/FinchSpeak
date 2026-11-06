import { describe, expect, it } from 'vitest';
import { buildInviteLink, fingerprintsMatch, parseInviteLink } from './inviteLink';

const FP = Array.from({ length: 32 }, (_, i) => i.toString(16).padStart(2, '0').toUpperCase()).join(':');

describe('invite links', () => {
  it('round-trips server, code and fingerprint through the finchspeak://join format', () => {
    const link = buildInviteLink('https://chat.example.com:8443', 'K7M2QX9TPW4B', FP);
    expect(link).toBe(`finchspeak://join?server=https%3A%2F%2Fchat.example.com%3A8443&code=K7M2QX9TPW4B&fp=${FP}`);
    expect(parseInviteLink(link)).toEqual({ server: 'https://chat.example.com:8443', code: 'K7M2QX9TPW4B', fp: FP });
  });

  it('rejects plain http, other schemes and other actions', () => {
    expect(parseInviteLink(`finchspeak://join?server=${encodeURIComponent('http://chat.example.com')}&code=K7M2`)).toBeNull();
    expect(parseInviteLink(`https://join?server=${encodeURIComponent('https://chat.example.com')}&code=K7M2`)).toBeNull();
    expect(parseInviteLink(`javascript://join?server=${encodeURIComponent('https://chat.example.com')}&code=K7M2`)).toBeNull();
    expect(parseInviteLink(`finchspeak://leave?server=${encodeURIComponent('https://chat.example.com')}&code=K7M2`)).toBeNull();
  });

  it('rejects links without a usable server or code and garbage input', () => {
    expect(parseInviteLink('finchspeak://join?code=K7M2')).toBeNull();
    expect(parseInviteLink('finchspeak://join?server=not-a-url&code=K7M2')).toBeNull();
    expect(parseInviteLink(`finchspeak://join?server=${encodeURIComponent('https://chat.example.com')}`)).toBeNull();
    expect(parseInviteLink(`finchspeak://join?server=${encodeURIComponent('https://chat.example.com')}&code=<script>`)).toBeNull();
    expect(parseInviteLink('not a link at all')).toBeNull();
  });

  it('accepts a link without a fingerprint and returns fp as null', () => {
    expect(parseInviteLink(`finchspeak://join?server=${encodeURIComponent('https://chat.example.com')}&code=K7M2`)).toEqual({
      server: 'https://chat.example.com', code: 'K7M2', fp: null,
    });
  });

  it('keeps a link whose fingerprint is malformed, without a fingerprint', () => {
    expect(parseInviteLink(`finchspeak://join?server=${encodeURIComponent('https://chat.example.com')}&code=K7M2&fp=zz-not-hex`)).toEqual({
      server: 'https://chat.example.com', code: 'K7M2', fp: null,
    });
  });

  it('keeps only the origin of the server address', () => {
    const link = parseInviteLink(`finchspeak://join?server=${encodeURIComponent('https://user:pw@chat.example.com:8443/path?x=1')}&code=K7M2`);
    expect(link?.server).toBe('https://chat.example.com:8443');
  });

  it('compares fingerprints ignoring case, spaces and colons', () => {
    expect(fingerprintsMatch(FP, FP.toLowerCase().replace(/:/g, ' '))).toBe(true);
    expect(fingerprintsMatch(FP, FP.replace('00', 'FF'))).toBe(false);
    expect(fingerprintsMatch(null, FP)).toBe(false);
  });
});
