import { createSignal, For, Show, onMount, createEffect } from 'solid-js';
import { activeChannel, messages, sendMessage, chatLoading } from '../stores/chatStore';
import { currentUser } from '../stores/authStore';

export function ChatArea() {
  const [inputText, setInputText] = createSignal('');
  let messagesEndRef: HTMLDivElement | undefined;

  const scrollToBottom = () => {
    messagesEndRef?.scrollIntoView({ behavior: 'smooth' });
  };

  createEffect(() => {
    // When messages update, scroll down
    messages();
    scrollToBottom();
  });

  const handleSend = async (e: Event) => {
    e.preventDefault();
    const text = inputText();
    if (!text.trim()) return;
    setInputText('');
    await sendMessage(text);
  };

  const formatTimestamp = (ts: number) => {
    const d = new Date(ts > 1e12 ? ts : ts * 1000);
    return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  };

  return (
    <main class="chat-main">
      {/* Channel Header */}
      <div class="chat-header">
        <div class="header-channel-name">
          <span style={{ color: "var(--text-dim)", "font-size": "1.2rem" }}>#</span>
          <span>{activeChannel()?.name || 'select-channel'}</span>
          <Show when={activeChannel()?.topic}>
            <span class="header-topic">{activeChannel()?.topic}</span>
          </Show>
        </div>
      </div>

      {/* Message List */}
      <div class="message-list">
        <Show when={chatLoading()}>
          <div style={{ color: "var(--text-muted)", "text-align": "center", padding: "20px" }}>
            Loading message history...
          </div>
        </Show>

        <Show when={!chatLoading() && messages().length === 0}>
          <div style={{ color: "var(--text-dim)", "text-align": "center", "margin-top": "auto", "margin-bottom": "auto" }}>
            <p style={{ "font-size": "1.2rem", "font-weight": "600", color: "var(--text-muted)" }}>
              Welcome to #{activeChannel()?.name}!
            </p>
            <p style={{ "font-size": "0.9rem", "margin-top": "6px" }}>
              This is the start of the conversation. Send the first message!
            </p>
          </div>
        </Show>

        <For each={messages()}>
          {(msg) => (
            <div class="message-item">
              <div class="avatar">
                {msg.author_name?.charAt(0).toUpperCase() || 'U'}
              </div>
              <div class="message-content-wrapper">
                <div class="message-meta">
                  <span class="message-author">{msg.author_name}</span>
                  <span class="message-time">{formatTimestamp(msg.timestamp)}</span>
                </div>
                <div class="message-text">{msg.content}</div>

                {/* Attachments */}
                <Show when={msg.attachments && msg.attachments.length > 0}>
                  <div style={{ "margin-top": "8px", display: "flex", "flex-wrap": "wrap", gap: "8px" }}>
                    <For each={msg.attachments}>
                      {(att) => (
                        <div style={{
                          background: "var(--bg-card)",
                          border: "1px solid var(--border-color)",
                          padding: "6px 12px",
                          "border-radius": "6px",
                          "font-size": "0.85rem",
                          display: "flex",
                          "align-items": "center",
                          gap: "6px",
                        }}>
                          <span>📎</span>
                          <span>{att.filename}</span>
                          <span style={{ color: "var(--text-dim)", "font-size": "0.75rem" }}>
                            ({Math.round(att.size_bytes / 1024)} KB)
                          </span>
                        </div>
                      )}
                    </For>
                  </div>
                </Show>
              </div>
            </div>
          )}
        </For>
        <div ref={messagesEndRef} />
      </div>

      {/* Chat Input Bar */}
      <div class="chat-input-container">
        <form onSubmit={handleSend} class="chat-input-box">
          <input
            type="text"
            class="chat-input"
            placeholder={`Message #${activeChannel()?.name || 'channel'}...`}
            value={inputText()}
            onInput={(e) => setInputText(e.currentTarget.value)}
          />
          <button
            type="submit"
            class="icon-btn"
            style={{ color: "var(--accent-teal)" }}
            title="Send"
          >
            ➤
          </button>
        </form>
      </div>
    </main>
  );
}
