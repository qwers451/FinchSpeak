import { createMemo, createSignal, For, Show } from 'solid-js';
import { setAdminOpen } from '../../stores/adminStore';
import { markRead, unread } from '../../stores/activityStore';
import { setProfileOpen } from '../../stores/profileStore';
import { activeChannel, channels, selectChannel } from '../../stores/chatStore';
import {
  activeVoiceChannel, allVoiceParticipants, isDeafened, isLocalSpeaking, isMuted, leaveVoice, pingRtt, toggleDeafen, toggleMute,
} from '../../stores/voiceStore';
import { isSharing } from '../../stores/screenState';
import { connectionStatus, knownUsers, myStatus as ownStatus, presence, statusTexts } from '../../stores/sessionStore';
import { gameOf } from '../../stores/activityGameStore';
import { currentUser } from '../../stores/authStore';
import { RoomTools } from '../voice/RoomTools';
import type { Channel, User, UserStatus } from '../../types';
import { Avatar, Button, FinchBird, Icon } from '../../ui';
import { locale, t } from '../../i18n';
import { ChannelActions, ChannelDialogs, type ChannelDialog } from '../Sidebar';
import { ChatArea } from '../ChatArea';
import { VoicePanel } from '../VoicePanel';
import { participantContextMenu, participantMenuKey } from '../voice/participantMenu';
import { NestPreview, NestRow } from './Nest';

const statusOf = (user: User): UserStatus => presence()[user.id] ?? user.status ?? 'offline';
const statusClass = (status: UserStatus) => (status === 'online' ? 'on' : status === 'idle' ? 'idle' : status === 'donotdisturb' ? 'dnd' : 'off');
const nameOf = (user: User) => user.display_name || user.username;
const isAdmin = () => currentUser()?.role === 'admin';
const isStaff = () => currentUser()?.role === 'admin' || currentUser()?.role === 'moderator';

/** The little birdhouse drawn before a text channel's name. */
function HouseIcon() {
  return (
    <svg class="house-ic" viewBox="0 0 36 44" aria-hidden="true">
      <path class="bh-roof" d="M0 18 L18 2 L36 18 Z" />
      <rect class="bh-box" x="5" y="18" width="26" height="24" rx="3" />
      <circle class="bh-hole" cx="18" cy="28" r="5" />
    </svg>
  );
}

/** A text channel as a birdhouse row; the badge counts what is unread inside. */
function HouseRow(props: { channel: Channel; onDialog: (dialog: ChannelDialog) => void }) {
  const open = () => { void selectChannel(props.channel); markRead(props.channel.id); };
  const isOpen = () => activeChannel()?.id === props.channel.id;
  return (
    <div
      class={`hrow birdhouse${isOpen() ? ' on' : ''}${unread()[props.channel.id] ? ' unread' : ''}`}
      role="button"
      tabindex="0"
      aria-current={isOpen() ? 'page' : undefined}
      aria-label={props.channel.name}
      onClick={open}
      onKeyDown={(event) => { if (event.target !== event.currentTarget) return; if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); open(); } }}
    >
      <HouseIcon />
      <span class="trunc grow" title={props.channel.name}>{props.channel.name}</span>
      <Show when={unread()[props.channel.id]}>
        {(entry) => (
          <span class={`badge${entry().mention ? ' mention' : ''}`} title={entry().mention ? t('You were mentioned') : t('Unread messages')}>
            {entry().count > 99 ? '99+' : entry().count}
          </span>
        )}
      </Show>
      <ChannelActions channel={props.channel} onDialog={props.onDialog} />
    </div>
  );
}

/**
 * Finch mode (variant A, "the tree at the side"): the trunk runs down the left column and every voice room hangs on
 * it as a nest that lists its birds by name; text channels are birdhouses below; everyone online but in no room sits
 * on the wire. The rest of the window is a full-size chat or room. Colours come from the theme, so every theme and the
 * custom designs apply here too.
 */
export function TreeView() {
  const [dialog, setDialog] = createSignal<ChannelDialog>(null);
  const nests = createMemo(() => channels().filter((channel) => channel.channel_type === 'voice'));
  const houses = createMemo(() => channels().filter((channel) => channel.channel_type === 'text'));

  const inNests = createMemo(() => new Set(allVoiceParticipants().map((participant) => participant.user_id)));
  const onWire = createMemo(() => knownUsers()
    .filter((user) => statusOf(user) !== 'offline' && !inNests().has(user.id))
    .sort((a, b) => nameOf(a).localeCompare(nameOf(b), locale())));
  const offlineCount = () => knownUsers().filter((user) => statusOf(user) === 'offline').length;
  const wireLine = (user: User) => {
    const game = gameOf(user.id);
    if (game) return t('Playing {game}', { game });
    return statusTexts()[user.id] || t(({ on: 'Online', idle: 'Idle', dnd: 'Do not disturb', off: 'Offline' })[statusClass(statusOf(user))]);
  };

  const myStatus = () => {
    const status = ownStatus();
    return status === 'donotdisturb' ? 'dnd' : status === 'idle' ? 'idle' : 'on';
  };
  const statusLabel = () => t(({ on: 'Online', idle: 'Idle', dnd: 'Do not disturb' })[myStatus()]);
  const content = () => activeChannel();

  return (
    <div class="tree-layout">
      <aside class="tree-side" aria-label={t('Tree of rooms')}>
        <header class="tree-bar">
          <img src="/custom/assets/brand/finchspeak-tile-small.svg" alt="" />
          <span class="tree-title grow">FinchSpeak</span>
          <Show when={isAdmin()}>
            <Button iconOnly size="sm" variant="ghost" icon="plus" class="tree-add-voice" label={t('Create voice channel')} onClick={() => setDialog({ kind: 'create', type: 'voice' })} />
          </Show>
        </header>

        <div class="tree-scroll">
          <span class="sec-label tree-sec">{t('Nests')}</span>
          <div class="tree-trunk">
            <For each={nests()} fallback={<div class="nempty">{t('No nests yet')}</div>}>
              {(channel) => <NestRow channel={channel} actions={(target) => <ChannelActions channel={target} onDialog={setDialog} />} />}
            </For>
          </div>

          <div class="tree-sec-row">
            <span class="sec-label tree-sec">{t('Birdhouses')}</span>
            <Show when={isAdmin()}>
              <Button iconOnly size="sm" variant="ghost" icon="plus" class="tree-add-text" label={t('Create text channel')} onClick={() => setDialog({ kind: 'create', type: 'text' })} />
            </Show>
          </div>
          <For each={houses()}>{(channel) => <HouseRow channel={channel} onDialog={setDialog} />}</For>

          <span class="sec-label tree-sec">{t('On the wire — {count}', { count: onWire().length })}</span>
          <div class="tree-wire" role="list" aria-label={t('On the wire — {count}', { count: onWire().length })}>
            <For each={onWire()}>
              {(user) => (
                <div class="wrow" role="listitem" tabindex="0" title={`${nameOf(user)} — ${wireLine(user)}`}
                  onContextMenu={(event) => participantContextMenu(event, user.id)} onKeyDown={(event) => participantMenuKey(event, user.id)}>
                  <Avatar name={nameOf(user)} id={user.id} size={24} src={user.avatar_url} status={statusClass(statusOf(user))} />
                  <span class="col grow wrow-text">
                    <span class="trunc">{nameOf(user)}</span>
                    <span class="t3 fs12 trunc">{wireLine(user)}</span>
                  </span>
                </div>
              )}
            </For>
            <Show when={offlineCount() > 0}><div class="wrow t3 fs12 tree-offline">{t('Offline — {count}', { count: offlineCount() })}</div></Show>
          </div>
        </div>

        <footer class="tree-perch">
          {/* Not a control: the gear below is the one way into profile and settings. */}
          <div class="profile-card tree-me" title={currentUser()?.display_name || currentUser()?.username}>
            <Avatar name={currentUser()?.display_name || currentUser()?.username} id={currentUser()?.id} size={32} status={myStatus()} speaking={isLocalSpeaking()} src={currentUser()?.avatar_url} />
            <span class="col tree-label">
              <span class="b trunc fs13">{currentUser()?.display_name || currentUser()?.username}</span>
              <Show when={activeVoiceChannel()} fallback={<span class="t3 fs12 trunc">{(currentUser() && statusTexts()[currentUser()!.id]) || t('On the wire. Pick a nest.')}</span>}>
                {(channel) => (
                  <span class="fs12 trunc tree-voice-state" title={t('In the nest: {channel}', { channel: channel().name })}>
                    {t('In the nest: {channel}', { channel: channel().name })}{connectionStatus() === 'connected' ? (pingRtt() !== null ? ` · ${pingRtt()} ms` : '') : ` · ${t('Voice disconnected')}`}
                  </span>
                )}
              </Show>
              <Show when={isSharing()}><span class="row g6 fs12 sharing-note"><Icon name="share" size="sm" />{t('You are sharing your screen')}</span></Show>
            </span>
          </div>
          <div class="tree-perch-controls">
            <Button iconOnly size="sm" variant={isMuted() ? 'danger' : 'ghost'} icon={isMuted() ? 'mic-off' : 'mic'} label={isMuted() ? t('Unmute') : t('Mute')} onClick={() => void toggleMute()} />
            <Button iconOnly size="sm" variant={isDeafened() ? 'danger' : 'ghost'} icon={isDeafened() ? 'phones-off' : 'phones'} label={isDeafened() ? t('Undeafen') : t('Deafen')} onClick={() => void toggleDeafen()} />
            <Show when={activeVoiceChannel()}>
              <Button iconOnly size="sm" variant="danger" icon="hangup" class="tree-leave" label={t('Leave room')} title={t('Fly out of the nest')} onClick={() => void leaveVoice()} />
            </Show>
            <Button iconOnly size="sm" variant="ghost" icon="gear" label={t('Profile and settings')} onClick={() => setProfileOpen(true)} />
            <Show when={isStaff()}>
              <Button iconOnly size="sm" variant="ghost" icon="shield" label={t('Administration')} class="ty" onClick={() => setAdminOpen(true)} />
            </Show>
          </div>
          <RoomTools compact />
        </footer>
      </aside>

      <main class="tree-main">
        <Show when={content()} fallback={
          <div class="tree-empty">
            <FinchBird size={96} />
            <h3>{t('Pick a nest or a birdhouse')}</h3>
            <span class="t3">{t('One click looks inside, a double click flies in')}</span>
          </div>
        }>
          {(channel) => (
            <Show when={channel().channel_type === 'text'} fallback={
              <Show when={activeVoiceChannel()?.id === channel().id} fallback={<NestPreview channel={channel()} />}>
                <VoicePanel />
              </Show>
            }>
              <ChatArea />
            </Show>
          )}
        </Show>
      </main>

      <ChannelDialogs dialog={dialog()} onClose={() => setDialog(null)} />
    </div>
  );
}
