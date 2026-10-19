import { For, Index, Show } from 'solid-js';
import { knownUsers, presence, statusTexts } from '../stores/sessionStore';
import { Avatar } from '../ui';
import type { User, UserStatus } from '../types';
import { locale, t } from '../i18n';

const statusClass = (status: UserStatus) => (status === 'online' ? 'on' : status === 'idle' ? 'idle' : status === 'donotdisturb' ? 'dnd' : 'off');
const statusOf = (user: User): UserStatus => presence()[user.id] ?? user.status ?? 'offline';

const byName = (a: User, b: User) => (a.display_name || a.username).localeCompare(b.display_name || b.username, locale());

/** Members grouped as admins, moderators and the rest of those online, then everyone offline. Empty groups are left out. */
export function MembersPanel() {
  const groups = () => {
    const online = knownUsers().filter((user) => statusOf(user) !== 'offline');
    const roleIs = (role: User['role']) => online.filter((user) => user.role === role);
    const list: { title: string; users: User[] }[] = [
      { title: 'Admins — {count}', users: roleIs('admin') },
      { title: 'Moderators — {count}', users: roleIs('moderator') },
      { title: 'Online — {count}', users: online.filter((user) => user.role !== 'admin' && user.role !== 'moderator') },
      { title: 'Offline — {count}', users: knownUsers().filter((user) => statusOf(user) === 'offline') },
    ];
    return list.filter((group) => group.users.length > 0).map((group) => ({ ...group, users: [...group.users].sort(byName) }));
  };

  const Row = (props: { user: User }) => (
    <div class={`mem${statusOf(props.user) === 'offline' ? ' off' : ''}`}>
      <Avatar name={props.user.display_name || props.user.username} id={props.user.id} size={32} src={props.user.avatar_url} status={statusClass(statusOf(props.user))} />
      <div class="col grow" style={{ 'min-width': 0 }}>
        <span class="trunc">{props.user.display_name || props.user.username}</span>
        <Show when={statusTexts()[props.user.id]}><span class="t3 fs12 trunc">{statusTexts()[props.user.id]}</span></Show>
      </div>
    </div>
  );

  return (
    <aside class="members" aria-label={t('Members')}>
      {/* Index, not For: the group objects are rebuilt on every change, but the user objects inside are not, so rows survive. */}
      <Index each={groups()}>
        {(group, index) => (
          <>
            <span class="sec-label" style={{ padding: index === 0 ? '0 8px 8px' : '16px 8px 8px' }}>{t(group().title, { count: group().users.length })}</span>
            <For each={group().users}>{(user) => <Row user={user} />}</For>
          </>
        )}
      </Index>
    </aside>
  );
}
