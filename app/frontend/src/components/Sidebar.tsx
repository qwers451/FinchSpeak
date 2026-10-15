import { For, Show } from 'solid-js';
import {
  channels,
  activeChannel,
  selectChannel,
} from '../stores/chatStore';
import {
  activeVoiceChannel,
  joinVoice,
  leaveVoice,
  isMuted,
  isDeafened,
  toggleMute,
  toggleDeafen,
  isLocalSpeaking,
  pingRtt,
} from '../stores/voiceStore';
import { currentUser, logoutUser } from '../stores/authStore';
import type { Channel } from '../types';

export function Sidebar() {
  const textChannels = () => channels().filter((c) => c.channel_type === 'text');
  const voiceChannels = () => channels().filter((c) => c.channel_type === 'voice');

  return (
    <aside class="sidebar">
      {/* Brand Header */}
      <div class="brand-header">
        <img src="/logo.png" alt="FinchSpeak" class="brand-logo" />
        <span class="brand-title">FinchSpeak</span>
      </div>

      {/* Channel Navigation */}
      <div class="channel-list">
        {/* Text Channels */}
        <div class="channel-category" style={{ display: "flex", "justify-content": "space-between", "align-items": "center" }}>
          <span>Text Channels</span>
          <button
            class="icon-btn"
            style={{ "font-size": "0.9rem", padding: "2px 6px", cursor: "pointer" }}
            title="Create Text Channel"
            onClick={async () => {
              const name = prompt('Enter new channel name:');
              if (name && name.trim()) {
                await api.createChannel(name.trim(), 'text');
                await loadChannels();
              }
            }}
          >
            +
          </button>
        </div>
        <For each={textChannels()}>
          {(channel) => (
            <div
              class={`channel-item ${activeChannel()?.id === channel.id ? 'active' : ''}`}
              style={{ cursor: "pointer" }}
              onClick={() => selectChannel(channel)}
            >
              <span style={{ "font-size": "1.1rem", color: "var(--text-dim)" }}>#</span>
              <span>{channel.name}</span>
            </div>
          )}
        </For>

        {/* Voice Channels */}
        <div class="channel-category" style={{ display: "flex", "justify-content": "space-between", "align-items": "center", "margin-top": "14px" }}>
          <span>Voice Channels</span>
          <button
            class="icon-btn"
            style={{ "font-size": "0.9rem", padding: "2px 6px", cursor: "pointer" }}
            title="Create Voice Channel"
            onClick={async () => {
              const name = prompt('Enter new voice channel name:');
              if (name && name.trim()) {
                await api.createChannel(name.trim(), 'voice');
                await loadChannels();
              }
            }}
          >
            +
          </button>
        </div>
        <For each={voiceChannels()}>
          {(channel) => (
            <div
              class={`channel-item ${activeVoiceChannel()?.id === channel.id ? 'active' : ''}`}
              style={{ cursor: "pointer" }}
              onClick={() => {
                selectChannel(channel);
                if (activeVoiceChannel()?.id !== channel.id) {
                  joinVoice(channel);
                }
              }}
            >
              <span style={{ "font-size": "1.1rem" }}>🔊</span>
              <span>{channel.name}</span>
            </div>
          )}
        </For>
      </div>

      {/* Active Voice Connection Status Banner */}
      <Show when={activeVoiceChannel()}>
        {(channel) => (
          <div class="voice-banner">
            <div class="voice-status">
              <span style={{ "font-size": "0.95rem" }}>● Connected</span>
              <span style={{ color: "var(--text-main)", "font-weight": "500" }}>{channel().name}</span>
              <Show when={pingRtt() !== null}>
                <span class="ping-indicator">{pingRtt()}ms</span>
              </Show>
            </div>
            <button
              class="icon-btn active-danger"
              title="Disconnect"
              onClick={() => leaveVoice()}
            >
              ✕
            </button>
          </div>
        )}
      </Show>

      {/* Current User Bar */}
      <div class="user-panel">
        <div class="user-profile">
          <div class={`avatar ${isLocalSpeaking() ? 'speaking' : ''}`}>
            {currentUser()?.display_name?.charAt(0).toUpperCase() || 'U'}
          </div>
          <div class="user-info">
            <span class="user-name">{currentUser()?.display_name || 'User'}</span>
            <span class="user-status">● Online</span>
          </div>
        </div>

        <div class="user-controls">
          <button
            class={`icon-btn ${isMuted() ? 'active-danger' : ''}`}
            title={isMuted() ? 'Unmute' : 'Mute'}
            onClick={() => toggleMute()}
          >
            {isMuted() ? '🔇' : '🎙️'}
          </button>
          <button
            class={`icon-btn ${isDeafened() ? 'active-danger' : ''}`}
            title={isDeafened() ? 'Undeafen' : 'Deafen'}
            onClick={() => toggleDeafen()}
          >
            {isDeafened() ? '🚫' : '🎧'}
          </button>
          <button
            class="icon-btn"
            title="Log out"
            onClick={() => logoutUser()}
          >
            ⏻
          </button>
        </div>
      </div>
    </aside>
  );
}
