import { createSignal } from 'solid-js';
import type { ScreenSources, WatchedShare } from '../types';

// Signals only, no imports from other stores: voiceStore and screenStore both need them.

/** We are sharing our screen. */
export const [isSharing, setIsSharing] = createSignal<boolean>(false);
/** The share we are watching, if any. */
export const [watching, setWatching] = createSignal<WatchedShare | null>(null);
export const [screenSources, setScreenSources] = createSignal<ScreenSources>({ monitors: [], windows: [] });

export function resetScreenState() {
  setIsSharing(false);
  setWatching(null);
}
