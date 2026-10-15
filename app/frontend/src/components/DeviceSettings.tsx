import { For, onMount } from 'solid-js';
import type { AudioDevice } from '../types';
import {
  availableDevices,
  chooseAudioDevice,
  loadAudioDevices,
  selectedDevices,
  type DeviceSelection,
} from '../stores/voiceStore';

/** Microphone and speaker pickers. A change is applied at once, also during a call. */
export function DeviceSettings() {
  onMount(() => {
    void loadAudioDevices();
  });

  const picker = (kind: keyof DeviceSelection, label: string, devices: () => AudioDevice[]) => (
    <label class="device-field">
      <span>{label}</span>
      <select onChange={(event) => void chooseAudioDevice(kind, event.currentTarget.value || null)}>
        <option value="" selected={selectedDevices()[kind] === null}>System default</option>
        <For each={devices()}>
          {(device) => (
            <option value={device.name} selected={selectedDevices()[kind] === device.name}>
              {device.name}{device.is_default ? ' (default)' : ''}
            </option>
          )}
        </For>
      </select>
    </label>
  );

  return (
    <div class="device-settings">
      {picker('input', 'Microphone', () => availableDevices().inputs)}
      {picker('output', 'Speakers', () => availableDevices().outputs)}
    </div>
  );
}
