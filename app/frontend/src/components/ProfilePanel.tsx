import { Show, createSignal } from 'solid-js';
import { currentUser, logoutUser } from '../stores/authStore';
import { profileMessage, profileOpen, savePassword, saveDisplayName, setProfileMessage, setProfileOpen } from '../stores/profileStore';

/** Your own settings: display name and password. */
export function ProfilePanel() {
  const [name, setName] = createSignal('');
  const [current, setCurrent] = createSignal('');
  const [next, setNext] = createSignal('');
  const [again, setAgain] = createSignal('');

  const close = () => {
    setProfileOpen(false);
    setProfileMessage(null);
    setCurrent('');
    setNext('');
    setAgain('');
  };

  return (
    <Show when={profileOpen()}>
      <div class="modal-overlay">
        <div class="modal-card admin-panel">
          <div class="admin-header">
            <h2>Your profile</h2>
            <button type="button" class="icon-btn" title="Close" onClick={close}>✕</button>
          </div>
          <Show when={profileMessage()}><div class="admin-fresh-code" role="status">{profileMessage()}</div></Show>

          <h3>Display name</h3>
          <form class="admin-invite-form" onSubmit={async (e) => { e.preventDefault(); if (await saveDisplayName(name() || currentUser()?.display_name || '')) setName(''); }}>
            <label>Name <input type="text" maxLength={32} placeholder={currentUser()?.display_name} value={name()} onInput={(e) => setName(e.currentTarget.value)} /></label>
            <button type="submit" class="btn-primary">Change name</button>
          </form>

          <h3>Password</h3>
          <form class="admin-invite-form" onSubmit={async (e) => {
            e.preventDefault();
            if (await savePassword(current(), next(), again())) { setCurrent(''); setNext(''); setAgain(''); }
          }}>
            <label>Current <input type="password" autocomplete="current-password" value={current()} onInput={(e) => setCurrent(e.currentTarget.value)} /></label>
            <label>New <input type="password" autocomplete="new-password" value={next()} onInput={(e) => setNext(e.currentTarget.value)} /></label>
            <label>Again <input type="password" autocomplete="new-password" value={again()} onInput={(e) => setAgain(e.currentTarget.value)} /></label>
            <button type="submit" class="btn-primary">Change password</button>
          </form>

          <h3>Session</h3>
          <button type="button" class="btn-secondary" onClick={() => { close(); void logoutUser(); }}>Log out</button>
        </div>
      </div>
    </Show>
  );
}
