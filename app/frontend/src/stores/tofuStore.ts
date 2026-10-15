import { createSignal } from 'solid-js';

// Certificate the user is asked to confirm: { hostname, fingerprint } | null.
//
// A leaf module on purpose (no imports of other stores): `sessionStore` handlers use it at load
// time, and `authStore` imports `sessionStore`, so keeping it in `authStore` made a cycle that
// crashed the built app with "Cannot access ... before initialization".
export const [pendingTofu, setPendingTofu] = createSignal<{ hostname: string; fingerprint: string } | null>(null);
