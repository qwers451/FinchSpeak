import { createEffect, createSignal, For, on, onMount, Show } from 'solid-js';
import {
  availableDevices, changeVoiceSettings, chooseAudioDevice, isMuted, levelFraction, loadAudioDevices, micLevelDb, micActivation,
  micTestPhase, selectedDevices, startMicTest, voiceSettings, type DeviceSelection,
} from '../../stores/voiceStore';
import { HotkeyButton } from './HotkeyButton';
import { resetSeats, seats, setSpatialEnabled, spatialEnabled } from '../../stores/spatialStore';
import type { AudioDevice } from '../../types';
import { Button, Icon, Modal, Slider, Toggle } from '../../ui';
import { t } from '../../i18n';
import { Select } from '../../ui/Select';
import { api } from '../../services/api';

function DevicePicker(props: { kind: keyof DeviceSelection; label: string; devices: AudioDevice[] }) {
  const missing = () => {
    const chosen = selectedDevices()[props.kind];
    return chosen !== null && !props.devices.some((device) => device.name === chosen);
  };
  return (
    <div class="field">
      <span class="field-label">{props.label}</span>
      <Select label={props.label} value={selectedDevices()[props.kind] ?? ''}
        options={[
          { value: '', label: t('System default') },
          ...props.devices.map((device) => ({ value: device.name, label: device.name + (device.is_default ? t(' (default)') : '') })),
          ...(missing() ? [{ value: selectedDevices()[props.kind] ?? '', label: selectedDevices()[props.kind] ?? '', disabled: true }] : []),
        ]}
        onChange={(value) => void chooseAudioDevice(props.kind, value || null)} />
      <Show when={missing()}>
        <span class="err"><Icon name="warn" size="sm" />{t('This device is not connected; the system default is used instead.')}</span>
      </Show>
    </div>
  );
}

/** D3: devices, microphone level with the speech threshold, keys, hear myself, test. */
function VoiceSettingsBody() {
  const [diagnosticsBusy, setDiagnosticsBusy] = createSignal(false);
  const [diagnosticsAction, setDiagnosticsAction] = createSignal<'start' | 'stop' | 'save' | null>(null);
  const [diagnosticsStatus, setDiagnosticsStatus] = createSignal('');
  const [diagnosticsError, setDiagnosticsError] = createSignal('');
  const [diagnosticCapture, setDiagnosticCapture] = createSignal(false);

  const startDiagnostics = async () => {
    setDiagnosticsBusy(true);
    setDiagnosticsAction('start');
    setDiagnosticsError('');
    setDiagnosticsStatus('');
    try {
      await api.startAudioDiagnostics();
      // The backend starts a fresh diagnostic capture on every call.
      setDiagnosticsStatus(t('Capturing audio diagnostics…'));
      setDiagnosticCapture(true);
    } catch (error) {
      setDiagnosticsError(error instanceof Error ? error.message : String(error));
    } finally {
      setDiagnosticsBusy(false);
      setDiagnosticsAction(null);
    }
  };

  const stopDiagnostics = async () => {
    setDiagnosticsBusy(true);
    setDiagnosticsAction('stop');
    setDiagnosticsError('');
    setDiagnosticsStatus('');
    try {
      await api.stopAudioDiagnostics();
      setDiagnosticCapture(false);
      setDiagnosticsStatus(t('Capture stopped. You can save the report.'));
    } catch (error) {
      setDiagnosticsError(error instanceof Error ? error.message : String(error));
    } finally {
      setDiagnosticsBusy(false);
      setDiagnosticsAction(null);
    }
  };

  const saveDiagnostics = async () => {
    setDiagnosticsBusy(true);
    setDiagnosticsAction('save');
    setDiagnosticsError('');
    setDiagnosticsStatus('');
    try {
      const report = await api.getAudioDiagnostics();
      const url = URL.createObjectURL(new Blob([report], { type: 'application/json' }));
      const link = document.createElement('a');
      link.href = url;
      link.download = 'finchspeak-audio-diagnostics.json';
      document.body.append(link);
      try {
        link.click();
      } finally {
        link.remove();
        // Let the browser hand the blob to its download manager before releasing it.
        window.setTimeout(() => URL.revokeObjectURL(url), 1000);
      }
      setDiagnosticsStatus(t('Audio diagnostics report saved.'));
    } catch (error) {
      setDiagnosticsError(error instanceof Error ? error.message : String(error));
    } finally {
      setDiagnosticsBusy(false);
      setDiagnosticsAction(null);
    }
  };

  const meterWidth = () => `${Math.round(levelFraction(isMuted() ? -120 : micLevelDb()) * 100)}%`;
  const thresholdLeft = () => `${Math.round(levelFraction(voiceSettings().vad_threshold_db) * 100)}%`;

  return (
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
        <HotkeyButton action="push_to_talk" />

        <div class="setrow">
          <div class="col grow"><strong class="fs13">{t('Mute microphone')}</strong><span class="t3 fs12">{t('Pressed to mute or unmute; works in the background.')}</span></div>
          <HotkeyButton action="toggle_mute" />
        </div>
        <div class="setrow">
          <div class="col grow"><strong class="fs13">{t('Turn sound off')}</strong><span class="t3 fs12">{t('Pressed to turn all sound off or on; works in the background.')}</span></div>
          <HotkeyButton action="toggle_deafen" />
        </div>

        <div class="setrow">
          <div class="col grow"><strong class="fs13">{t('Spatial audio')}</strong><span class="t3 fs12">{t('Everyone in the room is heard from their own side. Best with headphones; drag people on the stage in the room to move them.')}</span></div>
          <Toggle label={t('Spatial audio')} checked={spatialEnabled()} onChange={setSpatialEnabled} />
        </div>
        <Show when={spatialEnabled() && Object.keys(seats()).length > 0}>
          <div class="row"><Button size="sm" variant="ghost" icon="refresh" onClick={() => resetSeats()}>{t('Spread everyone out again')}</Button></div>
        </Show>

        <div class="setrow">
          <div class="col grow"><strong class="fs13">{t('Hear myself')}</strong><span class="t3 fs12">{t('Plays processed microphone audio continuously, except during a microphone test.')}</span></div>
          <Toggle label={t('Hear myself')} checked={voiceSettings().monitor} onChange={(checked) => void changeVoiceSettings({ monitor: checked })} />
        </div>
        <Show when={voiceSettings().monitor}>
          <div class="auth-banner amber" role="note"><Icon name="warn" /><span class="fs13">{t('Use headphones, or the speakers will feed back into the microphone.')}</span></div>
        </Show>

        <div class="col g8" aria-label={t('Audio diagnostics')}>
          <div class="col g4">
            <strong class="fs13">{t('Audio diagnostics')}</strong>
            <span class="t3 fs12">{t('Collects device names, audio configuration, and technical counters. No audio is recorded.')}</span>
          </div>
          <div class="row g8">
            <Button variant="secondary" icon="play" aria-label={t('Start or reset audio diagnostics')} disabled={diagnosticsBusy()} loading={diagnosticsAction() === 'start'} onClick={() => void startDiagnostics()}>
              {t('Start / reset capture')}
            </Button>
            <Show when={diagnosticCapture()}>
              <Button variant="secondary" icon="x" aria-label={t('Stop audio diagnostics')} disabled={diagnosticsBusy()} loading={diagnosticsAction() === 'stop'} onClick={() => void stopDiagnostics()}>
                {t('Stop capture')}
              </Button>
            </Show>
            <Button variant="secondary" icon="dl" aria-label={t('Save audio diagnostics report')} disabled={diagnosticsBusy() || diagnosticCapture()} loading={diagnosticsAction() === 'save'} onClick={() => void saveDiagnostics()}>
              {t('Save report')}
            </Button>
          </div>
          <Show when={diagnosticsStatus()}><span class="t3 fs12" role="status">{diagnosticsStatus()}</span></Show>
          <Show when={diagnosticsError()}><span class="err" role="alert"><Icon name="warn" size="sm" />{diagnosticsError()}</span></Show>
        </div>

      </div>
  );
}

/** Voice and audio as a page inside the settings window. */
export function VoiceSettingsPage() {
  onMount(() => void loadAudioDevices());
  return <VoiceSettingsBody />;
}

/** Voice and audio as its own dialog, opened from the voice room. */
export function VoiceSettingsModal(props: { open: boolean; onClose: () => void }) {
  createEffect(on(() => props.open, (open) => { if (open) void loadAudioDevices(); }));
  return (
    <Modal open={props.open} onClose={props.onClose} title={t('Voice and audio')} width="560px">
      <div class="col g20">
        <VoiceSettingsBody />
        <div class="row" style={{ 'justify-content': 'flex-end' }}><Button variant="primary" onClick={props.onClose}>{t('Done')}</Button></div>
      </div>
    </Modal>
  );
}
