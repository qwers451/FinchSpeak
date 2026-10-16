import { createSignal, For, Show } from 'solid-js';
import { setAdminOpen } from '../stores/adminStore';
import { markRead, unread } from '../stores/activityStore';
import { setProfileOpen } from '../stores/profileStore';
import { channels, activeChannel, selectChannel, loadChannels } from '../stores/chatStore';
import {
  activeVoiceChannel,
  joinVoice,
  leaveVoice,
  isMuted,
  isDeafened,
  toggleMute,
  toggleDeafen,
  isLocalSpeaking,
  pingRtt,
  participantsForChannel,
  speakingPeers,
} from '../stores/voiceStore';
import { connectionStatus, presence, statusTexts } from '../stores/sessionStore';
import { setAppError } from '../stores/notificationStore';
import { currentUser } from '../stores/authStore';
import type { Channel } from '../types';
import { api } from '../services/api';
import { moveChannel, removeChannel, renameChannel } from '../stores/channelAdminStore';
import { Avatar, Button, ConfirmDialog, Field, Icon, Modal } from '../ui';

type ChannelDialog =
  | { kind: 'create'; type: 'text' | 'voice' }
  | { kind: 'rename'; channel: Channel }
  | { kind: 'delete'; channel: Channel }
  | null;

const isAdmin = () => currentUser()?.role === 'admin';
const isStaff = () => currentUser()?.role === 'admin' || currentUser()?.role === 'moderator';

/** Up / down / rename / delete, shown on hover (and keyboard focus) to admins. */
function ChannelActions(props: { channel: Channel; onDialog: (dialog: ChannelDialog) => void }) {
  const act = (action: () => void) => (event: MouseEvent) => {
    event.stopPropagation();
    action();
  };
  return (
    <Show when={isAdmin()}>
      <span class="acts">
        <button type="button" aria-label={`Move ${props.channel.name} up`} title="Move up" onClick={act(() => void moveChannel(props.channel, -1))}><Icon name="up" /></button>
        <button type="button" aria-label={`Move ${props.channel.name} down`} title="Move down" onClick={act(() => void moveChannel(props.channel, 1))}><Icon name="down" /></button>
        <button type="button" aria-label={`Rename ${props.channel.name}`} title="Rename" onClick={act(() => props.onDialog({ kind: 'rename', channel: props.channel }))}><Icon name="edit" /></button>
        <button type="button" aria-label={`Delete ${props.channel.name}`} title="Delete" onClick={act(() => props.onDialog({ kind: 'delete', channel: props.channel }))}><Icon name="trash" /></button>
      </span>
    </Show>
  );
}

function ChannelNameDialog(props: { dialog: Extract<ChannelDialog, { kind: 'create' | 'rename' }>; onClose: () => void }) {
  const initial = () => (props.dialog.kind === 'rename' ? props.dialog.channel.name : '');
  const [name, setName] = createSignal(initial());
  const [busy, setBusy] = createSignal(false);
  const title = () => (props.dialog.kind === 'rename' ? 'Rename channel' : props.dialog.type === 'voice' ? 'New voice channel' : 'New text channel');

  const submit = async (event: Event) => {
    event.preventDefault();
    const value = name().trim();
    if (!value || busy()) return;
    setBusy(true);
    try {
      if (props.dialog.kind === 'rename') await renameChannel(props.dialog.channel, value);
      else {
        await api.createChannel(value, props.dialog.type);
        await loadChannels();
      }
      props.onClose();
    } catch (error) {
      setAppError(String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal open onClose={props.onClose} title={title()} width="420px">
      <form class="col g16" onSubmit={submit}>
        <Field label="Channel name" value={name()} onInput={setName} placeholder={props.dialog.kind === 'create' && props.dialog.type === 'voice' ? 'Quiet corner' : 'general'} autofocus />
        <div class="row g8" style={{ 'justify-content': 'flex-end' }}>
          <Button variant="secondary" onClick={props.onClose}>Cancel</Button>
          <Button type="submit" variant="primary" loading={busy()} disabled={!name().trim()}>{props.dialog.kind === 'rename' ? 'Save' : 'Create'}</Button>
        </div>
      </form>
    </Modal>
  );
}

export function Sidebar() {
  const [dialog, setDialog] = createSignal<ChannelDialog>(null);
  const [collapsed, setCollapsed] = createSignal<{ text: boolean; voice: boolean }>({ text: false, voice: false });
  const textChannels = () => channels().filter((c) => c.channel_type === 'text');
  const voiceChannels = () => channels().filter((c) => c.channel_type === 'voice');
  const myStatus = () => {
    const user = currentUser();
    const status = user ? presence()[user.id] ?? user.status ?? 'online' : 'online';
    return status === 'donotdisturb' ? 'dnd' : status === 'idle' ? 'idle' : 'on';
  };
  const statusLabel = () => ({ on: 'Online', idle: 'Idle', dnd: 'Do not disturb' })[myStatus()];

  const toggleCategory = (kind: 'text' | 'voice') => setCollapsed((previous) => ({ ...previous, [kind]: !previous[kind] }));

  return (
    <aside class="side sidebar" aria-label="Channels">
      <div class="brand">
        <img src="./mark.svg" alt="" />
        <span>FinchSpeak</span>
      </div>

      <nav class="nav">
        <div class="cat">
          <button type="button" class="cat-toggle" aria-expanded={!collapsed().text} onClick={() => toggleCategory('text')}>
            <Icon name={collapsed().text ? 'chev-r' : 'chev-d'} />Text channels
          </button>
          <Show when={isAdmin()}>
            <button type="button" class="cat-add" title="Create text channel" aria-label="Create text channel" onClick={() => setDialog({ kind: 'create', type: 'text' })}><Icon name="plus" /></button>
          </Show>
        </div>
        <Show when={!collapsed().text}>
          <For each={textChannels()}>
            {(channel) => (
              <div
                class={`ch channel-item${activeChannel()?.id === channel.id ? ' on' : ''}${unread()[channel.id] ? ' unread' : ''}`}
                role="button"
                tabindex="0"
                aria-current={activeChannel()?.id === channel.id ? 'page' : undefined}
                onClick={() => { void selectChannel(channel); markRead(channel.id); }}
                onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); void selectChannel(channel); markRead(channel.id); } }}
              >
                <Icon name="hash" size="sm" />
                <span class="trunc">{channel.name}</span>
                <ChannelActions channel={channel} onDialog={setDialog} />
                <Show when={unread()[channel.id]}>
                  {(entry) => (
                    <span class={`badge${entry().mention ? ' mention' : ''}`} style={{ 'margin-left': 'auto' }} title={entry().mention ? 'You were mentioned' : 'Unread messages'}>
                      {entry().count > 99 ? '99+' : entry().count}
                    </span>
                  )}
                </Show>
              </div>
            )}
          </For>
        </Show>

        <div class="cat">
          <button type="button" class="cat-toggle" aria-expanded={!collapsed().voice} onClick={() => toggleCategory('voice')}>
            <Icon name={collapsed().voice ? 'chev-r' : 'chev-d'} />Voice channels
          </button>
          <Show when={isAdmin()}>
            <button type="button" class="cat-add" title="Create voice channel" aria-label="Create voice channel" onClick={() => setDialog({ kind: 'create', type: 'voice' })}><Icon name="plus" /></button>
          </Show>
        </div>
        <Show when={!collapsed().voice}>
          <For each={voiceChannels()}>
            {(channel) => (
              <>
                <div
                  class={`ch channel-item${activeVoiceChannel()?.id === channel.id ? ' on' : ''}`}
                  role="button"
                  tabindex="0"
                  onClick={() => { void selectChannel(channel); if (activeVoiceChannel()?.id !== channel.id) void joinVoice(channel); }}
                  onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); void selectChannel(channel); if (activeVoiceChannel()?.id !== channel.id) void joinVoice(channel); } }}
                >
                  <Icon name="vol" size="sm" />
                  <span class="trunc">{channel.name}</span>
                  <ChannelActions channel={channel} onDialog={setDialog} />
                  <Show when={participantsForChannel(channel.id).length > 0}>
                    <span class="tag violet" style={{ 'margin-left': 'auto' }}>{participantsForChannel(channel.id).length}</span>
                  </Show>
                </div>
                <For each={participantsForChannel(channel.id)}>
                  {(participant) => (
                    <div class="vp">
                      <Avatar name={participant.username} id={participant.user_id} size={24} speaking={speakingPeers().has(participant.peer_id)} />
                      <span class="grow trunc">{participant.username}</span>
                      <Show when={participant.is_muted}><Icon name="mic-off" title="Muted" style={{ color: 'var(--red-text)' }} /></Show>
                      <Show when={participant.is_deafened}><Icon name="phones-off" title="Deafened" style={{ color: 'var(--red-text)' }} /></Show>
                      <Show when={participant.is_sharing}><Icon name="share" title="Sharing screen" style={{ color: 'var(--cyan)' }} /></Show>
                    </div>
                  )}
                </For>
              </>
            )}
          </For>
        </Show>
      </nav>

      <Show when={activeVoiceChannel()}>
        {(channel) => (
          <div class="vstrip voice-banner">
            <div class="col grow" style={{ 'min-width': 0 }}>
              <span class="row g6 b fs13" style={{ color: '#A39CFF' }}>
                <Icon name="bars" size="sm" />{connectionStatus() === 'connected' ? 'Voice connected' : 'Voice disconnected'}
              </span>
              <span class="t2 fs12 trunc">{channel().name}{pingRtt() !== null ? ` · ${pingRtt()} ms` : ''}</span>
            </div>
            <Button iconOnly size="sm" variant="danger" icon="hangup" label="Disconnect from voice" title="Disconnect" onClick={() => void leaveVoice()} />
          </div>
        )}
      </Show>

      <div class="upanel user-panel">
        <button type="button" class="row g10 grow profile-open" style={{ gap: '10px', 'min-width': 0 }} aria-label="Open profile" title={currentUser()?.display_name || currentUser()?.username} onClick={() => setProfileOpen(true)}>
          <Avatar name={currentUser()?.display_name ?? currentUser()?.username} id={currentUser()?.id} size={40} status={myStatus()} speaking={isLocalSpeaking()} src={currentUser()?.avatar_url} />
          <div class="col grow" style={{ 'min-width': 0, 'text-align': 'left' }}>
            <span class="b trunc fs13">{currentUser()?.display_name || currentUser()?.username}</span>
            <span class="t3 fs12 trunc">{(currentUser() && statusTexts()[currentUser()!.id]) || statusLabel()}</span>
          </div>
        </button>
        <Button iconOnly size="sm" variant={isMuted() ? 'danger' : 'ghost'} icon={isMuted() ? 'mic-off' : 'mic'} label={isMuted() ? 'Unmute' : 'Mute'} title={isMuted() ? 'Unmute' : 'Mute'} onClick={() => void toggleMute()} />
        <Button iconOnly size="sm" variant={isDeafened() ? 'danger' : 'ghost'} icon={isDeafened() ? 'phones-off' : 'phones'} label={isDeafened() ? 'Undeafen' : 'Deafen'} title={isDeafened() ? 'Undeafen' : 'Deafen'} onClick={() => void toggleDeafen()} />
        <Button iconOnly size="sm" variant="ghost" icon="sliders" label="Profile and settings" title="Profile and settings" onClick={() => setProfileOpen(true)} />
        <Show when={isStaff()}>
          <Button iconOnly size="sm" variant="ghost" icon="shield" label="Administration" title="Administration" class="ty" onClick={() => setAdminOpen(true)} />
        </Show>
      </div>

      <Show when={dialog()}>
        {(current) => (
          <>
            <Show when={current().kind !== 'delete'}>
              <ChannelNameDialog dialog={current() as Extract<ChannelDialog, { kind: 'create' | 'rename' }>} onClose={() => setDialog(null)} />
            </Show>
            <ConfirmDialog
              open={current().kind === 'delete'}
              danger
              title={`Delete #${(current() as { channel?: Channel }).channel?.name ?? ''}?`}
              message="All messages and files in this channel will be removed. People in a deleted voice channel are sent out."
              confirmLabel="Delete channel"
              onCancel={() => setDialog(null)}
              onConfirm={() => { const d = current(); if (d.kind === 'delete') void removeChannel(d.channel); setDialog(null); }}
            />
          </>
        )}
      </Show>
    </aside>
  );
}
