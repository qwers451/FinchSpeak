import { createEffect, createRoot } from 'solid-js';
import { api } from '../services/api';
import { connectionStatus, localPeerId, myStatus, nativeSessionGeneration, serverFeatures } from './sessionStore';
import { prefs } from './settingsStore';

// A fresh window starts after commands issued by the previous window.
let revision = Date.now() * 1000;

/** Bind every native privacy change to the authenticated session that produced it. */
export function startGameActivitySync(): () => void {
  return createRoot((dispose) => {
    createEffect(() => {
      const generation = nativeSessionGeneration();
      const peer = localPeerId();
      const enabled = connectionStatus() === 'connected' && serverFeatures().includes('activity')
        && prefs().privacy.shareActivity && myStatus() !== 'donotdisturb';
      if (generation === null || peer === null) return;
      void api.setActivitySharing(generation, peer, enabled, ++revision)
        .catch((error) => console.warn('Could not update game activity sharing', error));
    });
    return dispose;
  });
}
