import { createSignal, onMount, onCleanup, Show } from 'solid-js';
import { currentUser, setCurrentUser } from './stores/authStore';
import { activeChannel, loadChannels } from './stores/chatStore';
import { appError, setAppError, connectionStatus, connectionError } from './stores/sessionStore';
import { subscribeSessionEvents } from './services/sessionEvents';
import { api } from './services/api';
import { Sidebar } from './components/Sidebar';
import { ChatArea } from './components/ChatArea';
import { VoicePanel } from './components/VoicePanel';
import { AuthModal } from './components/AuthModal';
import { TofuModal } from './components/TofuModal';
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
    <>
    <Show when={appError()}>
      <div class="status-banner error-banner" role="alert">
        <span>{appError()}</span>
        <button class="icon-btn" aria-label="Dismiss error" onClick={() => setAppError(null)}>✕</button>
      </div>
    </Show>
    <Show when={currentUser() && connectionStatus() !== 'connected'}>
      <div class="status-banner" role="status">
        {connectionStatus() === 'reconnecting' ? 'Переподключение…' : 'Нет соединения'}
        <Show when={connectionError()}><span> — {connectionError()}</span></Show>
      </div>
    </Show>
    <Show when={eventsReady()} fallback={<div role="status">Подключение интерфейса…</div>}>
    <div class="app-layout">
      <Show when={currentUser()} fallback={<AuthModal />}>
        <Sidebar />
        <Show when={activeChannel()} fallback={
          <div style={{ flex: 1, display: "flex", "align-items": "center", "justify-content": "center", "flex-direction": "column", gap: "16px", background: "var(--bg-main)", color: "var(--text-muted)" }}>
            <div style={{ "font-size": "3rem" }}>💬</div>
            <h2 style={{ color: "var(--text-main)", "font-weight": "600" }}>Welcome to FinchSpeak</h2>
            <p style={{ "font-size": "0.95rem" }}>Select a channel in the sidebar to start chatting or talking.</p>
          </div>
        }>
          <Show when={activeChannel()?.channel_type === 'text'} fallback={<VoicePanel />}>
            <ChatArea />
          </Show>
        </Show>
      </Show>

      {/* Trust On First Use Certificate Modal */}
      <TofuModal />
    </div>
    </Show>
    </>
  );
}
