import { createEffect, createSignal, For, on, Show } from 'solid-js';
import { describeError } from '../../i18n/errors';
import { boardSounds, canRemove, loadBoard, playBoardSound, removeBoardSound, setSoundboardHidden, setSoundboardOpen, soundboardHidden, soundboardOpen, uploadBoardSound } from '../../stores/soundboardStore';
import { setAppError } from '../../stores/notificationStore';
import { setVolume, SOUNDBOARD_VOLUME_KEY, volumeFor } from '../../stores/volumeStore';
import type { SoundInfo } from '../../types';
import { Button, Icon, Modal, RoomIcon, Slider, Toggle } from '../../ui';
import { rippleFromEvent } from '../../ui/motion';
import { t } from '../../i18n';
import { EmojiPicker } from '../chat/EmojiPicker';

const NAME_MAX = 32;

/** The soundboard of the voice room: a grid of sounds to play, a search, and adding or removing sounds. */
export function SoundboardPanel() {
  const [query, setQuery] = createSignal('');
  const [name, setName] = createSignal('');
  const [emoji, setEmoji] = createSignal('');
  const [picker, setPicker] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  let fileInput!: HTMLInputElement;

  // The board is fetched each time the panel opens, so other people's uploads show up.
  createEffect(on(soundboardOpen, (open) => {
    if (open) void loadBoard();
  }));

  const shown = (): SoundInfo[] => {
    const text = query().trim().toLowerCase();
    return text ? boardSounds().filter((sound) => sound.name.toLowerCase().includes(text)) : boardSounds();
  };

  async function onFile(file: File | undefined) {
    if (!file) return;
    const trimmed = name().trim();
    if (!trimmed) {
      setAppError(t('Name the sound first'));
      return;
    }
    setBusy(true);
    try {
      await uploadBoardSound(file, trimmed, emoji());
      setName('');
      setEmoji('');
    } catch (err) {
      setAppError(describeError(err));
    } finally {
      setBusy(false);
    }
  }

  async function remove(sound: SoundInfo) {
    try {
      await removeBoardSound(sound.id);
    } catch (err) {
      setAppError(describeError(err));
    }
  }

  return (
    <Modal open={soundboardOpen()} onClose={() => setSoundboardOpen(false)} title={t('Soundboard')} width="420px">
      <div class="soundboard-panel">
        <input
          type="search"
          class="input"
          placeholder={t('Search sounds')}
          aria-label={t('Search sounds')}
          value={query()}
          onInput={(event) => setQuery(event.currentTarget.value)}
        />
        <div class="soundboard-grid">
          <For each={shown()} fallback={<p class="t3 fs12">{boardSounds().length ? t('Nothing found') : t('No sounds yet')}</p>}>
            {(sound) => (
              <div class="soundboard-tile">
                <button type="button" class="soundboard-play" title={sound.name} onClick={(event) => { rippleFromEvent(event); void playBoardSound(sound.id); }}>
                  <Show when={sound.emoji?.trim()} fallback={<RoomIcon name="soundboard" size="sm" class="t3" />}><span aria-hidden="true" class="soundboard-emoji">{sound.emoji}</span></Show>
                  <span class="trunc">{sound.name}</span>
                </button>
                <Show when={canRemove(sound)}>
                  <Button variant="ghost" size="sm" iconOnly icon="trash" label={t('Remove sound')} onClick={() => void remove(sound)} />
                </Show>
              </div>
            )}
          </For>
        </div>
        <div class="soundboard-add">
          <input type="text" class="input" maxLength={NAME_MAX} placeholder={t('Sound name')} aria-label={t('Sound name')} value={name()} onInput={(event) => setName(event.currentTarget.value)} />
          <div class="soundboard-emoji-field">
            <Button variant="secondary" class="soundboard-emoji-pick" aria-haspopup="dialog" aria-expanded={picker()} label={emoji() ? t('Emoji: {emoji}', { emoji: emoji() }) : t('Choose an emoji')} icon={emoji() ? undefined : 'smile'} onClick={() => setPicker(!picker())}>
              <Show when={emoji()} fallback={<span class="t3 trunc">{t('Emoji')}</span>}><span class="soundboard-emoji">{emoji()}</span></Show>
            </Button>
            <Show when={emoji()}>
              <Button iconOnly size="sm" variant="ghost" icon="x" class="soundboard-emoji-clear" label={t('Remove the emoji')} onClick={() => setEmoji('')} />
            </Show>
            <Show when={picker()}>
              <div class="soundboard-emoji-popover">
                <EmojiPicker showQuick={false} onClose={() => setPicker(false)} onPick={(char) => { setEmoji(char); setPicker(false); }} />
              </div>
            </Show>
          </div>
          <Button variant="secondary" icon="plus" loading={busy()} disabled={busy()} onClick={() => fileInput.click()}>{t('Add a sound')}</Button>
          <span class="t3 fs12 soundboard-hint">{t('Up to 5 seconds')}</span>
          <input ref={fileInput} type="file" accept="audio/*" hidden onChange={(event) => void onFile(event.currentTarget.files?.[0])} />
        </div>
        <div class="col g6 soundboard-volume">
          <span class="fs13">{t('Soundboard volume')}</span>
          <Slider value={Math.round(volumeFor(SOUNDBOARD_VOLUME_KEY) * 100)} min={0} max={200} cool label={t('Soundboard volume')} onChange={(value) => void setVolume(SOUNDBOARD_VOLUME_KEY, undefined, value / 100)} />
        </div>
        <div class="row g8 between fs13">
          <span>{t("Don't play the soundboard for me")}</span>
          <Toggle checked={soundboardHidden()} label={t("Don't play the soundboard for me")} onChange={(next) => setSoundboardHidden(next)} />
        </div>
      </div>
    </Modal>
  );
}
