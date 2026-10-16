import { Show } from 'solid-js';
import { isSharing, stopSharing } from '../../stores/screenStore';
import { isDeafened, isMuted, leaveVoice, pushToTalkKey, toggleDeafen, toggleMute, voiceSettings } from '../../stores/voiceStore';
import { Icon, type IconName } from '../../ui';

type CtlProps = { label: string; icon: IconName; class?: string; pressed?: boolean; onClick: () => void };

function Ctl(props: CtlProps) {
  return (
    <button type="button" class={`ctl${props.class ? ` ${props.class}` : ''}`} aria-label={props.label} title={props.label} aria-pressed={props.pressed} onClick={props.onClick}>
      <Icon name={props.icon} size="lg" />
    </button>
  );
}

/** Mute, deafen, share, settings, leave: the bar at the bottom of a voice room. */
export function Dock(props: { onShare: () => void; onSettings: () => void }) {
  const talking = () => voiceSettings().push_to_talk && voiceSettings().key_down;
  return (
    <div class="row center g16" style={{ padding: '0 20px 20px' }}>
      <Show when={voiceSettings().push_to_talk && pushToTalkKey()}>
        <span class="pill" style={talking() ? { 'border-color': 'var(--cyan)', color: 'var(--cyan)' } : undefined} role="status">
          <span class="dot" style={{ background: talking() ? 'var(--cyan)' : 'var(--text-3)' }} />
          {pushToTalkKey()} · {talking() ? 'talking' : 'push to talk'}
        </span>
      </Show>
      <div class="dock" role="toolbar" aria-label="Voice controls">
        <Ctl label={isMuted() ? 'Unmute' : 'Mute'} icon={isMuted() ? 'mic-off' : 'mic'} class={isMuted() ? 'active' : ''} pressed={isMuted()} onClick={() => void toggleMute()} />
        <Ctl label={isDeafened() ? 'Undeafen' : 'Deafen'} icon={isDeafened() ? 'phones-off' : 'phones'} class={isDeafened() ? 'active' : ''} pressed={isDeafened()} onClick={() => void toggleDeafen()} />
        <Ctl label={isSharing() ? 'Stop sharing' : 'Share your screen'} icon="share" class={isSharing() ? 'live' : ''} pressed={isSharing()} onClick={() => (isSharing() ? void stopSharing() : props.onShare())} />
        <Ctl label="Voice settings" icon="sliders" onClick={props.onSettings} />
        <Ctl label="Leave the room" icon="hangup" class="leave" onClick={() => void leaveVoice()} />
      </div>
    </div>
  );
}
