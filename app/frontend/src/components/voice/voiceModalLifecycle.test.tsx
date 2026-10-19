import { createSignal } from 'solid-js';
import { render } from 'solid-js/web';
import { afterEach, expect, it, vi } from 'vitest';
import { VoiceSettingsModal } from './VoiceSettingsModal';
import { api } from '../../services/api';
let dispose: (()=>void)|undefined;
afterEach(()=>{dispose?.();document.body.innerHTML='';vi.restoreAllMocks();});
it('enumerates devices on opening and reopening, without touching audio while closed', async()=>{
 const get=vi.spyOn(api,'getAudioDevices').mockResolvedValue({inputs:[],outputs:[],selected_input:null,selected_output:null});
 const host=document.createElement('div');document.body.append(host);
 const [open,setOpen]=createSignal(false);
 dispose=render(()=><VoiceSettingsModal open={open()} onClose={()=>setOpen(false)}/>,host);
 expect(get).not.toHaveBeenCalled();
 setOpen(true); await Promise.resolve(); expect(get).toHaveBeenCalledTimes(1);
 setOpen(false); setOpen(true); await Promise.resolve(); expect(get).toHaveBeenCalledTimes(2);
});

function mountVoiceSettings() {
  vi.spyOn(api, 'getAudioDevices').mockResolvedValue({ inputs: [], outputs: [], selected_input: null, selected_output: null });
  const host = document.createElement('div');
  document.body.append(host);
  const [open, setOpen] = createSignal(true);
  dispose = render(() => <VoiceSettingsModal open={open()} onClose={() => setOpen(false)} />, host);
  return open;
}
const pttKeyButton = () => document.querySelector('button[aria-label="Push-to-talk key"]') as HTMLButtonElement;
const escapeOn = (element: HTMLElement) => {
  const event = new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', bubbles: true, cancelable: true });
  element.dispatchEvent(event);
  return event;
};

it('Esc while choosing the push-to-talk key cancels the choice and keeps the voice dialog open', async () => {
  const open = mountVoiceSettings();
  await Promise.resolve();
  pttKeyButton().click();
  await Promise.resolve();
  expect(pttKeyButton().textContent).toContain('Press a key');

  escapeOn(pttKeyButton());
  await Promise.resolve();
  expect(open()).toBe(true);
  expect(document.querySelector('[role="dialog"]')).toBeTruthy();
  expect(pttKeyButton().textContent).toContain('Choose key');
});

it('Esc outside key capture still closes the voice dialog', async () => {
  const open = mountVoiceSettings();
  await Promise.resolve();
  escapeOn(pttKeyButton());
  await Promise.resolve();
  expect(open()).toBe(false);
});
