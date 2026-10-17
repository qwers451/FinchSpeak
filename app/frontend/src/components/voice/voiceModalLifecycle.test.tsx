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
