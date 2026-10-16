import { For, Show } from 'solid-js';
import { knownUsers, presence, statusTexts } from '../stores/sessionStore';
import { Avatar } from '../ui';
import type { User, UserStatus } from '../types';

const statusClass = (status: UserStatus) => (status === 'online' ? 'on' : status === 'idle' ? 'idle' : status === 'donotdisturb' ? 'dnd' : 'off');
const statusOf = (user: User): UserStatus => presence()[user.id] ?? user.status ?? 'offline';

/** Online / offline members (right side). */
export function MembersPanel() {
  const online = () => knownUsers().filter((user) => statusOf(user) !== 'offline');
  const offline = () => knownUsers().filter((user) => statusOf(user) === 'offline');

  const Row = (props: { user: User }) => (
    <div class={`mem${statusOf(props.user) === 'offline' ? ' off' : ''}`}>
      <Avatar name={props.user.display_name || props.user.username} id={props.user.id} size={32} src={props.user.avatar_url} status={statusClass(statusOf(props.user))} />
      <div class="col grow" style={{ 'min-width': 0 }}>
        <span class="trunc">{props.user.display_name || props.user.username}</span>
        <Show when={statusTexts()[props.user.id]}><span class="t3 fs12 trunc">{statusTexts()[props.user.id]}</span></Show>
      </div>
      <Show when={props.user.role === 'admin'}><span class="tag admin">Admin</span></Show>
    </div>
  );

  return (
    <aside class="members" aria-label="Members">
      <span class="sec-label" style={{ padding: '0 8px 8px' }}>Online — {online().length}</span>
      <For each={online()}>{(user) => <Row user={user} />}</For>
      <Show when={offline().length > 0}>
        <span class="sec-label" style={{ padding: '16px 8px 8px' }}>Offline — {offline().length}</span>
        <For each={offline()}>{(user) => <Row user={user} />}</For>
      </Show>
    </aside>
  );
}
