import { createSignal, Show } from 'solid-js';
import { chooseHotkey, hotkeyFor, shortcutFromEvent, type HotkeyAction } from '../../stores/voiceStore';
import { Button, Icon } from '../../ui';
import { t } from '../../i18n';

const LABELS: Record<HotkeyAction, string> = {
  push_to_talk: 'Push-to-talk key',
  toggle_mute: 'Mute microphone key',
  toggle_deafen: 'Turn sound off key',
  overlay: 'Mini window key',
};

/** Click, then press the key to use for this action; Esc cancels, × removes the key. */
export function HotkeyButton(props: { action: HotkeyAction }) {
  const [capturing, setCapturing] = createSignal(false);
  const capture = (event: KeyboardEvent) => {
    event.preventDefault();
    event.stopPropagation(); // while choosing a key, Esc must not reach the dialog and close it
    if (event.code === 'Escape') {
      setCapturing(false);
      return;
    }
    const shortcut = shortcutFromEvent(event);
    if (!shortcut) return; // a modifier on its own: wait for the real key
    setCapturing(false);
    void chooseHotkey(props.action, shortcut);
  };
  return (
    <div class="row g8">
      <button
        type="button"
        class={`btn secondary${capturing() ? ' is-focus' : ''}`}
        aria-label={t(LABELS[props.action])}
        onClick={() => setCapturing(true)}
        onKeyDown={(event) => capturing() && capture(event)}
        onBlur={() => setCapturing(false)}
      >
        <Icon name="key" size="sm" />{capturing() ? t('Press a key… (Esc cancels)') : hotkeyFor(props.action) ?? t('Choose key')}
      </button>
      <Show when={hotkeyFor(props.action)}>
        <Button iconOnly size="sm" variant="ghost" icon="x" label={t('Remove the key')} onClick={() => void chooseHotkey(props.action, null)} />
      </Show>
    </div>
  );
}
