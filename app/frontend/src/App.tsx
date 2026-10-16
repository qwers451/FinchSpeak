import { createSignal, onMount, onCleanup, Show } from 'solid-js';
import { currentUser, setCurrentUser } from './stores/authStore';
import { activeChannel, loadChannels } from './stores/chatStore';
import { setAppError } from './stores/sessionStore';
import { subscribeSessionEvents } from './services/sessionEvents';
import { api } from './services/api';
import { Sidebar } from './components/Sidebar';
import { prefs } from './stores/settingsStore';
import { MembersPanel } from './components/MembersPanel';
import { ChatArea } from './components/ChatArea';
import { VoicePanel } from './components/VoicePanel';
import { AuthModal } from './components/AuthModal';
import { CertChangedModal, TofuModal } from './components/TofuModal';
import { ConnectionBanner, Splash } from './components/ConnectionBanner';
import { IconSprite, Titlebar, ToastHost } from './ui';
import { AdminModal } from './components/admin/AdminModal';
import { SettingsModal } from './components/settings/SettingsModal';
import type { UnlistenFn } from '@tauri-apps/api/event';

export function App() {
  let unlisten: UnlistenFn | undefined;
  let disposed = false;
  const [eventsReady, setEventsReady] = createSignal(false);

  onMount(async () => {
    try {
      const subscriptions = await subscribeSessionEvents();
      if (disposed) { subscriptions.dispose(); return; }
      unlisten = subscriptions.dispose;
      const user = await api.getCurrentUser();
      if (disposed) return;
      if (user) {
        setCurrentUser(user);
        await loadChannels();
        await subscriptions.refresh();
      }
      setEventsReady(true);
    } catch (e) {
      if (!disposed) setAppError(String(e));
    } finally {
      if (!disposed && unlisten) setEventsReady(true);
    }
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
      <Show when={eventsReady()} fallback={<Splash />}>
        <Show when={currentUser()} fallback={<AuthModal />}>
          <div class="app-layout">
            <Sidebar />
            <Show when={activeChannel()} fallback={
              <div class="splash" style={{ background: 'var(--bg-app)' }}>
                <img src="./mascot.svg" alt="" width="260" />
                <h2>Welcome to FinchSpeak</h2>
                <p class="t2">Pick a channel on the left to start chatting or talking.</p>
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
      <TofuModal />
      <CertChangedModal />
      <AdminModal />
      <SettingsModal />
      <ToastHost />
    </div>
  );
}
