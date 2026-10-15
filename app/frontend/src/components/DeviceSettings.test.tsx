import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { render } from 'solid-js/web';
import { api } from '../services/api';
import { DeviceSettings } from './DeviceSettings';
import { selectedDevices, setSelectedDevices } from '../stores/voiceStore';

let dispose: (() => void) | undefined;

beforeEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  setSelectedDevices({ input: null, output: null });
  document.body.innerHTML = '<div id="root"></div>';
  vi.spyOn(api, 'getAudioDevices').mockResolvedValue({
    inputs: [{ name: 'Built-in mic', is_default: true }, { name: 'Headset mic', is_default: false }],
    outputs: [{ name: 'Speakers', is_default: true }, { name: 'Headset', is_default: false }],
    selected_input: null,
    selected_output: 'Headset',
  });
});

afterEach(() => dispose?.());

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

it('AUD-024 lists every device and marks the one in use', async () => {
  dispose = render(() => <DeviceSettings />, document.getElementById('root')!);
  await flush();

  const [input, output] = Array.from(document.querySelectorAll('select')) as HTMLSelectElement[];
  expect(Array.from(input.options).map((o) => o.textContent)).toEqual(['System default', 'Built-in mic (default)', 'Headset mic']);
  expect(Array.from(output.options).map((o) => o.textContent)).toEqual(['System default', 'Speakers (default)', 'Headset']);
  expect(input.value).toBe('');
  expect(output.value).toBe('Headset');
});

it('AUD-024 switching a dropdown applies the device immediately', async () => {
  const apply = vi.spyOn(api, 'setAudioDevices').mockResolvedValue();
  dispose = render(() => <DeviceSettings />, document.getElementById('root')!);
  await flush();

  const [input] = Array.from(document.querySelectorAll('select')) as HTMLSelectElement[];
  input.value = 'Headset mic';
  input.dispatchEvent(new Event('change', { bubbles: true }));
  await flush();

  expect(apply).toHaveBeenCalledWith('Headset mic', 'Headset');
  expect(selectedDevices().input).toBe('Headset mic');
});

it('AUD-024 picking "System default" clears the choice', async () => {
  const apply = vi.spyOn(api, 'setAudioDevices').mockResolvedValue();
  dispose = render(() => <DeviceSettings />, document.getElementById('root')!);
  await flush();

  const [, output] = Array.from(document.querySelectorAll('select')) as HTMLSelectElement[];
  output.value = '';
  output.dispatchEvent(new Event('change', { bubbles: true }));
  await flush();

  expect(apply).toHaveBeenCalledWith(null, null);
});
