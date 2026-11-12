import { createSignal, For, Show } from 'solid-js';
import { moveVoice, setAdminOpen } from '../stores/adminStore';
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
import { isLocallyMuted } from '../stores/localMuteStore';
import { isSharing } from '../stores/screenState';
import { participantContextMenu, participantMenuKey } from './voice/participantMenu';
import { isMeParticipant } from './voice/ParticipantTile';
import { VOICE_USER_DRAG, canDrag, dropTarget, readVoiceDrag, type VoiceDrag } from './voice/voiceDrag';
import { setNestView } from '../stores/nestView';
import { musicAvailable, musicState, setMusicPanelOpen } from '../stores/musicStore';
import { connectionStatus, localPeerId, knownUsers, myStatus as ownStatus, statusTexts } from '../stores/sessionStore';
import { setAppError } from '../stores/notificationStore';
import { currentUser } from '../stores/authStore';
import { gameOf } from '../stores/activityGameStore';
import type { Channel, VoiceParticipant } from '../types';
import { api } from '../services/api';
import { moveChannel, removeChannel, renameChannel } from '../stores/channelAdminStore';
import { Avatar, Button, ConfirmDialog, Field, Icon, Modal } from '../ui';
import { t } from '../i18n';
import { describeError } from '../i18n/errors';

export type ChannelDialog =
  | { kind: 'create'; type: 'text' | 'voice' }
  | { kind: 'rename'; channel: Channel }
  | { kind: 'delete'; channel: Channel }
  | null;

const isAdmin = () => currentUser()?.role === 'admin';
const isStaff = () => currentUser()?.role === 'admin' || currentUser()?.role === 'moderator';

/** Up / down / rename / delete, shown on hover (and keyboard focus) to admins. */
export function ChannelActions(props: { channel: Channel; onDialog: (dialog: ChannelDialog) => void }) {
  const act = (action: () => void) => (event: MouseEvent) => {
    event.stopPropagation();
    action();
  };
  return (
    <Show when={isAdmin()}>
      <span class="acts">
        <button type="button" aria-label={t('Move {channel} up', { channel: props.channel.name })} title={t('Move up')} onClick={act(() => void moveChannel(props.channel, -1))}><Icon name="up" /></button>
        <button type="button" aria-label={t('Move {channel} down', { channel: props.channel.name })} title={t('Move down')} onClick={act(() => void moveChannel(props.channel, 1))}><Icon name="down" /></button>
        <button type="button" aria-label={t('Rename {channel}', { channel: props.channel.name })} title={t('Rename')} onClick={act(() => props.onDialog({ kind: 'rename', channel: props.channel }))}><Icon name="edit" /></button>
        <button type="button" aria-label={t('Delete {channel}', { channel: props.channel.name })} title={t('Delete')} onClick={act(() => props.onDialog({ kind: 'delete', channel: props.channel }))}><Icon name="trash" /></button>
      </span>
    </Show>
  );
}

function ChannelNameDialog(props: { dialog: Extract<ChannelDialog, { kind: 'create' | 'rename' }>; onClose: () => void }) {
  const initial = () => (props.dialog.kind === 'rename' ? props.dialog.channel.name : '');
  const [name, setName] = createSignal(initial());
  const [busy, setBusy] = createSignal(false);
  const title = () => t(props.dialog.kind === 'rename' ? 'Rename channel' : props.dialog.type === 'voice' ? 'New voice channel' : 'New text channel');

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
      setAppError(describeError(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal open onClose={props.onClose} title={title()} width="420px">
      <form class="col g16" onSubmit={submit}>
        <Field label={t('Channel name')} value={name()} onInput={setName} placeholder={props.dialog.kind === 'create' && props.dialog.type === 'voice' ? t('Quiet corner') : t('general')} autofocus />
        <div class="row g8 dialog-actions-primary">
          <Button variant="secondary" onClick={props.onClose}>{t('Cancel')}</Button>
          <Button type="submit" variant="primary" loading={busy()} disabled={!name().trim()}>{props.dialog.kind === 'rename' ? t('Save') : t('Create')}</Button>
        </div>
      </form>
    </Modal>
  );
}

/** The create / rename / delete dialogs behind the channel actions; shared by the classic sidebar and the Finch tree. */
export function ChannelDialogs(props: { dialog: ChannelDialog; onClose: () => void }) {
  return (
    <Show when={props.dialog}>
      {(current) => (
        <>
          <Show when={current().kind !== 'delete'}>
            <ChannelNameDialog dialog={current() as Extract<ChannelDialog, { kind: 'create' | 'rename' }>} onClose={props.onClose} />
          </Show>
          <ConfirmDialog
            open={current().kind === 'delete'}
            danger
            title={t('Delete #{channel}?', { channel: (current() as { channel?: Channel }).channel?.name ?? '' })}
            message={t('All messages and files in this channel will be removed. People in a deleted voice channel are sent out.')}
            confirmLabel={t('Delete channel')}
            onCancel={props.onClose}
            onConfirm={() => { const d = current(); if (d.kind === 'delete') void removeChannel(d.channel); props.onClose(); }}
          />
        </>
      )}
    </Show>
  );
}

export function Sidebar() {
  const [dragged, setDragged] = createSignal<VoiceDrag | null>(null);
  const [dropChannel, setDropChannel] = createSignal<string | null>(null);
  const dragStart = (event: DragEvent, participant: VoiceParticipant) => {
    if (!canDrag(participant) || !event.dataTransfer) { event.preventDefault(); return; }
    const payload = { userId: participant.user_id, peerId: participant.peer_id, from: participant.channel_id };
    event.dataTransfer.setData(VOICE_USER_DRAG, JSON.stringify(payload));
    event.dataTransfer.effectAllowed = 'move';
    setDragged(payload);
  };
  const dragOver = (event: DragEvent, channel: Channel) => {
    if (!event.dataTransfer?.types.includes(VOICE_USER_DRAG) || event.dataTransfer.types.includes('Files') || !dropTarget(dragged(), channel)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = 'move';
    setDropChannel(channel.id);
  };
  const drop = (event: DragEvent, channel: Channel) => {
    const participant = dropTarget(readVoiceDrag(event.dataTransfer), channel);
    setDropChannel(null);
    setDragged(null);
    if (!participant) return;
    event.preventDefault();
    event.stopPropagation();
    if (isMeParticipant(participant, localPeerId())) void joinVoice(channel);
    else void moveVoice(participant.user_id, participant.peer_id, channel.id);
  };
  const [dialog, setDialog] = createSignal<ChannelDialog>(null);
  const [collapsed, setCollapsed] = createSignal<{ text: boolean; voice: boolean }>({ text: false, voice: false });
  const textChannels = () => channels().filter((c) => c.channel_type === 'text');
  const voiceChannels = () => channels().filter((c) => c.channel_type === 'voice');
  const myStatus = () => {
    const status = ownStatus();
    return status === 'donotdisturb' ? 'dnd' : status === 'idle' ? 'idle' : 'on';
  };
  const statusLabel = () => t(({ on: 'Online', idle: 'Idle', dnd: 'Do not disturb' })[myStatus()]);

  const toggleCategory = (kind: 'text' | 'voice') => setCollapsed((previous) => ({ ...previous, [kind]: !previous[kind] }));

  return (
    <aside class="side sidebar" aria-label={t('Channels')}>
      <div class="brand">
        <img src="/custom/assets/brand/finch-mark.svg" alt="" />
        <span>FinchSpeak</span>
      </div>

      <nav class="nav">
        <div class="cat">
      <button type="button" class="cat-toggle" aria-expanded={!collapsed().text} onClick={() => toggleCategory('text')}>
            <Icon name={collapsed().text ? 'chev-r' : 'chev-d'} />{t('Text channels')}
          </button>
          <Show when={isAdmin()}>
            <button type="button" class="cat-add" title={t('Create text channel')} aria-label={t('Create text channel')} onClick={() => setDialog({ kind: 'create', type: 'text' })}><Icon name="plus" /></button>
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
                onKeyDown={(event) => { if (event.target !== event.currentTarget) return; if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); void selectChannel(channel); markRead(channel.id); } }}
              >
                <Icon name="hash" size="sm" />
                <span class="trunc" title={channel.name}>{channel.name}</span>
                <ChannelActions channel={channel} onDialog={setDialog} />
                <Show when={unread()[channel.id]}>
                  {(entry) => (
                    <span class={`badge${entry().mention ? ' mention' : ''}`} title={entry().mention ? t('You were mentioned') : t('Unread messages')}>
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
            <Icon name={collapsed().voice ? 'chev-r' : 'chev-d'} />{t('Voice channels')}
          </button>
          <button type="button" class="cat-add nest-toggle" title={t('Switch to Finch mode')} aria-label={t('Switch to Finch mode')} onClick={() => setNestView(true)}><Icon name="feather" /></button>
          <Show when={isAdmin()}>
            <button type="button" class="cat-add" title={t('Create voice channel')} aria-label={t('Create voice channel')} onClick={() => setDialog({ kind: 'create', type: 'voice' })}><Icon name="plus" /></button>
          </Show>
        </div>
        <Show when={!collapsed().voice}>
          <For each={voiceChannels()}>
            {(channel) => (
              <div class="voice-channel-group" onDragOver={(event) => dragOver(event, channel)} onDrop={(event) => drop(event, channel)}
                onDragLeave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropChannel(null); }}>
                <div
                  class={`ch channel-item${activeVoiceChannel()?.id === channel.id ? ' on' : ''}${dropChannel() === channel.id ? ' drop-target' : ''}`}
                  role="button"
                  tabindex="0"
                  onClick={() => { void selectChannel(channel); if (activeVoiceChannel()?.id !== channel.id) void joinVoice(channel); }}
                  onKeyDown={(event) => { if (event.target !== event.currentTarget) return; if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); void selectChannel(channel); if (activeVoiceChannel()?.id !== channel.id) void joinVoice(channel); } }}
                >
                  <Icon name="vol" size="sm" />
                  <span class="trunc" title={channel.name}>{channel.name}</span>
                  <ChannelActions channel={channel} onDialog={setDialog} />
                  <Show when={participantsForChannel(channel.id).some((participant) => participant.is_sharing)}>
                    <span class="tag live channel-live">{t('LIVE')}</span>
                  </Show>
                  <Show when={participantsForChannel(channel.id).length > 0}>
                    <span class="tag violet channel-count" classList={{ 'with-live': participantsForChannel(channel.id).some((participant) => participant.is_sharing) }}>{participantsForChannel(channel.id).length}</span>
                  </Show>
                </div>
                <div class="voice-people" classList={{ 'drop-target': dropChannel() === channel.id }}>
                <For each={participantsForChannel(channel.id)}>
                  {(participant) => (
                    <div class="vp" tabindex="0" draggable={canDrag(participant)}
                      onDragStart={(event) => dragStart(event, participant)} onDragEnd={() => { setDragged(null); setDropChannel(null); }} onContextMenu={(event) => participantContextMenu(event, participant.user_id, participant)} onKeyDown={(event) => participantMenuKey(event, participant.user_id, participant)}>
                      <Avatar name={participant.username} id={participant.user_id} size={24} src={knownUsers().find((user) => user.id === participant.user_id)?.avatar_url} speaking={speakingPeers().has(participant.peer_id) || (isMeParticipant(participant, localPeerId()) && !isMuted() && isLocalSpeaking())} levelPeerId={participant.peer_id} />
                      <span class="grow trunc">{participant.username}</span><Show when={gameOf(participant.user_id)}><Icon name="gamepad" title={t('Playing {game}', { game: gameOf(participant.user_id)! })} /></Show>
                      <Show when={!isMeParticipant(participant, localPeerId()) && isLocallyMuted(participant.user_id)}><Icon name="vol-off" title={t('You muted this person')} /></Show>
                      <Show when={participant.is_muted}><Icon name="mic-off" title={t('Muted')} class="tr" /></Show>
                      <Show when={participant.is_deafened}><Icon name="phones-off" title={t('Deafened')} class="tr" /></Show>
                      <Show when={participant.is_sharing}><span class="tag live" title={t('Sharing screen')}><Icon name="share" size="sm" /> {t('LIVE')}</span></Show>
                    </div>
                  )}
                </For>
                </div>
              </div>
            )}
          </For>
        </Show>
      </nav>

      <Show when={activeVoiceChannel()}>
        {(channel) => (
          <div class="vstrip voice-banner">
            <div class="col grow">
                <span class="row g6 b fs13 voice-connection">
                <Icon name="bars" size="sm" />{connectionStatus() === 'connected' ? t('Voice connected') : t('Voice disconnected')}
              </span>
              <span class="t2 fs12 trunc">{channel().name}{pingRtt() !== null ? ` · ${pingRtt()} ms` : ''}</span>
              <Show when={isSharing()}><span class="row g6 fs12 tr"><Icon name="share" size="sm" />{t('You are sharing your screen')}</span></Show>
            </div>
            <Show when={musicAvailable() && musicState()?.playing}>
              <button type="button" class="music-note" aria-label={t('Music is playing')} title={t('Music')} onClick={() => setMusicPanelOpen(true)}><Icon name="music" size="sm" /></button>
            </Show>
            <Button iconOnly size="sm" variant="danger" icon="hangup" label={t('Disconnect from voice')} title={t('Disconnect')} onClick={() => void leaveVoice()} />
          </div>
        )}
      </Show>

      <div class="upanel user-panel">
        <button type="button" class="row g12 grow profile-open" aria-label={t('Open profile')} title={currentUser()?.display_name || currentUser()?.username} onClick={() => setProfileOpen(true)}>
          <Avatar name={currentUser()?.display_name || currentUser()?.username} id={currentUser()?.id} size={40} status={myStatus()} speaking={isLocalSpeaking()} src={currentUser()?.avatar_url} levelPeerId={localPeerId() ?? undefined} />
          <div class="col grow">
            <span class="b trunc fs13">{currentUser()?.display_name || currentUser()?.username}</span>
            <span class="t3 fs12 trunc">{(currentUser() && statusTexts()[currentUser()!.id]) || statusLabel()}</span>
          </div>
        </button>
        <Button iconOnly size="sm" variant={isMuted() ? 'danger' : 'ghost'} icon={isMuted() ? 'mic-off' : 'mic'} label={isMuted() ? t('Unmute') : t('Mute')} title={isMuted() ? t('Unmute') : t('Mute')} onClick={() => void toggleMute()} />
        <Button iconOnly size="sm" variant={isDeafened() ? 'danger' : 'ghost'} icon={isDeafened() ? 'phones-off' : 'phones'} label={isDeafened() ? t('Undeafen') : t('Deafen')} title={isDeafened() ? t('Undeafen') : t('Deafen')} onClick={() => void toggleDeafen()} />
        <Button iconOnly size="sm" variant="ghost" icon="sliders" label={t('Profile and settings')} title={t('Profile and settings')} onClick={() => setProfileOpen(true)} />
        <Show when={isStaff()}>
          <Button iconOnly size="sm" variant="ghost" icon="shield" label={t('Administration')} title={t('Administration')} class="ty" onClick={() => setAdminOpen(true)} />
        </Show>
      </div>

      <ChannelDialogs dialog={dialog()} onClose={() => setDialog(null)} />
    </aside>
  );
}
