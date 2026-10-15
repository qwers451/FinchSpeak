import { Show } from 'solid-js';
import { pendingTofu, setPendingTofu } from '../stores/authStore';
import { api } from '../services/api';

export function TofuModal() {
  const handleConfirm = async () => {
    const data = pendingTofu();
    if (data) {
      await api.trustFingerprint(data.hostname, data.fingerprint);
      setPendingTofu(null);
    }
  };

  const handleReject = () => {
    setPendingTofu(null);
  };

  return (
    <Show when={pendingTofu()}>
      {(data) => (
        <div class="modal-overlay">
          <div class="modal-card">
            <h2 class="modal-title" style={{ color: "var(--accent-teal)" }}>
              🔒 Trust Server Certificate?
            </h2>
            <p style={{ "font-size": "0.9rem", color: "var(--text-muted)", "line-height": "1.5" }}>
              This is your first time connecting to <strong>{data().hostname}</strong>. Verify the server's SHA-256 certificate fingerprint:
            </p>
            <div class="fingerprint-box">
              {data().fingerprint}
            </div>
            <p style={{ "font-size": "0.8rem", color: "var(--text-dim)" }}>
              Once trusted (TOFU), FinchSpeak will ensure this exact certificate is presented on future connections to prevent man-in-the-middle attacks.
            </p>
            <div style={{ display: "flex", gap: "10px", "margin-top": "6px" }}>
              <button class="btn-secondary" style={{ flex: 1 }} onClick={handleReject}>
                Cancel
              </button>
              <button class="btn-primary" style={{ flex: 1 }} onClick={handleConfirm}>
                Trust & Connect
              </button>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
