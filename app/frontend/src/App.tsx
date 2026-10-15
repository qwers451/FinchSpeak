import { onMount, onCleanup, Show } from 'solid-js';
import { currentUser, setPendingTofu } from './stores/authStore';
import { activeChannel, loadChannels, handleIncomingMessage, handleChannelCreated } from './stores/chatStore';
import {
  handleUserSpeaking,
  setIsLocalSpeaking,
  setPingRtt,
  setVoiceParticipants,
} from './stores/voiceStore';
import { api } from './services/api';
import { Sidebar } from './components/Sidebar';
import { ChatArea } from './components/ChatArea';
import { VoicePanel } from './components/VoicePanel';
import { AuthModal } from './components/AuthModal';
import { TofuModal } from './components/TofuModal';
import type { UnlistenFn } from '@tauri-apps/api/event';

export function App() {
  const unlisteners: UnlistenFn[] = [];

  onMount(async () => {
    // Check if user is already logged in
    try {
      const user = await api.getCurrentUser();
      if (user) {
        await loadChannels();
      }
    } catch (_) {}

    // Attach event listeners
    try {
      unlisteners.push(await api.onNewMessage(handleIncomingMessage));
      unlisteners.push(
        await api.onUserSpeaking((data) => handleUserSpeaking(data.peer_id, data.is_speaking))
      );
      unlisteners.push(await api.onLocalSpeaking((s) => setIsLocalSpeaking(s)));
      unlisteners.push(await api.onPingRtt((data) => setPingRtt(data.rtt_ms)));
      unlisteners.push(await api.onTofuRequired((data) => setPendingTofu(data)));
      unlisteners.push(await api.onChannelCreated(handleChannelCreated));
      unlisteners.push(
        await api.onUserJoinedVoice((data) => {
          setVoiceParticipants((prev) => {
            if (prev.some((p) => p.peer_id === data.peer_id)) return prev;
            return [...prev, data];
          });
        })
      );
      unlisteners.push(
        await api.onUserLeftVoice((data) => {
          setVoiceParticipants((prev) => prev.filter((p) => p.peer_id !== data.peer_id));
        })
      );
    } catch (e) {
      console.warn('Failed to register some Tauri event listeners (browser dev mode?):', e);
    }
  });

  onCleanup(() => {
    unlisteners.forEach((u) => u());
  });

  return (
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
  );
}
