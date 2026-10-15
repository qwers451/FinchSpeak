import { createSignal, Show } from 'solid-js';
import {
  serverUrl,
  setServerUrl,
  loginUser,
  registerUser,
  authLoading,
  authError,
} from '../stores/authStore';

export function AuthModal() {
  const [isRegister, setIsRegister] = createSignal(false);
  const [username, setUsername] = createSignal('alice');
  const [displayName, setDisplayName] = createSignal('Alice');
  const [password, setPassword] = createSignal('password123');

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    if (isRegister()) {
      await registerUser(username(), displayName(), password());
    } else {
      await loginUser(username(), password());
    }
  };

  return (
    <div class="modal-overlay">
      <div class="modal-card">
        {/* Brand */}
        <div style={{ display: "flex", "align-items": "center", gap: "12px", "justify-content": "center", "margin-bottom": "6px" }}>
          <img src="/logo.png" alt="FinchSpeak" style={{ width: "42px", height: "42px", "border-radius": "10px", "box-shadow": "var(--glow-teal)" }} />
          <h2 class="modal-title">FinchSpeak</h2>
        </div>

        <div style={{ "text-align": "center", color: "var(--text-muted)", "font-size": "0.9rem" }}>
          {isRegister() ? 'Create an account to join the server' : 'Welcome back! Sign in to continue'}
        </div>

        <Show when={authError()}>
          <div style={{
            background: "rgba(255, 71, 87, 0.15)",
            border: "1px solid var(--accent-danger)",
            color: "#ff6b81",
            padding: "10px 14px",
            "border-radius": "8px",
            "font-size": "0.85rem"
          }}>
            {authError()}
          </div>
        </Show>

        <form onSubmit={handleSubmit} style={{ display: "flex", "flex-direction": "column", gap: "14px" }}>
          <div class="form-group">
            <label class="form-label">Server URL</label>
            <input
              type="text"
              class="form-input"
              value={serverUrl()}
              onInput={(e) => setServerUrl(e.currentTarget.value)}
              placeholder="http://127.0.0.1:8080"
              required
            />
          </div>

          <div class="form-group">
            <label class="form-label">Username</label>
            <input
              type="text"
              class="form-input"
              value={username()}
              onInput={(e) => setUsername(e.currentTarget.value)}
              placeholder="e.g. alice"
              required
            />
          </div>

          <Show when={isRegister()}>
            <div class="form-group">
              <label class="form-label">Display Name</label>
              <input
                type="text"
                class="form-input"
                value={displayName()}
                onInput={(e) => setDisplayName(e.currentTarget.value)}
                placeholder="e.g. Alice Cooper"
                required
              />
            </div>
          </Show>

          <div class="form-group">
            <label class="form-label">Password</label>
            <input
              type="password"
              class="form-input"
              value={password()}
              onInput={(e) => setPassword(e.currentTarget.value)}
              placeholder="••••••••"
              required
            />
          </div>

          <button
            type="submit"
            class="btn-primary"
            disabled={authLoading()}
            style={{ opacity: authLoading() ? 0.7 : 1 }}
          >
            {authLoading() ? 'Connecting via QUIC...' : (isRegister() ? 'Register & Connect' : 'Sign In')}
          </button>
        </form>

        <div style={{ "text-align": "center", "font-size": "0.85rem", color: "var(--text-dim)" }}>
          {isRegister() ? 'Already have an account? ' : "Don't have an account? "}
          <span
            style={{ color: "var(--accent-teal)", cursor: "pointer", "font-weight": "600" }}
            onClick={() => setIsRegister(!isRegister())}
          >
            {isRegister() ? 'Sign In' : 'Register'}
          </span>
        </div>
      </div>
    </div>
  );
}
