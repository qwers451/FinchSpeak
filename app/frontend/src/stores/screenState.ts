import { createSignal } from 'solid-js';
import type { ScreenSources, WatchedShare } from '../types';

// Signals only, no imports from other stores: voiceStore and screenStore both need them.

/** We are sharing our screen. */
export const [isSharing, setIsSharing] = createSignal<boolean>(false);
/** What our own share shows, for the "You are sharing" plate. */
export const [sharingInfo, setSharingInfo] = createSignal<{ label: string; audio: boolean } | null>(null);
/** The share we are watching, if any. */
export const [watching, setWatching] = createSignal<WatchedShare | null>(null);
/** A share we were watching ended: kept on screen for a few seconds so people know why. */
export const [endedShare, setEndedShare] = createSignal<{ user_id: string; peer_id: number; name: string; reason: 'stopped' | 'left' | 'disconnected' | 'room-closed' } | null>(null);
export const [screenSources, setScreenSources] = createSignal<ScreenSources>({ monitors: [], windows: [] });

/** Our own share ends with the connection; what we watch is resumed after a reconnect. */
export function resetScreenStateOnDisconnect() {
  setIsSharing(false);
  setSharingInfo(null);
}

export function resetScreenState() {
  setIsSharing(false);
  setSharingInfo(null);
  setWatching(null);
  setEndedShare(null);
}
