import { createSignal } from 'solid-js';

// The profile dialog's signals live in a leaf module so the sign-out in authStore can close it
// without importing profileStore (which imports authStore).
export const [profileOpen, setProfileOpen] = createSignal(false);
export const [profileMessage, setProfileMessage] = createSignal<string | null>(null);
