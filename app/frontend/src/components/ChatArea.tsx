import { createSignal, For, Show, createEffect, createMemo, onCleanup } from 'solid-js';
import { activeChannel, messages, sendMessage, chatLoading, editMessage, deleteMessage, toggleReaction } from '../stores/chatStore';
import { currentUser } from '../stores/authStore';
import { markRead, mentions, notifyTyping, typingNames } from '../stores/activityStore';
import { attachFile, downloadAttachment, pendingAttachments, removePending, takePending, uploading } from '../stores/attachmentStore';
import type { Message } from '../types';

const QUICK_REACTIONS = ['👍', '❤️', '😂', '🎉', '😮', '😢'];

export function ChatArea() {
  const [inputText, setInputText] = createSignal('');
  const [editingId, setEditingId] = createSignal<string | null>(null);
  const [editText, setEditText] = createSignal('');
  const [pickerFor, setPickerFor] = createSignal<string | null>(null);
  const [now, setNow] = createSignal(Date.now());
  let messagesEndRef: HTMLDivElement | undefined;
  let fileInput: HTMLInputElement | undefined;

  // Typing hints expire on their own: re-evaluate twice a second.
  const timer = setInterval(() => setNow(Date.now()), 500);
  onCleanup(() => clearInterval(timer));

  const scrollToBottom = () => {
    messagesEndRef?.scrollIntoView({ behavior: 'smooth' });
  };

  createEffect(() => {
    // When messages update, scroll down
    messages();
    scrollToBottom();
  });

  // Whatever is open counts as read.
  createEffect(() => {
    const channel = activeChannel();
    messages();
    if (channel) markRead(channel.id);
  });

  const typingLine = createMemo(() => {
    const channel = activeChannel();
    if (!channel) return '';
    const names = typingNames(channel.id, now());
    if (names.length === 0) return '';
    if (names.length === 1) return `${names[0]} is typing…`;
    if (names.length === 2) return `${names[0]} and ${names[1]} are typing…`;
    return 'Several people are typing…';
  });

  const handleSend = async (e: Event) => {
    e.preventDefault();
    const text = inputText();
    if (!text.trim() && pendingAttachments().length === 0) return;
    setInputText('');
    await sendMessage(text.trim() || '📎', takePending());
  };

  const startEdit = (message: Message) => {
    setEditingId(message.id);
    setEditText(message.content);
  };

  const finishEdit = async (save: boolean) => {
    const id = editingId();
    setEditingId(null);
    if (save && id) await editMessage(id, editText());
  };

  const formatTimestamp = (ts: number) => {
    const d = new Date(ts > 1e12 ? ts : ts * 1000);
    return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  };

  const isMine = (message: Message) => message.author_id === currentUser()?.id;
  const canDelete = (message: Message) => isMine(message) || currentUser()?.role === 'admin';
  const stored = (message: Message) => !message.id.startsWith('local-');

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
            <div class={`message-item ${mentions(msg.content) && !isMine(msg) ? 'mentioned' : ''}`}>
              <div class="avatar">
                {msg.author_name?.charAt(0).toUpperCase() || 'U'}
              </div>
              <div class="message-content-wrapper">
                <div class="message-meta">
                  <span class="message-author">{msg.author_name}</span>
                  <span class="message-time">{formatTimestamp(msg.timestamp)}</span>
                  <Show when={msg.edited_at}><span class="message-edited" title="Edited">(edited)</span></Show>
                </div>

                <Show when={editingId() === msg.id} fallback={<div class="message-text">{msg.content}</div>}>
                  <form class="message-edit" onSubmit={(e) => { e.preventDefault(); void finishEdit(true); }}>
                    <input
                      class="chat-input"
                      value={editText()}
                      onInput={(e) => setEditText(e.currentTarget.value)}
                      onKeyDown={(e) => { if (e.key === 'Escape') void finishEdit(false); }}
                      ref={(el) => queueMicrotask(() => el.focus())}
                    />
                    <button type="submit">Save</button>
                    <button type="button" onClick={() => void finishEdit(false)}>Cancel</button>
                  </form>
                </Show>

                {/* Attachments */}
                <Show when={msg.attachments && msg.attachments.length > 0}>
                  <div style={{ "margin-top": "8px", display: "flex", "flex-wrap": "wrap", gap: "8px" }}>
                    <For each={msg.attachments}>
                      {(att) => (
                        <button type="button" class="attachment" title="Download" onClick={() => void downloadAttachment(att)}>
                          <span>📎</span>
                          <span>{att.filename}</span>
                          <span class="attachment-size">({Math.max(1, Math.round(att.size_bytes / 1024))} KB)</span>
                        </button>
                      )}
                    </For>
                  </div>
                </Show>

                {/* Reactions */}
                <Show when={(msg.reactions?.length ?? 0) > 0}>
                  <div class="reactions">
                    <For each={msg.reactions}>
                      {(reaction) => (
                        <button
                          type="button"
                          class={`reaction ${reaction.user_ids.includes(currentUser()?.id ?? '') ? 'mine' : ''}`}
                          onClick={() => void toggleReaction(msg, reaction.emoji)}
                        >
                          {reaction.emoji} {reaction.user_ids.length}
                        </button>
                      )}
                    </For>
                  </div>
                </Show>

                {/* Actions (hover) */}
                <Show when={stored(msg)}>
                  <div class="message-actions">
                    <button type="button" title="React" onClick={() => setPickerFor(pickerFor() === msg.id ? null : msg.id)}>😊</button>
                    <Show when={isMine(msg)}>
                      <button type="button" title="Edit" onClick={() => startEdit(msg)}>✎</button>
                    </Show>
                    <Show when={canDelete(msg)}>
                      <button type="button" title="Delete" onClick={() => { if (confirm('Delete this message?')) void deleteMessage(msg.id); }}>🗑</button>
                    </Show>
                  </div>
                  <Show when={pickerFor() === msg.id}>
                    <div class="reaction-picker">
                      <For each={QUICK_REACTIONS}>
                        {(emoji) => (
                          <button type="button" onClick={() => { setPickerFor(null); void toggleReaction(msg, emoji); }}>{emoji}</button>
                        )}
                      </For>
                    </div>
                  </Show>
                </Show>
              </div>
            </div>
          )}
        </For>
        <div ref={messagesEndRef} />
      </div>

      {/* Typing hint */}
      <div class="typing-line" aria-live="polite">{typingLine()}</div>

      {/* Chat Input Bar */}
      <div class="chat-input-container">
        <Show when={pendingAttachments().length > 0 || uploading()}>
          <div class="pending-attachments">
            <For each={pendingAttachments()}>
              {(att) => (
                <span class="pending-attachment">
                  📎 {att.filename}
                  <button type="button" title="Remove" onClick={() => removePending(att.id)}>✕</button>
                </span>
              )}
            </For>
            <Show when={uploading()}><span class="pending-attachment">Uploading…</span></Show>
          </div>
        </Show>
        <form onSubmit={handleSend} class="chat-input-box">
          <input
            ref={fileInput}
            type="file"
            style={{ display: 'none' }}
            onChange={async (e) => {
              const file = e.currentTarget.files?.[0];
              e.currentTarget.value = '';
              if (file) await attachFile(file);
            }}
          />
          <button type="button" class="icon-btn" title="Attach a file" onClick={() => fileInput?.click()}>📎</button>
          <input
            type="text"
            class="chat-input"
            placeholder={`Message #${activeChannel()?.name || 'channel'}...`}
            value={inputText()}
            onInput={(e) => {
              setInputText(e.currentTarget.value);
              const channel = activeChannel();
              if (channel && e.currentTarget.value) notifyTyping(channel.id);
            }}
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
