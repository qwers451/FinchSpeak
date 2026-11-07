import { createSignal } from 'solid-js';
import type { SessionEvents, ScreenSources, WatchedShare } from '../types';

// Signals only, no imports from other stores: voiceStore and screenStore both need them.

export const [screenAudioSource, setScreenAudioSource] = createSignal<SessionEvents['screen-audio-source']['source'] | null>(null);

/** Local backend generation; end events from another share do not affect this one. */
export const [screenShareGeneration, setScreenShareGeneration] = createSignal<number | null>(null);
export const [lastEndedScreenShareGeneration, setLastEndedScreenShareGeneration] = createSignal(0);

/** We are sharing our screen. */
export const [isSharing, setIsSharing] = createSignal<boolean>(false);
/** What our own share shows, for the "You are sharing" plate. */
export const [sharingInfo, setSharingInfo] = createSignal<{ label: string; audio: boolean } | null>(null);
/** The share we are watching, if any. */
export const [watching, setWatching] = createSignal<WatchedShare | null>(null);
/** A share we were watching ended: kept on screen for a few seconds so people know why. */
export const [endedShare, setEndedShare] = createSignal<{ user_id: string; peer_id: number; name: string; reason: 'stopped' | 'left' | 'disconnected' | 'room-closed' } | null>(null);
export const [screenSources, setScreenSources] = createSignal<ScreenSources>({ monitors: [], windows: [] });

/** Invalidates pending starts when a stop, disconnect or natural end wins the race. */
export const [screenShareEpoch, setScreenShareEpoch] = createSignal(0);
export function endLocalScreenShare() {
  setScreenShareEpoch((epoch) => epoch + 1);
  setIsSharing(false);
  setScreenShareGeneration(null);
  setScreenAudioSource(null);
  setSharingInfo(null);
}

/** Our own share ends with the connection; what we watch is resumed after a reconnect. */
export function resetScreenStateOnDisconnect() {
  endLocalScreenShare();
}

export function resetScreenState() {
  endLocalScreenShare();
  setLastEndedScreenShareGeneration(0);
  setWatching(null);
  setEndedShare(null);
}
