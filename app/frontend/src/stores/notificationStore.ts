import { createSignal } from 'solid-js';

export const [appError, setAppError] = createSignal<string | null>(null);
