import { expect, it, vi } from 'vitest';

/**
 * The real entry (`index.tsx`) loads `App` first. The stores import each other, and a table that
 * reads another store's export while the module is still being evaluated gets `undefined` in
 * Vitest and throws "Cannot access ... before initialization" in the built bundle (a blank app).
 * Loading the entry first, as production does, makes that visible.
 */
it('every session handler is a function when the app entry is loaded first', async () => {
  vi.resetModules();
  await import('./App');
  const { sessionHandlers } = await import('./stores/sessionStore');

  for (const [event, handler] of Object.entries(sessionHandlers)) {
    expect(typeof handler, `handler for "${event}"`).toBe('function');
  }
});
