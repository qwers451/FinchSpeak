/**
 * One video message from Rust: byte 0 holds the flags (bit 0 set = key frame), bytes 1.. are the
 * H.264 access unit. Returns null for anything that is not such a message.
 */
export function parseVideoChunk(message: unknown): { isKey: boolean; data: Uint8Array } | null {
  let bytes: Uint8Array;
  if (message instanceof ArrayBuffer) {
    bytes = new Uint8Array(message);
  } else if (ArrayBuffer.isView(message)) {
    bytes = new Uint8Array(message.buffer, message.byteOffset, message.byteLength);
  } else if (Array.isArray(message) && message.every((value) => typeof value === 'number')) {
    bytes = Uint8Array.from(message);
  } else {
    return null;
  }
  if (bytes.length < 2) return null;
  return { isKey: (bytes[0] & 1) === 1, data: bytes.subarray(1) };
}
