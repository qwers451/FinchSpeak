import { describe, expect, it } from 'vitest';
import { parseVideoChunk } from './videoChunk';

describe('parseVideoChunk', () => {
  it('reads a key frame from an ArrayBuffer', () => {
    const message = new Uint8Array([1, 0x00, 0x00, 0x00, 0x01, 0x67]).buffer;
    const chunk = parseVideoChunk(message);
    expect(chunk).not.toBeNull();
    expect(chunk!.isKey).toBe(true);
    expect(Array.from(chunk!.data)).toEqual([0x00, 0x00, 0x00, 0x01, 0x67]);
  });

  it('reads a delta frame from a Uint8Array with a non-zero byteOffset', () => {
    const backing = new Uint8Array([9, 9, 9, 0, 0x41, 0x9a]);
    const view = new Uint8Array(backing.buffer, 3, 3);
    const chunk = parseVideoChunk(view);
    expect(chunk).not.toBeNull();
    expect(chunk!.isKey).toBe(false);
    expect(Array.from(chunk!.data)).toEqual([0x41, 0x9a]);
  });

  it('accepts a plain number array', () => {
    const chunk = parseVideoChunk([1, 7, 8]);
    expect(chunk).not.toBeNull();
    expect(chunk!.isKey).toBe(true);
    expect(Array.from(chunk!.data)).toEqual([7, 8]);
  });

  it('returns null for messages shorter than two bytes', () => {
    expect(parseVideoChunk(new ArrayBuffer(0))).toBeNull();
    expect(parseVideoChunk(new ArrayBuffer(1))).toBeNull();
    expect(parseVideoChunk(new Uint8Array([1]))).toBeNull();
    expect(parseVideoChunk([1])).toBeNull();
  });

  it('returns null for unknown inputs', () => {
    expect(parseVideoChunk('not a chunk')).toBeNull();
    expect(parseVideoChunk({ is_key: true, data: [1, 2, 3] })).toBeNull();
    expect(parseVideoChunk(null)).toBeNull();
    expect(parseVideoChunk(undefined)).toBeNull();
    expect(parseVideoChunk([1, 'x', 3])).toBeNull();
  });

  it('shares the buffer of the input instead of copying', () => {
    const buffer = new Uint8Array([0, 5, 6, 7]).buffer;
    const chunk = parseVideoChunk(buffer);
    expect(chunk).not.toBeNull();
    expect(chunk!.data.buffer).toBe(buffer);
    expect(chunk!.data.byteOffset).toBe(1);
    expect(chunk!.data.byteLength).toBe(3);
  });
});
