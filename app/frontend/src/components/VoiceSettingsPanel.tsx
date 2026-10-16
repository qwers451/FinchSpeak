import { Show, createSignal } from 'solid-js';
import { changeVoiceSettings, choosePushToTalkKey, pushToTalkKey, shortcutFromEvent, voiceSettings } from '../stores/voiceStore';

/** Microphone volume, speech threshold, push-to-talk and "hear myself". */
export function VoiceSettingsPanel() {
  const [capturing, setCapturing] = createSignal(false);

  const captureKey = (event: KeyboardEvent) => {
    event.preventDefault();
    if (event.code === 'Escape') {
      setCapturing(false);
      return;
    }
    const shortcut = shortcutFromEvent(event);
    if (!shortcut) return; // a modifier on its own: wait for the real key
    setCapturing(false);
    void choosePushToTalkKey(shortcut);
  };

  return (
    <div class="voice-settings">
      <label class="device-field">
        <span>Microphone volume ({Math.round(voiceSettings().input_gain * 100)}%)</span>
        <input
          type="range" min="0" max="3" step="0.05"
          value={voiceSettings().input_gain}
          onChange={(e) => void changeVoiceSettings({ input_gain: parseFloat(e.currentTarget.value) })}
        />
      </label>
      <label class="device-field">
        <span>Speech threshold ({Math.round(voiceSettings().vad_threshold_db)} dB; lower picks up quieter speech)</span>
        <input
          type="range" min="-70" max="-15" step="1"
          value={voiceSettings().vad_threshold_db}
          onChange={(e) => void changeVoiceSettings({ vad_threshold_db: parseFloat(e.currentTarget.value) })}
        />
      </label>
      <div class="device-field ptt-row">
        <label>
          <input
            type="checkbox"
            checked={voiceSettings().push_to_talk}
            onChange={(e) => void changeVoiceSettings({ push_to_talk: e.currentTarget.checked })}
          />
          Push to talk
        </label>
        <button
          type="button"
          class={capturing() ? 'danger' : ''}
          onClick={() => setCapturing(true)}
          onKeyDown={(e) => capturing() && captureKey(e)}
          onBlur={() => setCapturing(false)}
        >
          {capturing() ? 'Press a key… (Esc cancels)' : pushToTalkKey() ?? 'Choose key'}
        </button>
        <Show when={pushToTalkKey()}>
          <button type="button" title="Remove the key" onClick={() => void choosePushToTalkKey(null)}>✕</button>
        </Show>
        <Show when={voiceSettings().push_to_talk && voiceSettings().key_down}><span class="ptt-live">● talking</span></Show>
      </div>
      <label class="device-field">
        <span>
          <input
            type="checkbox"
            checked={voiceSettings().monitor}
            onChange={(e) => void changeVoiceSettings({ monitor: e.currentTarget.checked })}
          />
          Hear myself (use headphones, or you will get feedback)
        </span>
      </label>
    </div>
  );
}
