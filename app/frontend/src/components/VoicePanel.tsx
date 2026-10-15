import { For, Show } from 'solid-js';
import {
  activeVoiceChannel,
  voiceParticipants,
  speakingPeers,
  isLocalSpeaking,
} from '../stores/voiceStore';
import { currentUser } from '../stores/authStore';
import { api } from '../services/api';
import { localPeerId } from '../stores/sessionStore';
import { DeviceSettings } from './DeviceSettings';

export function VoicePanel() {
  const handleVolumeChange = (peerId: number, vol: number) => {
    api.setSpeakerVolume(peerId, vol);
  };

  return (
    <div style={{ flex: 1, display: "flex", "flex-direction": "column", height: "100%", background: "var(--bg-main)" }}>
      {/* Header */}
      <div class="chat-header">
        <div class="header-channel-name">
          <span>🔊</span>
          <span>{activeVoiceChannel()?.name || 'Voice Room'}</span>
        </div>
      </div>

      <DeviceSettings />

      {/* Participant Grid */}
      <div class="voice-grid">
        {/* Local user card */}
        <div class={`participant-card ${isLocalSpeaking() ? 'speaking' : ''}`}>
          <div class="participant-avatar">
            {currentUser()?.display_name?.charAt(0).toUpperCase() || 'U'}
          </div>
          <div class="participant-name">
            {currentUser()?.display_name || 'You'} (You)
          </div>
        </div>

        {/* Remote participants */}
        <For each={voiceParticipants().filter((participant) => participant.peer_id !== localPeerId())}>
          {(participant) => {
            const isSpeaking = () => speakingPeers().has(participant.peer_id);
            return (
              <div class={`participant-card ${isSpeaking() ? 'speaking' : ''}`}>
                <div class="participant-avatar">
                  {participant.username.charAt(0).toUpperCase()}
                </div>
                <div class="participant-name">{participant.username}</div>
                <Show when={participant.is_muted}><span title="Muted">🔇</span></Show>
                <Show when={participant.is_deafened}><span title="Deafened">🎧</span></Show>

                {/* Per-user volume slider */}
                <div style={{ display: "flex", "align-items": "center", gap: "8px", width: "100%", "margin-top": "6px" }}>
                  <span style={{ "font-size": "0.75rem", color: "var(--text-dim)" }}>🔈</span>
                  <input
                    type="range"
                    min="0"
                    max="2"
                    step="0.05"
                    value="1.0"
                    style={{ flex: 1, "accent-color": "var(--accent-teal)" }}
                    onChange={(e) => handleVolumeChange(participant.peer_id, parseFloat(e.currentTarget.value))}
                  />
                  <span style={{ "font-size": "0.75rem", color: "var(--text-dim)" }}>🔊</span>
                </div>
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
}
