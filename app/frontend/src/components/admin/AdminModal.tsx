import { createEffect, createSignal, For, on, Show } from 'solid-js';
import {
  adminOpen, auditEntries, auditHasMore, changeRegistration, createInvite, describeAudit, invites, loadAudit, loadInvites, loadMembers, loadMoreAudit,
  loadOverview, makeBackup, members, moderate, overview, revokeInvite, setAdminOpen, type MemberAction,
} from '../../stores/adminStore';
import { currentUser } from '../../stores/authStore';
import { channels } from '../../stores/chatStore';
import { moveChannel, removeChannel, renameChannel } from '../../stores/channelAdminStore';
import type { AdminUser, Channel } from '../../types';
import { Avatar, Button, ConfirmDialog, Field, Icon, type IconName, Modal, pushToast, Toggle } from '../../ui';
import { formatNumber, locale, t } from '../../i18n';

type Tab = 'members' | 'invites' | 'channels' | 'server' | 'log';

const when = (seconds: number | null | undefined) => (seconds ? new Date(seconds * 1000).toLocaleString(locale() === 'ru' ? 'ru-RU' : 'en-GB') : t('never'));
const ago = (seconds: number | null | undefined, now = Date.now()) => {
  if (!seconds) return t('never');
  const minutes = Math.max(0, Math.round((now / 1000 - seconds) / 60));
  if (minutes < 1) return t('just now');
  if (minutes < 60) return t('{count} min ago', { count: minutes });
  if (minutes < 60 * 24) return t('{count} h ago', { count: Math.round(minutes / 60) });
  return t('{count} d ago', { count: Math.round(minutes / 1440) });
};
const bytes = (n: number | null | undefined) => (n == null ? '—' : n < 1024 * 1024 ? `${formatNumber(Math.max(1, Math.round(n / 1024)))} KB` : n < 1024 ** 3 ? `${formatNumber(n / 1024 / 1024, 1)} MB` : `${formatNumber(n / 1024 ** 3, 2)} GB`);
const uptime = (s: number) => (s < 3600 ? `${Math.floor(s / 60)} min` : s < 86400 ? `${Math.floor(s / 3600)} h` : `${Math.floor(s / 86400)} d ${Math.floor((s % 86400) / 3600)} h`);

const BAN_CHOICES: [string, number | undefined][] = [['1 hour', 3600], ['1 day', 86_400], ['1 week', 604_800], ['For good', undefined]];

type Pending = { member: AdminUser; action: MemberAction; seconds?: number; label: string } | null;

function MembersTab(props: { isAdmin: boolean }) {
  const [pending, setPending] = createSignal<Pending>(null);
  const [banFor, setBanFor] = createSignal<AdminUser | null>(null);
  const me = () => currentUser()?.id;
  const may = (member: AdminUser) => member.id !== me() && (props.isAdmin || member.role === 'member');
  const consequence = (p: NonNullable<Pending>) => {
    const name = p.member.display_name;
    const banTime = p.seconds ? t(BAN_CHOICES.find(([, s]) => s === p.seconds)?.[0] ?? 'a while') : t('until you lift the ban');
    const messages: Record<MemberAction, [string, Record<string, string>] > = {
      kick: ['{name} is disconnected and has to sign in again. They can come back.', { name }],
      ban: ['{name} is disconnected and cannot sign in for {duration}.', { name, duration: banTime }],
      unban: ['{name} can sign in again.', { name }],
      'make-admin': ['{name} gets every power an admin has, including banning and changing roles.', { name }],
      'make-moderator': ['{name} can kick people, silence them in voice and delete messages.', { name }],
      'make-member': ['{name} loses their staff powers.', { name }],
    };
    const [source, params] = messages[p.action];
    return t(source, params);
  };

  return (
    <>
      <div class="tbl-wrap">
        <table class="tbl">
          <thead><tr><th>{t('Member')}</th><th>{t('Role')}</th><th>{t('State')}</th><th>{t('Last seen')}</th><th /></tr></thead>
          <tbody>
            <For each={members()}>
              {(member) => (
                <tr>
                  <td>
                    <div class="row g12">
                      <Avatar name={member.display_name} id={member.id} size={32} />
                      <div class="col" style={{ 'min-width': 0 }}><strong class="fs13 trunc">{member.display_name}</strong><span class="t3 fs12">@{member.username}</span></div>
                    </div>
                  </td>
                  <td>
                    <Show when={props.isAdmin && member.id !== me()} fallback={<span class={`tag ${member.role === 'admin' ? 'admin' : member.role === 'moderator' ? 'violet' : ''}`}>{t(member.role === 'admin' ? 'Admin' : member.role === 'moderator' ? 'Moderator' : 'Member')}</span>}>
                      <div class="row g4">
                        <For each={[['member', 'Member'], ['moderator', 'Moderator'], ['admin', 'Admin']] as const}>
                          {([role, label]) => (
                            <button type="button" class={`pill role-pill${member.role === role ? ' on' : ''}`} aria-pressed={member.role === role} disabled={member.role === role}
                              onClick={() => setPending({ member, action: `make-${role}` as MemberAction, label })}>{t(label)}</button>
                          )}
                        </For>
                      </div>
                    </Show>
                  </td>
                  <td>
                    <Show when={member.banned} fallback={<span class={member.online ? 'tt' : 't3'}>{member.online ? t('Online') : t('Offline')}</span>}>
                      <span class="tag red">{member.banned_until ? t('Banned until {when}', { when: when(member.banned_until) }) : t('Banned')}</span>
                    </Show>
                  </td>
                  <td class="t2 fs13">{member.online ? t('now') : ago(member.last_seen)}</td>
                  <td>
                    <Show when={may(member)}>
                      <div class="row g8" style={{ 'justify-content': 'flex-end' }}>
                        <Show when={member.online && !member.banned}><Button size="sm" variant="secondary" onClick={() => setPending({ member, action: 'kick', label: 'Kick' })}>{t('Kick')}</Button></Show>
                        <Show when={props.isAdmin}>
                          <Show when={member.banned} fallback={<Button size="sm" variant="danger-ghost" onClick={() => setBanFor(member)}>{t('Ban…')}</Button>}>
                            <Button size="sm" variant="secondary" onClick={() => setPending({ member, action: 'unban', label: 'Unban' })}>{t('Unban')}</Button>
                          </Show>
                        </Show>
                      </div>
                    </Show>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>

      <Modal open={!!banFor()} onClose={() => setBanFor(null)} title={t('Ban {name}', { name: banFor()?.display_name ?? '' })} width="420px">
        <div class="col g12">
          <span class="t2 fs13">{t('For how long? Admins cannot be banned: make them a member first.')}</span>
          <div class="col g8">
            <For each={BAN_CHOICES}>
              {([label, seconds]) => (
                <Button variant={seconds ? 'secondary' : 'danger'} block onClick={() => { const member = banFor()!; setBanFor(null); setPending({ member, action: 'ban', seconds, label: 'Ban' }); }}>{t(label)}</Button>
              )}
            </For>
          </div>
          <Button variant="ghost" onClick={() => setBanFor(null)}>{t('Cancel')}</Button>
        </div>
      </Modal>

      <ConfirmDialog
        open={!!pending()}
        danger={pending()?.action === 'ban' || pending()?.action === 'kick' || pending()?.action === 'make-member'}
        title={pending() ? t('{action}: {name}?', { action: t(pending()!.label || 'Change role'), name: pending()!.member.display_name }) : ''}
        message={pending() ? consequence(pending()!) : ''}
        confirmLabel={t(pending()?.label || 'Confirm')}
        onCancel={() => setPending(null)}
        onConfirm={() => { const p = pending()!; setPending(null); void moderate(p.member.id, p.action, p.seconds); }}
      />
    </>
  );
}

function InvitesTab() {
  const [maxUses, setMaxUses] = createSignal('1');
  const [hours, setHours] = createSignal('72');
  const [fresh, setFresh] = createSignal<string | null>(null);
  const create = async () => setFresh(await createInvite(Math.max(1, Number(maxUses()) || 1), Math.max(1, Number(hours()) || 1)));
  const state = (i: { uses: number; max_uses: number; expires_at: number }) => (i.uses >= i.max_uses ? 'Used up' : i.expires_at * 1000 < Date.now() ? 'Expired' : 'Active');
  return (
    <div class="col g16">
      <span class="t2 fs13">{t('People need a code to register. Give it to them yourself; it stops working after its uses or time run out.')}</span>
      <div class="row g12" style={{ 'align-items': 'flex-end' }}>
        <div style={{ width: '120px' }}><Field label={t('Uses')} value={maxUses()} onInput={setMaxUses} /></div>
        <div style={{ width: '160px' }}><Field label={t('Valid for (hours)')} value={hours()} onInput={setHours} /></div>
        <Button variant="primary" icon="ticket" onClick={() => void create()}>{t('Create invitation')}</Button>
      </div>
      <Show when={fresh()}>
        {(code) => (
          <div class="invite-fresh" role="status">
            <span class="t2 fs12">{t('New code')}</span>
            <strong class="invite-code">{code()}</strong>
            <Button size="sm" variant="secondary" icon="copy" onClick={() => { void navigator.clipboard?.writeText(code()); pushToast({ kind: 'success', title: t('Code copied') }); }}>{t('Copy')}</Button>
          </div>
        )}
      </Show>
      <table class="tbl">
        <thead><tr><th>{t('Code')}</th><th>{t('Used')}</th><th>{t('Expires')}</th><th>{t('State')}</th><th /></tr></thead>
        <tbody>
          <For each={invites()} fallback={<tr><td colSpan={5} class="t3">{t('No invitations yet.')}</td></tr>}>
            {(invite) => (
              <tr>
                <td><code>{invite.code}</code></td>
                <td>{invite.uses} / {invite.max_uses}</td>
                <td class="fs13">{when(invite.expires_at)}</td>
                <td><span class={`tag ${state(invite) === 'Active' ? 'teal' : ''}`}>{t(state(invite))}</span></td>
                <td><Button size="sm" variant="ghost" onClick={() => void revokeInvite(invite.code)}>{t('Revoke')}</Button></td>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    </div>
  );
}

function ChannelsTab() {
  const [renaming, setRenaming] = createSignal<{ channel: Channel; name: string } | null>(null);
  const [deleting, setDeleting] = createSignal<Channel | null>(null);
  const textCount = () => channels().filter((c) => c.channel_type === 'text').length;
  const list = (type: 'text' | 'voice') => channels().filter((c) => c.channel_type === type);
  const Group = (props: { type: 'text' | 'voice'; title: string; icon: IconName }) => (
    <div class="col g8">
      <span class="sec-label">{props.title}</span>
      <For each={list(props.type)}>
        {(channel, index) => (
          <div class="setrow">
            <Icon name={props.icon} size="sm" class="t3" />
            <div class="col grow"><strong class="fs13">{channel.name}</strong><Show when={channel.topic}><span class="t3 fs12 trunc">{channel.topic}</span></Show></div>
            <Button iconOnly size="sm" variant="ghost" icon="up" label={t('Move {channel} up', { channel: channel.name })} disabled={index() === 0} onClick={() => void moveChannel(channel, -1)} />
            <Button iconOnly size="sm" variant="ghost" icon="down" label={t('Move {channel} down', { channel: channel.name })} disabled={index() === list(props.type).length - 1} onClick={() => void moveChannel(channel, 1)} />
            <Button iconOnly size="sm" variant="ghost" icon="edit" label={t('Rename {channel}', { channel: channel.name })} onClick={() => setRenaming({ channel, name: channel.name })} />
            <Button iconOnly size="sm" variant="ghost" icon="trash" label={t('Delete {channel}', { channel: channel.name })} disabled={props.type === 'text' && textCount() <= 1} title={props.type === 'text' && textCount() <= 1 ? t('The last text channel cannot be deleted') : undefined} onClick={() => setDeleting(channel)} />
          </div>
        )}
      </For>
    </div>
  );
  return (
    <div class="col g20">
      <Group type="text" title={t('Text channels')} icon="hash" />
      <Group type="voice" title={t('Voice channels')} icon="vol" />
      <span class="t3 fs12">{t('Create channels with the + next to the channel lists.')}</span>
      <Modal open={!!renaming()} onClose={() => setRenaming(null)} title={t('Rename channel')} width="420px">
        <form class="col g16" onSubmit={(e) => { e.preventDefault(); const r = renaming()!; setRenaming(null); void renameChannel(r.channel, r.name); }}>
          <Field label={t('Channel name')} value={renaming()?.name ?? ''} onInput={(name) => setRenaming({ ...renaming()!, name })} autofocus />
          <div class="row g8" style={{ 'justify-content': 'flex-end' }}><Button variant="secondary" onClick={() => setRenaming(null)}>{t('Cancel')}</Button><Button type="submit" variant="primary">{t('Save')}</Button></div>
        </form>
      </Modal>
      <ConfirmDialog open={!!deleting()} danger title={`Delete ${deleting()?.name ?? ''}?`}
        message={t('All messages and files in this channel will be removed. People in a deleted voice channel are sent out.')} confirmLabel={t('Delete channel')}
        onCancel={() => setDeleting(null)} onConfirm={() => { const c = deleting()!; setDeleting(null); void removeChannel(c); }} />
    </div>
  );
}

function ServerTab() {
  const [busy, setBusy] = createSignal(false);
  const o = () => overview();
  return (
    <Show when={o()} fallback={<span class="skel pulse" style={{ height: '120px' }} />}>
      {(info) => (
        <div class="col g16">
          <div class="grid" style={{ 'grid-template-columns': 'repeat(2, minmax(0, 1fr))', gap: '12px' }}>
            <div class="card stat"><span class="t3 fs12">{t('Version')}</span><strong>{info().version}</strong></div>
            <div class="card stat"><span class="t3 fs12">{t('Running for')}</span><strong>{uptime(info().uptime_secs)}</strong></div>
            <div class="card stat"><span class="t3 fs12">{t('Ports')}</span><strong>QUIC {info().quic_port} · HTTPS {info().http_port}</strong></div>
            <div class="card stat"><span class="t3 fs12">{t('Uploaded files')}</span><strong>{bytes(info().uploads_bytes)}</strong></div>
          </div>
          <div class="col g4">
            <span class="field-label">{t('Certificate fingerprint')}</span>
            <div class="fp-grid plain" style={{ 'font-size': '12px' }}>{info().certificate_fingerprint ?? 'unknown'}</div>
            <span class="t3 fs12">{t('Share this with people so they can compare it when their app asks them to trust the server.')}</span>
            <Show when={info().certificate_fingerprint}><Button size="sm" variant="ghost" icon="copy" onClick={() => { void navigator.clipboard?.writeText(info().certificate_fingerprint!); pushToast({ kind: 'success', title: t('Fingerprint copied') }); }}>{t('Copy fingerprint')}</Button></Show>
          </div>
          <div class="setrow">
            <div class="col grow"><strong class="fs13">{t('Anyone can register')}</strong><span class="t3 fs12">{t('Off: a registration needs an invitation code. Takes effect at once.')}</span></div>
            <Toggle label={t('Anyone can register')} checked={info().registration === 'open'} onChange={(open) => void changeRegistration(open ? 'open' : 'invite')} />
          </div>
          <div class="col g8">
            <span class="field-label">{t('Backups')}</span>
            <span class="t2 fs13">{t('{count} kept · last: {when} ({size})', { count: info().backups_count, when: when(info().last_backup_at), size: bytes(info().last_backup_bytes) })}</span>
            <span class="t3 fs12 trunc">{t('Folder: {backup} · Logs: {logs}', { backup: info().backup_dir, logs: info().log_dir })}</span>
            <div><Button variant="secondary" icon="db" loading={busy()} onClick={async () => { setBusy(true); if (await makeBackup()) pushToast({ kind: 'success', title: t('Backup made') }); setBusy(false); }}>{t('Back up now')}</Button></div>
          </div>
        </div>
      )}
    </Show>
  );
}

function LogTab() {
  return (
    <div class="col g8">
      <For each={auditEntries()} fallback={<span class="t3">{t('Nothing has been logged yet.')}</span>}>
        {(entry) => (
          <div class="setrow">
            <div class="col grow"><span class="fs13"><strong>{entry.actor_name}</strong> {describeAudit(entry)}</span></div>
            <span class="t3 fs12" title={when(entry.at)}>{ago(entry.at)}</span>
          </div>
        )}
      </For>
      <Show when={auditHasMore()}><Button variant="secondary" onClick={() => void loadMoreAudit()}>{t('Show older')}</Button></Show>
    </div>
  );
}

/** H1-H4 and the action log in one window; moderators see the members and the log. */
export function AdminModal() {
  const [tab, setTab] = createSignal<Tab>('members');
  const isAdmin = () => currentUser()?.role === 'admin';
  const tabs = (): { id: Tab; label: string; icon: IconName }[] => [
    { id: 'members', label: 'Members', icon: 'users' },
    ...(isAdmin() ? [{ id: 'invites' as const, label: 'Invitations', icon: 'ticket' as const }, { id: 'channels' as const, label: 'Channels', icon: 'hash' as const }, { id: 'server' as const, label: 'Server', icon: 'db' as const }] : []),
    { id: 'log', label: 'Log', icon: 'clock' },
  ];
  // Load what a tab shows when it is opened.
  createEffect(on([adminOpen, tab], ([open, current]) => {
    if (!open) return;
    if (current === 'members') void loadMembers();
    if (current === 'invites') void loadInvites();
    if (current === 'server') void loadOverview();
    if (current === 'log') void loadAudit();
  }));
  return (
    <Modal open={adminOpen()} onClose={() => setAdminOpen(false)} title={t('Administration')} width="940px">
      <div class="settings">
        <nav class="setnav" aria-label={t('Administration sections')}>
          <For each={tabs()}>
            {(item) => <button type="button" class={`si${tab() === item.id ? ' on' : ''}`} aria-current={tab() === item.id ? 'page' : undefined} onClick={() => setTab(item.id)}><Icon name={item.icon} size="sm" />{t(item.label)}</button>}
          </For>
        </nav>
        <div class="settings-page">
          <Show when={tab() === 'members'}><MembersTab isAdmin={isAdmin()} /></Show>
          <Show when={tab() === 'invites' && isAdmin()}><InvitesTab /></Show>
          <Show when={tab() === 'channels' && isAdmin()}><ChannelsTab /></Show>
          <Show when={tab() === 'server' && isAdmin()}><ServerTab /></Show>
          <Show when={tab() === 'log'}><LogTab /></Show>
        </div>
      </div>
    </Modal>
  );
}
