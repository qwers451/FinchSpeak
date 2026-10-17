import { createMemo, createSignal, For, onMount, Show } from 'solid-js';
import { api } from '../../services/api';
import { currentUser, logoutUser } from '../../stores/authStore';
import { channels } from '../../stores/chatStore';
import {
  deleteAvatar, profileMessage, profileOpen, saveDisplayName, savePassword, saveStatus, setProfileMessage, setProfileOpen, STATUS_TEXT_MAX,
} from '../../stores/profileStore';
import { presence, statusTexts } from '../../stores/sessionStore';
import {
  isChannelMuted, muteChannel, prefs, unmuteChannel, updateAppearance, updateNotifications,
} from '../../stores/settingsStore';
import { choosePushToTalkKey, pushToTalkKey, shortcutFromEvent } from '../../stores/voiceStore';
import type { UserStatus } from '../../types';
import { Avatar, Button, Field, Icon, type IconName, Modal, pushToast, Slider, Toggle } from '../../ui';
import { passwordStrength } from '../../ui/passwordStrength';
import { AvatarCropDialog, checkAvatarFile } from './AvatarCropDialog';
import { t } from '../../i18n';
import { VoiceSettingsModal } from '../voice/VoiceSettingsModal';

export const APP_VERSION = '0.1.0';

type Page = 'profile' | 'notifications' | 'appearance' | 'keys' | 'voice';
const PAGES: { id: Page; label: string; icon: IconName }[] = [
  { id: 'profile', label: 'Profile', icon: 'user' },
  { id: 'notifications', label: 'Notifications', icon: 'bell' },
  { id: 'appearance', label: 'Appearance', icon: 'sliders' },
  { id: 'voice', label: 'Voice and audio', icon: 'vol' },
  { id: 'keys', label: 'Keys and about', icon: 'key' },
];

const STATUSES: { id: UserStatus; label: string; dot: string }[] = [
  { id: 'online', label: 'Online', dot: 'on' },
  { id: 'idle', label: 'Idle', dot: 'idle' },
  { id: 'donotdisturb', label: 'Do not disturb', dot: 'dnd' },
];

function Row(props: { title: string; hint?: string; control: unknown }) {
  return (
    <div class="setrow">
      <div class="col grow"><strong class="fs13">{props.title}</strong><Show when={props.hint}><span class="t3 fs12">{props.hint}</span></Show></div>
      {props.control as never}
    </div>
  );
}

function ProfilePage() {
  const me = () => currentUser();
  const [name, setName] = createSignal(me()?.display_name ?? '');
  const [file, setFile] = createSignal<File | null>(null);
  const [fileError, setFileError] = createSignal<string | null>(null);
  const [current, setCurrent] = createSignal('');
  const [next, setNext] = createSignal('');
  const [again, setAgain] = createSignal('');
  const [text, setText] = createSignal(me()?.id ? statusTexts()[me()!.id] ?? '' : '');
  let picker: HTMLInputElement | undefined;

  const myStatus = (): UserStatus => {
    const id = me()?.id;
    const s = id ? presence()[id] ?? me()?.status : undefined;
    return s && s !== 'offline' ? s : 'online';
  };
  const strength = () => passwordStrength(next());
  const mismatch = () => (again() && next() !== again() ? t('The passwords do not match') : undefined);

  const choose = (event: Event) => {
    const input = event.currentTarget as HTMLInputElement;
    const picked = input.files?.[0];
    input.value = '';
    if (!picked) return;
    const problem = checkAvatarFile(picked);
    setFileError(problem);
    if (!problem) setFile(picked);
  };

  return (
    <div class="col g24">
      <div class="row g20">
        <Avatar name={me()?.display_name} id={me()?.id} src={me()?.avatar_url} size={96} status={myStatus() === 'idle' ? 'idle' : myStatus() === 'donotdisturb' ? 'dnd' : 'on'} />
        <div class="col g8">
          <div class="row g8">
            <input ref={picker} type="file" accept="image/png,image/jpeg,image/webp" hidden onChange={choose} />
          <Button variant="secondary" icon="image" onClick={() => picker?.click()}>{me()?.avatar_url ? t('Change picture') : t('Upload picture')}</Button>
          <Show when={me()?.avatar_url}><Button variant="ghost" icon="trash" onClick={() => void deleteAvatar()}>{t('Remove')}</Button></Show>
          </div>
          <span class="t3 fs12">{t('PNG, JPEG or WebP, up to 12 MB. You choose which part people see.')}</span>
          <Show when={fileError()}><span class="err"><Icon name="warn" size="sm" />{t(fileError()!)}</span></Show>
        </div>
      </div>
      <Show when={file()}>{(picked) => <AvatarCropDialog file={picked()} onClose={() => setFile(null)} onDone={() => { setFile(null); setProfileMessage('Picture changed.'); }} />}</Show>

      <Show when={profileMessage()}><div class="auth-banner info" role="status"><Icon name="check" class="tc" /><span class="fs13">{t(profileMessage()!)}</span></div></Show>

      <form class="col g12" onSubmit={async (e) => { e.preventDefault(); await saveDisplayName(name()); }}>
        <Field label={t('Display name')} value={name()} onInput={setName} help={t('Shown to everyone. Up to 32 characters.')} />
        <div class="row g12">
          <span class="t3 fs12 grow">{t('Username')} <strong class="t2">@{me()?.username}</strong> · {t('cannot be changed')}</span>
          <Show when={me()?.role === 'admin'}><span class="tag admin">{t('Admin')}</span></Show>
          <Button type="submit" size="sm" variant="primary" disabled={!name().trim() || name().trim() === me()?.display_name}>{t('Save name')}</Button>
        </div>
      </form>

      <div class="col g12">
        <span class="field-label">{t('Status')}</span>
        <div class="seg" role="radiogroup" aria-label={t('Status')}>
          <For each={STATUSES}>
            {(option) => (
            <span role="radio" tabindex="0" aria-label={t(option.label)} aria-checked={myStatus() === option.id} class={myStatus() === option.id ? 'on' : ''}
                onClick={() => void saveStatus(option.id, text())} onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && void saveStatus(option.id, text())}>
                <i class={`dot ${option.dot}`} style={{ 'margin-right': '8px' }} />{t(option.label)}
              </span>
            )}
          </For>
        </div>
        <form class="row g8" onSubmit={(e) => { e.preventDefault(); void saveStatus(myStatus(), text()); }}>
          <div class="grow"><Field label={t('Status text')} value={text()} onInput={(value) => setText(value.slice(0, STATUS_TEXT_MAX))} placeholder={t('In a meeting, back at 5')} /></div>
          <Button type="submit" size="sm" variant="secondary">{t('Set')}</Button>
        </form>
      </div>

      <form class="col g12" onSubmit={async (e) => { e.preventDefault(); if (await savePassword(current(), next(), again())) { setCurrent(''); setNext(''); setAgain(''); } }}>
        <span class="field-label">{t('Change password')}</span>
        <Field label={t('Current password')} type="password" value={current()} onInput={setCurrent} autocomplete="current-password" />
        <Field label={t('New password')} type="password" value={next()} onInput={setNext} autocomplete="new-password" />
        <Show when={next()}>
          <div class="row g8">{[1, 2, 3, 4].map((step) => <div class={`strength-seg${strength().score >= step ? ' on' : ''}`} />)}<span class="fs12 ty b fit">{strength().label}</span></div>
        </Show>
        <Field label={t('Repeat the new password')} type="password" value={again()} onInput={setAgain} autocomplete="new-password" error={mismatch()} />
        <span class="t3 fs12">{t('Changing the password signs you out on your other devices.')}</span>
        <div class="row" style={{ 'justify-content': 'flex-end' }}><Button type="submit" variant="secondary" disabled={!current() || !next() || next() !== again()}>{t('Change password')}</Button></div>
      </form>

      <div class="row g12 between">
        <span class="t3 fs12">{t('Signed in as')} <strong class="t2">{me()?.username}</strong></span>
        <Button variant="danger-ghost" icon="logout" onClick={() => { setProfileOpen(false); void logoutUser(); }}>{t('Log out')}</Button>
      </div>
    </div>
  );
}

function NotificationsPage() {
  const n = () => prefs().notifications;
  const textChannels = () => channels().filter((channel) => channel.channel_type === 'text');
  return (
    <div class="col g8">
      <Row title={t('Mentions')} hint={t('A notification when someone mentions you, even if the window is in the background.')} control={<Toggle label={t('Mentions')} checked={n().mentions} onChange={(v) => updateNotifications({ mentions: v })} />} />
      <Row title={t('Every message')} hint={t('Also for channels you are not looking at.')} control={<Toggle label={t('Every message')} checked={n().allMessages} onChange={(v) => updateNotifications({ allMessages: v })} />} />
      <Row title={t('Someone joins my voice room')} control={<Toggle label={t('Someone joins my voice room')} checked={n().voiceJoin} onChange={(v) => updateNotifications({ voiceJoin: v })} />} />
      <Row title={t('Sounds')} hint={t('A short tone for mentions and for people joining.')} control={<Toggle label={t('Sounds')} checked={n().sounds} onChange={(v) => updateNotifications({ sounds: v })} />} />
      <div class="setrow">
        <div class="col grow"><strong class="fs13">{t('Sound volume')}</strong></div>
        <div style={{ width: '180px' }}><Slider label={t('Sound volume')} min={0} max={1} step={0.05} value={n().soundVolume} onChange={(v) => updateNotifications({ soundVolume: v })} /></div>
      </div>
      <Row title={t('Flash the taskbar')} hint={t('While the window is in the background.')} control={<Toggle label={t('Flash the taskbar')} checked={n().flash} onChange={(v) => updateNotifications({ flash: v })} />} />
      <span class="field-label" style={{ 'margin-top': '16px' }}>{t('Muted channels')}</span>
      <For each={textChannels()}>
        {(channel) => (
          <div class="setrow">
            <div class="col grow"><strong class="fs13">#{channel.name}</strong><Show when={isChannelMuted(channel.id)}><span class="t3 fs12">{t('Muted')}</span></Show></div>
            <Show when={isChannelMuted(channel.id)} fallback={
              <div class="row g8">
                <Button size="sm" variant="secondary" onClick={() => muteChannel(channel.id, 60 * 60 * 1000)}>{t('1 hour')}</Button>
                <Button size="sm" variant="secondary" onClick={() => muteChannel(channel.id, 8 * 60 * 60 * 1000)}>{t('8 hours')}</Button>
                <Button size="sm" variant="secondary" onClick={() => muteChannel(channel.id, null)}>{t('Until I turn it on')}</Button>
              </div>
            }>
              <Button size="sm" variant="primary" onClick={() => unmuteChannel(channel.id)}>{t('Turn on')}</Button>
            </Show>
          </div>
        )}
      </For>
    </div>
  );
}

function AppearancePage() {
  const a = () => prefs().appearance;
  const seg = <T extends string>(label: string, value: T, options: [T, string][], set: (v: T) => void) => (
    <div class="seg" role="radiogroup" aria-label={label}>
      <For each={options}>{([id, text]) => <span role="radio" tabindex="0" aria-checked={value === id} class={value === id ? 'on' : ''} onClick={() => set(id)} onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && set(id)}>{text}</span>}</For>
    </div>
  );
  return (
    <div class="col g8">
      <Row title={t('Theme')} control={seg(t('Theme'), a().theme, [['claude', 'Claude Dark'], ['custom', 'Finch Custom']], (theme) => updateAppearance({ theme }))} />
      <Row title={t('Language')} control={seg(t('Language'), a().language, [['ru', 'Русский'], ['en', 'English']], (language) => updateAppearance({ language }))} />
      <Row title={t('Density')} hint={t('Compact fits more messages on the screen.')} control={seg(t('Density'), a().density, [['comfortable', t('Comfortable')], ['compact', t('Compact')]], (density) => updateAppearance({ density }))} />
      <Row title={t('Text size')} control={seg(t('Text size'), a().textSize, [['small', t('Small')], ['medium', t('Medium')], ['large', t('Large')]], (textSize) => updateAppearance({ textSize }))} />
      <Row title={t('Reduce motion')} hint={t('Turns off pulsing rings and slides.')} control={<Toggle label={t('Reduce motion')} checked={a().reduceMotion} onChange={(v) => updateAppearance({ reduceMotion: v })} />} />
      <Row title={t('Close button hides to the tray')} hint={t('FinchSpeak keeps running (and keeps you in the call). Quit from the tray menu.')} control={<Toggle label={t('Close button hides to the tray')} checked={a().closeToTray} onChange={(v) => updateAppearance({ closeToTray: v })} />} />
      <Row title={t('Members panel')} hint={t('The list of people on the right.')} control={<Toggle label={t('Members panel')} checked={a().showMembers} onChange={(v) => updateAppearance({ showMembers: v })} />} />
    </div>
  );
}

function KeysPage() {
  const [capturing, setCapturing] = createSignal(false);
  const [info, setInfo] = createSignal<{ server_url: string | null; hostname: string | null; fingerprint: string | null }>({ server_url: null, hostname: null, fingerprint: null });
  const [version, setVersion] = createSignal(APP_VERSION);
  const [update, setUpdate] = createSignal<{ version: string; notes: string } | null>(null);
  const [checking, setChecking] = createSignal(false);
  const [upToDate, setUpToDate] = createSignal(false);
  onMount(async () => {
    try { setInfo(await api.serverInfo()); } catch { /* shown as unknown */ }
    try { setVersion(await api.appVersion()); } catch { /* keep the built-in text */ }
  });
  const checkForUpdate = async () => {
    setChecking(true);
    setUpToDate(false);
    try {
      const found = await api.checkUpdate();
      setUpdate(found);
      setUpToDate(!found);
    } catch (err) {
      pushToast({ kind: 'error', title: t('Could not check for updates'), text: String(err).replace(/^Error:\s*/, '') });
    } finally {
      setChecking(false);
    }
  };
  const install = async () => {
    try { await api.installUpdate(); } catch (err) { pushToast({ kind: 'error', title: t('The update was not installed'), text: String(err).replace(/^Error:\s*/, '') }); }
  };

  const capture = (event: KeyboardEvent) => {
    event.preventDefault();
    if (event.code === 'Escape') { setCapturing(false); return; }
    const shortcut = shortcutFromEvent(event);
    if (!shortcut) return;
    setCapturing(false);
    void choosePushToTalkKey(shortcut);
  };
  const diagnostics = createMemo(() => [
    `FinchSpeak ${version()}`,
    `Server: ${info().server_url ?? 'unknown'}`,
    `Certificate: ${info().fingerprint ?? 'unknown'}`,
    `Agent: ${navigator.userAgent}`,
  ].join('\n'));
  const copy = (text: string, what: 'Fingerprint' | 'Diagnostics') => { void navigator.clipboard?.writeText(text); pushToast({ kind: 'success', title: t(what === 'Fingerprint' ? 'Fingerprint copied' : 'Diagnostics copied') }); };

  return (
    <div class="col g16">
      <span class="field-label">{t('Keys')}</span>
      <div class="setrow">
        <div class="col grow"><strong class="fs13">{t('Push to talk')}</strong><span class="t3 fs12">{t('Held to speak; works in the background.')}</span></div>
        <button type="button" class={`btn secondary${capturing() ? ' is-focus' : ''}`} aria-label={t('Push-to-talk key')} onClick={() => setCapturing(true)} onKeyDown={(e) => capturing() && capture(e)} onBlur={() => setCapturing(false)}>
          <Icon name="key" size="sm" />{capturing() ? t('Press a key… (Esc cancels)') : pushToTalkKey() ?? t('Choose key')}
        </button>
        <Show when={pushToTalkKey()}><Button iconOnly size="sm" variant="ghost" icon="x" label={t('Remove the key')} onClick={() => void choosePushToTalkKey(null)} /></Show>
      </div>
      <div class="col g8">
        <For each={[['Enter', 'Send the message'], ['Shift + Enter', 'New line'], ['Esc', 'Cancel an edit, a reply or a dialog'], ['@', 'Mention someone']]}>
          {([key, what]) => <div class="row g12"><span class="kbd">{key}</span><span class="t2 fs13">{t(what)}</span></div>}
        </For>
      </div>

      <span class="field-label" style={{ 'margin-top': '8px' }}>{t('About')}</span>
      <div class="col g8">
        <div class="row g12 between"><span class="t2">{t('Version')}</span><strong>{version()}</strong></div>
        <div class="row g12 between">
          <span class="t2">{t('Updates')}</span>
          <Show when={update()} fallback={<Button size="sm" variant="secondary" icon="refresh" loading={checking()} onClick={() => void checkForUpdate()}>{upToDate() ? t('You have the newest version') : t('Check for updates')}</Button>}>
            {(found) => <Button size="sm" variant="primary" icon="dl" onClick={() => void install()}>{t('Install {version}', { version: found().version })}</Button>}
          </Show>
        </div>
        <Show when={update()?.notes}><span class="t3 fs12">{update()!.notes}</span></Show>
        <div class="row g12 between"><span class="t2">{t('Server')}</span><strong class="trunc">{info().server_url ?? t('unknown')}</strong></div>
        <div class="col g4">
          <span class="t2">{t('Certificate fingerprint (SHA-256)')}</span>
          <div class="fp-grid plain" style={{ 'font-size': '12px' }}>{info().fingerprint ?? t('unknown')}</div>
          <Show when={info().fingerprint}><Button size="sm" variant="ghost" icon="copy" onClick={() => copy(info().fingerprint!, 'Fingerprint')}>{t('Copy fingerprint')}</Button></Show>
        </div>
        <Button variant="secondary" icon="copy" onClick={() => copy(diagnostics(), 'Diagnostics')}>{t('Copy diagnostics')}</Button>
      </div>
    </div>
  );
}

/** F1-F4 in one window: profile, notifications, appearance, keys and about. */
export function SettingsModal() {
  const [page, setPage] = createSignal<Page>('profile');
  const [voiceOpen, setVoiceOpen] = createSignal(false);
  const [returnPage, setReturnPage] = createSignal<Page>('profile');
  return (
    <>
    <Modal open={profileOpen()} onClose={() => { setProfileOpen(false); setProfileMessage(null); }} title={t('Settings')} width="860px">
      <div class="settings">
        <nav class="setnav" aria-label={t('Settings sections')}>
          <For each={PAGES}>
            {(item) => (
              <button type="button" class={`si${page() === item.id ? ' on' : ''}`} aria-current={page() === item.id ? 'page' : undefined} onClick={() => {
                if (item.id === 'voice') { setReturnPage(page()); setPage('voice'); setProfileOpen(false); setVoiceOpen(true); }
                else setPage(item.id);
              }}>
                <Icon name={item.icon} size="sm" />{t(item.label)}
              </button>
            )}
          </For>
        </nav>
        <div class="settings-page">
          <Show when={page() === 'profile'}><ProfilePage /></Show>
          <Show when={page() === 'notifications'}><NotificationsPage /></Show>
          <Show when={page() === 'appearance'}><AppearancePage /></Show>
          <Show when={page() === 'keys'}><KeysPage /></Show>
        </div>
      </div>
    </Modal>
    <VoiceSettingsModal open={voiceOpen()} onClose={() => { setVoiceOpen(false); setPage(returnPage()); setProfileOpen(true); }} />
    </>
  );
}
