import { For, Show, createSignal, onMount } from 'solid-js';
import { adminOpen, createInvite, invites, loadInvites, revokeInvite, setAdminOpen } from '../stores/adminStore';

const formatTime = (seconds: number) => new Date(seconds * 1000).toLocaleString();

/** Admin window: invitation codes. */
export function AdminPanel() {
  const [maxUses, setMaxUses] = createSignal(1);
  const [hours, setHours] = createSignal(72);
  const [fresh, setFresh] = createSignal<string | null>(null);

  onMount(() => void loadInvites());

  const create = async () => setFresh(await createInvite(maxUses(), hours()));

  return (
    <Show when={adminOpen()}>
      <div class="modal-overlay">
        <div class="modal-card admin-panel">
          <div class="admin-header">
            <h2>Administration</h2>
            <button type="button" class="icon-btn" title="Close" onClick={() => setAdminOpen(false)}>✕</button>
          </div>

          <h3>Invitations</h3>
          <p class="admin-hint">People need a code to register. Give it to them in person; it stops working after its uses or time run out.</p>
          <div class="admin-invite-form">
            <label>Uses <input type="number" min="1" max="1000" value={maxUses()} onInput={(e) => setMaxUses(Number(e.currentTarget.value) || 1)} /></label>
            <label>Valid for (hours) <input type="number" min="1" max="8760" value={hours()} onInput={(e) => setHours(Number(e.currentTarget.value) || 1)} /></label>
            <button type="button" class="btn-primary" onClick={() => void create()}>Create invite</button>
          </div>
          <Show when={fresh()}>
            <div class="admin-fresh-code" role="status">New code: <code>{fresh()}</code></div>
          </Show>

          <table class="admin-table">
            <thead><tr><th>Code</th><th>Used</th><th>Expires</th><th /></tr></thead>
            <tbody>
              <For each={invites()}>
                {(invite) => (
                  <tr>
                    <td><code>{invite.code}</code></td>
                    <td>{invite.uses} / {invite.max_uses}</td>
                    <td>{formatTime(invite.expires_at)}</td>
                    <td><button type="button" onClick={() => void revokeInvite(invite.code)}>Revoke</button></td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </div>
      </div>
    </Show>
  );
}
