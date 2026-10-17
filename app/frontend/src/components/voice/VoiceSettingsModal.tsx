import { createEffect, createSignal, For, on, Show } from 'solid-js';
import {
  availableDevices, changeVoiceSettings, chooseAudioDevice, choosePushToTalkKey, isMuted, levelFraction, loadAudioDevices, micLevelDb, micActivation,
  micTestPhase, pushToTalkKey, selectedDevices, shortcutFromEvent, startMicTest, voiceSettings, type DeviceSelection,
} from '../../stores/voiceStore';
import type { AudioDevice } from '../../types';
import { Button, Icon, Modal, Slider, Toggle } from '../../ui';
import { t } from '../../i18n';

function DevicePicker(props: { kind: keyof DeviceSelection; label: string; devices: AudioDevice[] }) {
  const missing = () => {
    const chosen = selectedDevices()[props.kind];
    return chosen !== null && !props.devices.some((device) => device.name === chosen);
  };
  return (
    <div class="field">
      <span class="field-label">{props.label}</span>
      <div class="input">
        <Icon name={props.kind === 'input' ? 'mic' : 'vol'} size="sm" class="t3" />
        <select aria-label={props.label} onChange={(event) => void chooseAudioDevice(props.kind, event.currentTarget.value || null)}>
          <option value="" selected={selectedDevices()[props.kind] === null}>{t('System default')}</option>
          <For each={props.devices}>
            {(device) => <option value={device.name} selected={selectedDevices()[props.kind] === device.name}>{device.name}{device.is_default ? t(' (default)') : ''}</option>}
          </For>
        </select>
        <Icon name="chev-d" size="sm" class="t3" />
      </div>
      <Show when={missing()}>
        <span class="err"><Icon name="warn" size="sm" />{t('This device is not connected; the system default is used instead.')}</span>
      </Show>
    </div>
  );
}

/** D3: devices, microphone level with the speech threshold, push-to-talk, hear myself, test. */
export function VoiceSettingsModal(props: { open: boolean; onClose: () => void }) {
  const [capturing, setCapturing] = createSignal(false);
  createEffect(on(() => props.open, (open) => { if (open) void loadAudioDevices(); }));

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

  const meterWidth = () => `${Math.round(levelFraction(isMuted() ? -120 : micLevelDb()) * 100)}%`;
  const thresholdLeft = () => `${Math.round(levelFraction(voiceSettings().vad_threshold_db) * 100)}%`;

  return (
    <Modal open={props.open} onClose={props.onClose} title={t('Voice and audio')} width="560px">
      <div class="col g20">
        <DevicePicker kind="input" label={t('Microphone')} devices={availableDevices().inputs} />
        <DevicePicker kind="output" label={t('Speakers')} devices={availableDevices().outputs} />

        <div class="setrow">
          <div class="col grow"><strong class="fs13">{t('Voice enhancement')}</strong><span class="t3 fs12">{t('Balances quiet and loud speech and limits peaks.')}</span></div>
          <Toggle label={t('Voice enhancement')} checked={voiceSettings().voice_enhancement} onChange={(checked) => void changeVoiceSettings({ voice_enhancement: checked })} />
        </div>

        <div class="col g8">
          <div class="row between"><span class="field-label">{t('Input volume')}</span><span class="fs13 t2">{Math.round(voiceSettings().input_gain * 100)}%</span></div>
          <Slider label={t('Input volume')} min={0} max={3} step={0.05} value={voiceSettings().input_gain} onChange={(value) => void changeVoiceSettings({ input_gain: value })} />
        </div>

        <div class="col g8">
          <div class="row between"><span class="field-label">{t('Speech threshold for sending')}</span><span class="fs13 t2">{Math.round(voiceSettings().vad_threshold_db)} dB</span></div>
          <div class="meter" role="meter" aria-label={t('Microphone level')} aria-valuemin={-70} aria-valuemax={0} aria-valuenow={Math.round(micLevelDb())}>
            <i style={{ width: meterWidth(), transition: 'width 60ms linear' }} />
            <span class="thr" style={{ left: thresholdLeft() }} />
          </div>
          <Slider label={t('Speech threshold for sending')} min={-70} max={-15} step={1} value={voiceSettings().vad_threshold_db} onChange={(value) => void changeVoiceSettings({ vad_threshold_db: value })} />
          <span class="t3 fs12">
            <Show when={isMuted()} fallback={micActivation() ? t('Voice activation: open') : t('Voice activation: closed')}>
              {t('You are muted: the meter is off.')}
            </Show>
          </span>
        </div>

        <span class="t3 fs12">{t('The bar shows measured level. Activation also checks detected speech and push-to-talk; crossing the line alone does not open the microphone.')}</span>

        <div class="row g12">
          <Button variant="secondary" icon="play" disabled={micTestPhase() !== 'idle'} onClick={() => void startMicTest(3)}>
            {micTestPhase() === 'recording' ? t('Recording 3 s…') : micTestPhase() === 'playing' ? t('Playing back…') : t('Test microphone')}
          </Button>
          <span class="t3 fs12 grow">{t('Records three seconds with voice activation, then plays them back locally. Nobody else hears it.')}</span>
        </div>

        <div class="setrow">
          <div class="col grow"><strong class="fs13">{t('Push to talk')}</strong><span class="t3 fs12">{t('The microphone is open only while the key is held; it works while FinchSpeak is in the background.')}</span></div>
          <Toggle label={t('Push to talk')} checked={voiceSettings().push_to_talk} onChange={(checked) => void changeVoiceSettings({ push_to_talk: checked })} />
        </div>
        <div class="row g8">
          <button
            type="button"
            class={`btn secondary${capturing() ? ' is-focus' : ''}`}
            aria-label={t('Push-to-talk key')}
            onClick={() => setCapturing(true)}
            onKeyDown={(event) => capturing() && captureKey(event)}
            onBlur={() => setCapturing(false)}
          >
            <Icon name="key" size="sm" />{capturing() ? t('Press a key… (Esc cancels)') : pushToTalkKey() ?? t('Choose key')}
          </button>
          <Show when={pushToTalkKey()}>
            <Button iconOnly size="sm" variant="ghost" icon="x" label={t('Remove the key')} onClick={() => void choosePushToTalkKey(null)} />
          </Show>
        </div>

        <div class="setrow">
          <div class="col grow"><strong class="fs13">{t('Hear myself')}</strong><span class="t3 fs12">{t('Plays processed microphone audio continuously, except during a microphone test.')}</span></div>
          <Toggle label={t('Hear myself')} checked={voiceSettings().monitor} onChange={(checked) => void changeVoiceSettings({ monitor: checked })} />
        </div>
        <Show when={voiceSettings().monitor}>
          <div class="auth-banner amber" role="note"><Icon name="warn" /><span class="fs13">{t('Use headphones, or the speakers will feed back into the microphone.')}</span></div>
        </Show>

        <div class="row" style={{ 'justify-content': 'flex-end' }}><Button variant="primary" onClick={props.onClose}>{t('Done')}</Button></div>
      </div>
    </Modal>
  );
}
