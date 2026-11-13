import { createEffect, createRoot, createSignal } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import { emitTo, listen } from '@tauri-apps/api/event';
import type { User, VoiceParticipant } from '../types';
import { gameOf } from './activityGameStore';
import { knownUsers, localPeerId, sessionUserId } from './sessionStore';
import { activeVoiceChannel, isLocalSpeaking, isMuted, speakingPeers, voiceParticipants } from './voiceStore';
import { prefs, updateOverlay, type OverlayPrefs } from './settingsStore';

// The mini window: the main window tells it who is in the room, the mini window only shows that.

export interface OverlayPerson {
  name: string;
  avatar_url: string | null;
  speaking: boolean;
  muted: boolean;
}

/** The payload of the `overlay-state` event. `room` is null when I am not in a voice room. */
export interface OverlayState {
  room: string | null;
  people: OverlayPerson[];
  me_muted: boolean;
}

export interface OverlayInput {
  room: { name: string } | null;
  participants: VoiceParticipant[];
  users: User[];
  localPeerId: number | null;
  speakingPeers: Set<number>;
  localSpeaking: boolean;
  localMuted: boolean;
}

/** What the mini window shows, built from the room and the people in it. */
export function buildOverlayState(input: OverlayInput): OverlayState {
  if (!input.room) return { room: null, people: [], me_muted: input.localMuted };
  const people = input.participants.map((participant): OverlayPerson => {
    const user = input.users.find((candidate) => candidate.id === participant.user_id);
    const mine = participant.peer_id === input.localPeerId;
    return {
      name: user?.display_name || user?.username || participant.username,
      avatar_url: user?.avatar_url ?? null,
      speaking: mine ? input.localSpeaking : input.speakingPeers.has(participant.peer_id),
      muted: mine ? input.localMuted : Boolean(participant.is_muted || participant.server_muted),
    };
  });
  return { room: input.room.name, people, me_muted: input.localMuted };
}

export type AutoAction = 'show' | 'hide' | 'none';

/** Show when my game starts (if the setting is on); hide when it stops only if the window was shown for it. */
export function autoOverlayAction(input: { autoInGame: boolean; inGame: boolean; visible: boolean; shownAutomatically: boolean }): AutoAction {
  if (input.autoInGame && input.inGame && !input.visible) return 'show';
  if (!input.inGame && input.visible && input.shownAutomatically) return 'hide';
  return 'none';
}

export const [overlayOpen, setOverlayOpen] = createSignal(false);
/** True while the window is open because of a game, so closing the game closes it again. */
let shownAutomatically = false;

/** Whether I am playing something right now (the same game detection the members list uses). */
export const myGameRunning = () => {
  const me = sessionUserId();
  return me ? gameOf(me) !== undefined : false;
};

export async function showOverlay(automatic = false) {
  shownAutomatically = automatic;
  await invoke('overlay_show');
  await setOverlayClickThrough(prefs().overlay.clickThrough);
  setOverlayOpen(true);
}

export async function hideOverlay() {
  shownAutomatically = false;
  setOverlayOpen(false);
  await invoke('overlay_hide');
}

export async function toggleOverlay() {
  if (overlayOpen()) await hideOverlay();
  else await showOverlay();
}

export async function setOverlayClickThrough(enabled: boolean) {
  try {
    await invoke('overlay_set_click_through', { enabled });
  } catch {
    // The mini window may not exist yet; it gets the setting when it is shown.
  }
}

/** Runs the automatic show/hide rule against the current game and settings. */
export async function applyAutoOverlay(): Promise<AutoAction> {
  const action = autoOverlayAction({
    autoInGame: prefs().overlay.autoInGame,
    inGame: myGameRunning(),
    visible: overlayOpen(),
    shownAutomatically,
  });
  if (action === 'show') await showOverlay(true).catch(() => { shownAutomatically = false; });
  if (action === 'hide') await hideOverlay();
  return action;
}

export function setOverlayPref(change: Partial<OverlayPrefs>) {
  updateOverlay(change);
  if (change.clickThrough !== undefined) void setOverlayClickThrough(change.clickThrough);
}

let started = false;

/** Called once by the main window: keeps the mini window up to date and runs the game rule. */
export async function startOverlaySync() {
  if (started) return;
  started = true;
  createRoot(() => {
    createEffect(() => {
      if (!overlayOpen()) return;
      const state = buildOverlayState({
        room: activeVoiceChannel(),
        participants: voiceParticipants(),
        users: knownUsers(),
        localPeerId: localPeerId(),
        speakingPeers: speakingPeers(),
        localSpeaking: isLocalSpeaking(),
        localMuted: isMuted(),
      });
      void emitTo('overlay', 'overlay-state', state).catch(() => undefined);
    });
    createEffect(() => {
      // Tracks the game, the setting and the window; the action itself is a plain call.
      void applyAutoOverlay();
    });
  });
  try {
    await listen<boolean>('overlay-visibility', (event) => {
      setOverlayOpen(event.payload);
      if (!event.payload) shownAutomatically = false;
    });
  } catch {
    // Outside the desktop app there is no mini window to follow.
  }
}
