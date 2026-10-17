import { afterEach, expect, it, vi } from 'vitest';
import { api } from '../services/api';
import { activeVoiceChannel, joinVoice, setActiveVoiceChannel } from './voiceStore';
import { appError, setAppError } from './notificationStore';
afterEach(() => vi.restoreAllMocks());
it('explains a failed voice join and does not show a successful voice connection', async () => {
 localStorage.clear(); setActiveVoiceChannel(null); setAppError(null);
 vi.spyOn(api,'joinVoiceChannel').mockRejectedValue(new Error('Connection refused'));
 vi.spyOn(console,'error').mockImplementation(()=>{});
 await joinVoice({id:'voice',short_id:1,name:'Room',channel_type:'voice',position:0,bitrate_kbps:64});
 expect(activeVoiceChannel()).toBeNull(); expect(appError()).toContain('Connection refused');
});
