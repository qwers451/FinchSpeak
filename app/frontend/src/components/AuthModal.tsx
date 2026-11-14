import { FinchBird } from '../ui/FinchBird';
import { createEffect, createSignal, on, Show } from 'solid-js';
import {
  serverUrl,
  setServerUrl,
  rememberedUsername,
  loginUser,
  registerUser,
  resumeLastSession,
  resumeFailed,
  cancelResumeRetry,
  authLoading,
  authError,
  setAuthError,
} from '../stores/authStore';
import { linkServerDiffers, pendingInvite, setPendingInvite } from '../stores/inviteLink';
import { Button, Field, Icon, Toggle } from '../ui';
import { passwordStrength } from '../ui/passwordStrength';
import { t } from '../i18n';
import { describeError } from '../i18n/errors';

type ErrorLook = { icon: 'warn' | 'ban'; tone: 'red' | 'amber'; title: string; hint?: string; retry: boolean; localized: boolean };

/** Turns the error text from the backend into the plain-words banner of the design (A2). */
export function describeAuthError(raw: string): ErrorLook {
  const text = raw.replace(/^Error:\s*/, '');
  const lower = text.toLowerCase();
  if (lower.includes('banned')) {
    return { icon: 'ban', tone: 'red', title: 'This account has been banned from the server', hint: 'Contact the server admin if you think this is a mistake.', retry: false, localized: true };
  }
  // Checked before the 401 test, so a wait such as "Try again in 401 seconds" is not read as a wrong password.
  if (/Too many attempts\. Try again in \d+ seconds/.test(text)) {
    return { icon: 'warn', tone: 'amber', title: describeError(text), retry: false, localized: false };
  }
  if (lower.includes('invalid username or password') || lower.includes('401')) {
    return { icon: 'warn', tone: 'red', title: 'Invalid username or password', hint: 'Check them and try again. Passwords are case-sensitive.', retry: false, localized: true };
  }
  if (lower.includes('cannot reach') || lower.includes('connection refused') || lower.includes('timed out')) {
    return { icon: 'warn', tone: 'amber', title: 'Cannot reach the server', hint: 'Check the address, your network and that the server is running.', retry: true, localized: true };
  }
  return { icon: 'warn', tone: 'red', title: describeError(text), retry: false, localized: false };
}

export function AuthModal() {
  // An invite link opens the register form with its server and code filled in; the user still types the rest and submits.
  const fromLink = pendingInvite();
  if (fromLink) setServerUrl(fromLink.server);
  const [isRegister, setIsRegister] = createSignal(!!fromLink);
  const [username, setUsername] = createSignal(rememberedUsername());
  const [displayName, setDisplayName] = createSignal('');
  const [password, setPassword] = createSignal('');
  const [inviteCode, setInviteCode] = createSignal(fromLink?.code ?? '');
  const [stay, setStay] = createSignal(true);
  const [serverError, setServerError] = createSignal<string | undefined>(undefined);
  // Typing a value means the user signs in by hand: scheduled automatic resumes stop, Retry uses the form.
  const typed = (set: (value: string) => void) => (value: string) => {
    cancelResumeRetry();
    set(value);
  };

  createEffect(on(pendingInvite, (link) => {
    if (!link) return;
    setServerUrl(link.server);
    setInviteCode(link.code);
    setIsRegister(true);
    setServerError(undefined);
    setAuthError(null);
  }, { defer: true }));

  const strength = () => passwordStrength(password());
  const inviteError = () => (authError() ?? '').toLowerCase().includes('invite') ? t('A valid invite code is required to register') : undefined;
  const usernameError = () => (authError() ?? '').toLowerCase().includes('taken') || (authError() ?? '').toLowerCase().includes('already exists') ? t('Username is already taken') : undefined;
  const banner = () => {
    const raw = authError();
    if (!raw || inviteError() || usernameError()) return null;
    return describeAuthError(raw);
  };

  const submit = async (event: Event) => {
    event.preventDefault();
    if (authLoading()) return;
    if (!serverUrl().trim()) {
      setServerError(t('Enter the server address'));
      return;
    }
    if (isRegister()) await registerUser(username().trim(), displayName().trim(), password(), inviteCode(), stay());
    else await loginUser(username().trim(), password(), stay());
  };

  const switchMode = () => {
    setIsRegister(!isRegister());
    setAuthError(null);
  };

  return (
    <div class="app auth">
      <div class="col center auth-brand">
        <div class="auth-disc auth-disc-a" />
        <div class="auth-disc auth-disc-b" />
        <FinchBird class="auth-mascot" size={460} label={t('FinchSpeak bird with headphones')} />
        <div class="col g8" style={{ 'text-align': 'center', position: 'relative' }}>
          <h1 style={{ 'font-size': '40px', 'line-height': '44px' }}>{t('Talk. Share.')} <span class="ty">{t('Chirp.')}</span></h1>
          <p class="t2 fs16" style={{ 'max-width': '400px' }}>{t('Your own voice rooms, chat and screen sharing, on a server you control.')}</p>
        </div>
      </div>
      <div class="col center grow auth-form-side">
        <form class="col g24 auth-form" onSubmit={submit} noValidate>
          <div class="col g8">
            <h2 style={{ 'font-size': '32px', 'line-height': '38px' }}>{isRegister() ? t('Join the flock') : t('Welcome back')}</h2>
            <span class="t2">{isRegister() ? t('Create your account on this server.') : t('Sign in to continue to your server.')}</span>
          </div>

          <Show when={banner()}>
            {(look) => (
              <div class={`auth-banner ${look().tone}`} role="alert">
                <Icon name={look().icon} class={look().tone === 'red' ? 'tr' : ''} />
                <div class="col grow">
                  <strong class="fs13">{look().localized ? t(look().title) : look().title}</strong>
                  <Show when={look().hint}><span class="t2 fs12">{t(look().hint!)}</span></Show>
                </div>
                <Show when={look().retry}>
                  <Button size="sm" variant="secondary" onClick={() => {
                    if (isRegister()) return;
                    void (resumeFailed() ? resumeLastSession() : loginUser(username().trim(), password(), stay()));
                  }}>{t('Retry')}</Button>
                </Show>
              </div>
            )}
          </Show>

          <div class="col g16">
            <Field label={t('Server address')} icon="db" value={serverUrl()} error={serverError()} placeholder="https://your-server:8443"
              onInput={(value) => {
                cancelResumeRetry();
                setServerUrl(value);
                setServerError(undefined);
                const link = pendingInvite();
                if (linkServerDiffers(link, value)) {
                  // The code belongs to the old server: it must not be sent to this one.
                  if (link && inviteCode() === link.code) setInviteCode('');
                  setPendingInvite(null);
                }
              }} />
            <Show when={isRegister()}>
              <Field label={t('Invite code')} icon="ticket" value={inviteCode()} onInput={setInviteCode} placeholder="K7M2QX9TPW4B" error={inviteError()}
                help={t('Ask the server admin for a code. It works a limited number of times and expires.')} />
            </Show>
            <Show when={isRegister()} fallback={
              <Field label={t('Username')} icon="user" value={username()} onInput={typed(setUsername)} placeholder="alice" autocomplete="username" autofocus={!rememberedUsername()} />
            }>
              <div class="row g12">
                <div class="grow"><Field label={t('Username')} value={username()} onInput={setUsername} placeholder="carol" autocomplete="username" error={usernameError()} /></div>
                <div class="grow"><Field label={t('Display name')} value={displayName()} onInput={setDisplayName} placeholder="Carol Danvers" /></div>
              </div>
            </Show>
            <div class="col g8">
              <Field label={t('Password')} icon="lock" type="password" value={password()} onInput={typed(setPassword)}
                autocomplete={isRegister() ? 'new-password' : 'current-password'} autofocus={!!rememberedUsername() && !isRegister()} />
              <Show when={isRegister() && password()}>
                <div class="row g8" aria-label={t('Password strength: {strength}', { strength: t(strength().label) })}>
                  {[1, 2, 3, 4].map((step) => <div class={`strength-seg${strength().score >= step ? ' on' : ''}`} />)}
                  <span class="fs12 ty b fit">{t(strength().label)}</span>
                </div>
              </Show>
            </div>
          </div>

          <div class="row g12" style={{ 'align-items': 'center' }}>
            <Toggle checked={stay()} onChange={setStay} label={t('Stay signed in')} />
            <span class="fs13">{t('Stay signed in')}</span>
          </div>

          <Button type="submit" variant="primary" size="lg" block loading={authLoading()}>
            {authLoading() ? t('Connecting securely…') : isRegister() ? t('Create account') : t('Sign in')}
          </Button>

          <div class="row g8 center">
            <span class="t2">{isRegister() ? t('Already have an account?') : t('New here?')}</span>
            <button type="button" class="link-btn ty b" style={{ margin: 0 }} onClick={switchMode}>{isRegister() ? t('Sign in') : t('Create an account')}</button>
          </div>
          <div class="row g8 t3 fs12" style={{ 'justify-content': 'center' }}>
            <Icon name="lock" size="sm" /><span>{t('Encrypted with TLS · certificate pinned on first use')}</span>
          </div>
        </form>
      </div>
    </div>
  );
}
