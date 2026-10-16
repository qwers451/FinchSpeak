import { api } from '../services/api';

// Pictures need our login, so they are fetched through the backend and kept as blob URLs.
// The address of a picture changes with the picture, so the address is the cache key.

const LIMIT = 200;
const cache = new Map<string, Promise<string>>();

/** The user id inside a picture address such as `/api/users/<id>/avatar?v=1`. */
export function userIdOf(address: string): string | null {
  return /^\/api\/users\/([0-9a-f-]{36})\/avatar/i.exec(address)?.[1] ?? null;
}

/** A displayable URL for `address`: server addresses are fetched once, anything else (data:, blob:) is used as it is. */
export function avatarSrc(address: string): Promise<string> {
  const userId = userIdOf(address);
  if (!userId) return Promise.resolve(address);
  const known = cache.get(address);
  if (known) return known;
  const loading = api.downloadAvatar(userId)
    .then((bytes) => URL.createObjectURL(new Blob([bytes], { type: 'image/png' })))
    .catch((error) => { cache.delete(address); throw error; });
  cache.set(address, loading);
  if (cache.size > LIMIT) {
    const oldest = cache.keys().next().value;
    if (oldest !== undefined) {
      void cache.get(oldest)?.then((url) => URL.revokeObjectURL(url)).catch(() => {});
      cache.delete(oldest);
    }
  }
  return loading;
}

export function clearAvatarCache() {
  for (const loading of cache.values()) void loading.then((url) => URL.revokeObjectURL(url)).catch(() => {});
  cache.clear();
}
