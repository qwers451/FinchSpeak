import { render } from 'solid-js/web';
import { afterEach, expect, it, vi } from 'vitest';
import { api } from '../../services/api';
import { VoiceSettingsModal } from './VoiceSettingsModal';

let dispose: (() => void) | undefined;
let host: HTMLDivElement;

afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
  vi.restoreAllMocks();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function mount() {
  host = document.createElement('div');
  document.body.append(host);
  dispose = render(() => <VoiceSettingsModal open onClose={() => {}} />, host);
}

it('starts and stops a diagnostics capture and reports local status', async () => {
  const start = vi.spyOn(api, 'startAudioDiagnostics').mockResolvedValue();
  const stop = vi.spyOn(api, 'stopAudioDiagnostics').mockResolvedValue();
  mount();

  const startButton = host.querySelector<HTMLButtonElement>('[aria-label="Start or reset audio diagnostics"]')!;
  startButton.click();
  await Promise.resolve();
  expect(start).toHaveBeenCalledOnce();
  expect(host.textContent).toContain('Capturing audio diagnostics…');
  expect(startButton.disabled).toBe(false);
  startButton.click();
  await Promise.resolve();
  expect(start).toHaveBeenCalledTimes(2);

  host.querySelector<HTMLButtonElement>('[aria-label="Stop audio diagnostics"]')!.click();
  await Promise.resolve();
  expect(stop).toHaveBeenCalledOnce();
  expect(host.textContent).toContain('Capture stopped. You can save the report.');
});

it('downloads the JSON report under the fixed safe filename and revokes its URL after scheduling', async () => {
  vi.spyOn(api, 'getAudioDiagnostics').mockResolvedValue('{"frames":12}');
  const createObjectURL = vi.fn((_blob: Blob) => 'blob:audio-diagnostics');
  const revokeObjectURL = vi.fn();
  vi.stubGlobal('URL', { createObjectURL, revokeObjectURL });
  const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {});
  vi.useFakeTimers();
  mount();

  host.querySelector<HTMLButtonElement>('[aria-label="Save audio diagnostics report"]')!.click();
  await Promise.resolve();

  expect(createObjectURL).toHaveBeenCalledOnce();
  const blob = createObjectURL.mock.calls[0][0] as Blob;
  expect(blob.type).toBe('application/json');
  expect(await blob.text()).toBe('{"frames":12}');
  expect(click).toHaveBeenCalledOnce();
  expect(revokeObjectURL).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1000);
  expect(revokeObjectURL).toHaveBeenCalledWith('blob:audio-diagnostics');
});

it('shows backend errors and explains that browser mode cannot run diagnostics', async () => {
  vi.spyOn(api, 'startAudioDiagnostics').mockRejectedValue(new Error('Capture is unavailable'));
  mount();
  host.querySelector<HTMLButtonElement>('[aria-label="Start or reset audio diagnostics"]')!.click();
  await Promise.resolve();
  await Promise.resolve();
  expect(host.querySelector('[role="alert"]')?.textContent).toContain('Capture is unavailable');

  await expect(api.getAudioDiagnostics()).rejects.toThrow('Audio diagnostics need the desktop app');
});
