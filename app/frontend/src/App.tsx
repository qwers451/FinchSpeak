import { createEffect, createMemo, createSignal, onMount, onCleanup, Show } from 'solid-js';
import { currentUser, setCurrentUser, resumeLastSession, resuming } from './stores/authStore';
import { activeChannel, loadChannels } from './stores/chatStore';
import { setAppError } from './stores/sessionStore';
import { subscribeSessionEvents } from './services/sessionEvents';
import { receiveInviteLink } from './stores/inviteLink';
import { api } from './services/api';
import { Sidebar } from './components/Sidebar';
import { prefs } from './stores/settingsStore';
import { startDesktopSync } from './stores/desktop';
import { restoreAudioDevices, restoreVoiceSettings } from './stores/voiceStore';
import { MembersPanel } from './components/MembersPanel';
import { ChatArea } from './components/ChatArea';
import { VoicePanel } from './components/VoicePanel';
import { AuthModal } from './components/AuthModal';
import { CertChangedModal, TofuModal } from './components/TofuModal';
import { ConnectionBanner, Splash } from './components/ConnectionBanner';
import { Overlays, SessionEndedScreen } from './components/overlays/Overlays';
import { sessionEnded } from './stores/sessionEnd';
import { IconSprite, Titlebar, ToastHost } from './ui';
import { AdminModal } from './components/admin/AdminModal';
import { SettingsModal } from './components/settings/SettingsModal';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { startUpdateChecks } from './stores/updateStore';
import { startAutoIdle } from './stores/idleStore';
import { t } from './i18n';
import { describeError } from './i18n/errors';

export function App() {
  let unlisten: UnlistenFn | undefined;
  let disposed = false;
  const [eventsReady, setEventsReady] = createSignal(false);

  onMount(async () => {
    startDesktopSync();
    try {
      const subscriptions = await subscribeSessionEvents();
      if (disposed) { subscriptions.dispose(); return; }
      unlisten = subscriptions.dispose;
      // A finchspeak:// link that started the app arrived before this window listened: take it now.
      const waitingLink = await api.takeInviteLink();
      if (waitingLink) receiveInviteLink(waitingLink);
      // Keys work system-wide, so register them at start, not only on joining a room.
      void restoreAudioDevices(); // the Voice settings page and the out-of-call mic test use the saved devices
      void restoreVoiceSettings();
      const user = await api.getCurrentUser();
      if (disposed) return;
      if (user) {
        setCurrentUser(user);
        await loadChannels();
        await subscriptions.refresh();
      } else {
        await resumeLastSession(() => subscriptions.refresh());
      }
      setEventsReady(true);
    } catch (e) {
      if (!disposed) setAppError(describeError(e));
    } finally {
      if (!disposed && unlisten) setEventsReady(true);
    }
  });

  // Background update checks and automatic idle run only while somebody is signed in.
  const signedIn = createMemo(() => !!currentUser());
  createEffect(() => {
    if (!signedIn()) return;
    onCleanup(startUpdateChecks());
    onCleanup(startAutoIdle());
  });

  onCleanup(() => {
    disposed = true;
    unlisten?.();
  });

  return (
    <div class="shell">
      <IconSprite />
      <Titlebar />
      <ConnectionBanner />
      <Show when={eventsReady() && !resuming()} fallback={<Splash />}>
        <Show when={!sessionEnded()} fallback={<SessionEndedScreen />}>
        <Show when={currentUser()} fallback={<AuthModal />}>
          <div class="app-layout">
            <Sidebar />
            <Show when={activeChannel()} fallback={
              <div class="splash" style={{ background: 'var(--bg-app)' }}>
                <img src="/custom/assets/illustrations/mascot.svg" alt="" width="260" />
                <h2>{t('Welcome to FinchSpeak')}</h2>
                <p class="t2">{t('Pick a channel on the left to start chatting or talking.')}</p>
              </div>
            }>
              <Show when={activeChannel()?.channel_type === 'text'} fallback={<VoicePanel />}>
                <ChatArea />
              </Show>
            </Show>
            <Show when={prefs().appearance.showMembers}><MembersPanel /></Show>
          </div>
        </Show>
        </Show>
      </Show>
      <Overlays />
      <TofuModal />
      <CertChangedModal />
      <AdminModal />
      <SettingsModal />
      <ToastHost />
    </div>
  );
}
