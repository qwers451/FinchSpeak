import { createMemo, createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { setAdminOpen } from '../../stores/adminStore';
import { markRead, unread } from '../../stores/activityStore';
import { setProfileOpen } from '../../stores/profileStore';
import { activeChannel, channels, selectChannel, setActiveChannel } from '../../stores/chatStore';
import {
  activeVoiceChannel, allVoiceParticipants, isDeafened, isLocalSpeaking, isMuted, leaveVoice, pingRtt, toggleDeafen, toggleMute,
} from '../../stores/voiceStore';
import { isSharing } from '../../stores/screenState';
import { connectionStatus, knownUsers, myStatus as ownStatus, presence, statusTexts } from '../../stores/sessionStore';
import { currentUser } from '../../stores/authStore';
import { branchPlan, setNestView } from '../../stores/nestView';
import type { Channel, User, UserStatus } from '../../types';
import { Avatar, Button, Icon } from '../../ui';
import { locale, t } from '../../i18n';
import { ChannelActions, ChannelDialogs, type ChannelDialog } from '../Sidebar';
import { ChatArea } from '../ChatArea';
import { VoicePanel } from '../VoicePanel';
import { participantContextMenu, participantMenuKey } from '../voice/participantMenu';
import { Nest } from './Nest';

const NEST_SPACE = 176; // px one nest takes on a branch, gap included
const TRUNK_SPACE = 96; // px the trunk and the paddings take from the scene width

const statusOf = (user: User): UserStatus => presence()[user.id] ?? user.status ?? 'offline';
const statusClass = (status: UserStatus) => (status === 'online' ? 'on' : status === 'idle' ? 'idle' : status === 'donotdisturb' ? 'dnd' : 'off');
const nameOf = (user: User) => user.display_name || user.username;
const isAdmin = () => currentUser()?.role === 'admin';
const isStaff = () => currentUser()?.role === 'admin' || currentUser()?.role === 'moderator';

/** A text channel as a birdhouse hanging from a branch; the badge counts what is unread inside. */
function Birdhouse(props: { channel: Channel; onDialog: (dialog: ChannelDialog) => void }) {
  const open = () => { void selectChannel(props.channel); markRead(props.channel.id); };
  const isOpen = () => activeChannel()?.id === props.channel.id;
  return (
    <div
      class={`birdhouse${isOpen() ? ' on' : ''}${unread()[props.channel.id] ? ' unread' : ''}`}
      role="button"
      tabindex="0"
      aria-current={isOpen() ? 'page' : undefined}
      aria-label={props.channel.name}
      onClick={open}
      onKeyDown={(event) => { if (event.target !== event.currentTarget) return; if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); open(); } }}
    >
      <div class="birdhouse-art">
        <svg viewBox="0 0 48 58" aria-hidden="true">
          <path class="bh-string" d="M24 0 L24 12" />
          <path class="bh-roof" d="M2 28 L24 10 L46 28 Z" />
          <rect class="bh-box" x="7" y="28" width="34" height="28" rx="3" />
          <circle class="bh-hole" cx="24" cy="39" r="6" />
          <path class="bh-perch" d="M19 50 L29 50" />
        </svg>
        <Show when={unread()[props.channel.id]}>
          {(entry) => (
            <span class={`badge${entry().mention ? ' mention' : ''}`} title={entry().mention ? t('You were mentioned') : t('Unread messages')}>
              {entry().count > 99 ? '99+' : entry().count}
            </span>
          )}
        </Show>
      </div>
      <span class="birdhouse-name trunc" title={props.channel.name}>{props.channel.name}</span>
      <ChannelActions channel={props.channel} onDialog={props.onDialog} />
    </div>
  );
}

/**
 * Finch mode: the whole window is one tree. Voice rooms are nests on its branches, text channels are birdhouses
 * hanging below them, everyone online but not in a room sits on the wire, and your own controls are on the perch.
 */
export function TreeView() {
  let scene: HTMLDivElement | undefined;
  const [dialog, setDialog] = createSignal<ChannelDialog>(null);
  const [folded, setFolded] = createSignal(false);
  const [perBranch, setPerBranch] = createSignal(4);

  onMount(() => {
    if (!scene || typeof ResizeObserver === 'undefined') return;
    const measure = () => setPerBranch(Math.min(8, Math.max(2, Math.floor(((scene?.clientWidth ?? 0) - TRUNK_SPACE) / NEST_SPACE))));
    const observer = new ResizeObserver(measure);
    observer.observe(scene);
    measure();
    onCleanup(() => observer.disconnect());
  });

  const branches = createMemo(() => branchPlan(
    channels().filter((channel) => channel.channel_type === 'voice'),
    channels().filter((channel) => channel.channel_type === 'text'),
    perBranch(),
  ));

  const inNests = createMemo(() => new Set(allVoiceParticipants().map((participant) => participant.user_id)));
  const onWire = createMemo(() => knownUsers()
    .filter((user) => statusOf(user) !== 'offline' && !inNests().has(user.id))
    .sort((a, b) => nameOf(a).localeCompare(nameOf(b), locale())));
  const offlineCount = () => knownUsers().filter((user) => statusOf(user) === 'offline').length;

  const myStatus = () => {
    const status = ownStatus();
    return status === 'donotdisturb' ? 'dnd' : status === 'idle' ? 'idle' : 'on';
  };
  const statusLabel = () => t(({ on: 'Online', idle: 'Idle', dnd: 'Do not disturb' })[myStatus()]);

  return (
    <div class={`tree-layout${activeChannel() ? ' has-content' : ''}${folded() && activeChannel() ? ' folded' : ''}`}>
      <header class="tree-bar">
        <img src="/custom/assets/brand/finch-mark.svg" alt="" />
        <span class="tree-title">FinchSpeak</span>
        <span class="grow" />
        <Show when={isAdmin()}>
          <Button size="sm" variant="ghost" icon="plus" class="tree-add-voice" title={t('Create voice channel')} onClick={() => setDialog({ kind: 'create', type: 'voice' })}>{t('Nest')}</Button>
          <Button size="sm" variant="ghost" icon="plus" class="tree-add-text" title={t('Create text channel')} onClick={() => setDialog({ kind: 'create', type: 'text' })}>{t('Birdhouse')}</Button>
        </Show>
        <Show when={activeChannel()}>
          <Button iconOnly size="sm" variant="ghost" icon={folded() ? 'chev-r' : 'chev-d'} class="tree-fold" aria-expanded={!folded()} label={folded() ? t('Show the tree') : t('Hide the tree')} onClick={() => setFolded(!folded())} />
          <Button iconOnly size="sm" variant="ghost" icon="x" class="tree-close" label={t('Back to the tree')} onClick={() => { setActiveChannel(null); setFolded(false); }} />
        </Show>
        <Button iconOnly size="sm" variant="ghost" icon="bars" class="nest-toggle" label={t('Switch to classic mode')} onClick={() => setNestView(false)} />
      </header>

      <div class="tree-scene" ref={scene} aria-label={t('Tree of rooms')}>
        <div class="tree-wire" role="list" aria-label={t('On the wire — {count}', { count: onWire().length })}>
          <span class="sec-label tree-wire-label">{t('On the wire — {count}', { count: onWire().length })}</span>
          <For each={onWire()}>
            {(user) => (
              <span class="tree-wire-bird" role="listitem" tabindex="0" title={statusTexts()[user.id] ? `${nameOf(user)} — ${statusTexts()[user.id]}` : nameOf(user)}
                onContextMenu={(event) => participantContextMenu(event, user.id)} onKeyDown={(event) => participantMenuKey(event, user.id)}>
                <Avatar name={nameOf(user)} id={user.id} size={32} src={user.avatar_url} status={statusClass(statusOf(user))} />
              </span>
            )}
          </For>
          <Show when={offlineCount() > 0}><span class="t3 fs12 tree-offline">{t('Offline — {count}', { count: offlineCount() })}</span></Show>
        </div>

        <div class="tree-crown">
          <For each={branches()}>
            {(branch) => (
              <div class="tree-branch">
                <div class="tree-nests"><For each={branch.nests}>{(channel) => <Nest channel={channel} actions={(target) => <ChannelActions channel={target} onDialog={setDialog} />} />}</For></div>
                <div class="tree-limb" aria-hidden="true" />
                <div class="tree-houses"><For each={branch.houses}>{(channel) => <Birdhouse channel={channel} onDialog={setDialog} />}</For></div>
              </div>
            )}
          </For>
        </div>
      </div>

      <Show when={activeChannel()}>
        <div class="tree-content">
          <Show when={activeChannel()?.channel_type === 'text'} fallback={<VoicePanel />}>
            <ChatArea />
          </Show>
        </div>
      </Show>

      <footer class="tree-perch">
        <button type="button" class="profile-open tree-me" aria-label={t('Open profile')} title={currentUser()?.display_name || currentUser()?.username} onClick={() => setProfileOpen(true)}>
          <Avatar name={currentUser()?.display_name || currentUser()?.username} id={currentUser()?.id} size={40} status={myStatus()} speaking={isLocalSpeaking()} src={currentUser()?.avatar_url} />
          <span class="col" style={{ 'min-width': 0, 'text-align': 'left' }}>
            <span class="b trunc fs13">{currentUser()?.display_name || currentUser()?.username}</span>
            <span class="t3 fs12 trunc">{(currentUser() && statusTexts()[currentUser()!.id]) || statusLabel()}</span>
          </span>
        </button>
        <Show when={activeVoiceChannel()} fallback={<span class="t3 fs12 grow trunc">{t('You are on the wire. Pick a nest to talk.')}</span>}>
          {(channel) => (
            <span class="col grow tree-voice" style={{ 'min-width': 0 }}>
              <span class="row g6 b fs13 tree-voice-state"><Icon name="bars" size="sm" />{connectionStatus() === 'connected' ? t('Voice connected') : t('Voice disconnected')}</span>
              <span class="t2 fs12 trunc">{t('In the nest: {channel}', { channel: channel().name })}{pingRtt() !== null ? ` · ${pingRtt()} ms` : ''}</span>
              <Show when={isSharing()}><span class="row g6 fs12" style={{ color: 'var(--red-text)' }}><Icon name="share" size="sm" />{t('You are sharing your screen')}</span></Show>
            </span>
          )}
        </Show>
        <Button iconOnly size="sm" variant={isMuted() ? 'danger' : 'ghost'} icon={isMuted() ? 'mic-off' : 'mic'} label={isMuted() ? t('Unmute') : t('Mute')} onClick={() => void toggleMute()} />
        <Button iconOnly size="sm" variant={isDeafened() ? 'danger' : 'ghost'} icon={isDeafened() ? 'phones-off' : 'phones'} label={isDeafened() ? t('Undeafen') : t('Deafen')} onClick={() => void toggleDeafen()} />
        <Show when={activeVoiceChannel()}>
          <Button iconOnly size="sm" variant="danger" icon="hangup" class="tree-leave" label={t('Fly out of the nest')} onClick={() => void leaveVoice()} />
        </Show>
        <Button iconOnly size="sm" variant="ghost" icon="sliders" label={t('Profile and settings')} onClick={() => setProfileOpen(true)} />
        <Show when={isStaff()}>
          <Button iconOnly size="sm" variant="ghost" icon="shield" label={t('Administration')} class="ty" onClick={() => setAdminOpen(true)} />
        </Show>
      </footer>

      <ChannelDialogs dialog={dialog()} onClose={() => setDialog(null)} />
    </div>
  );
}
