import { createEffect, createMemo, createSignal, For, on, onCleanup, Show } from 'solid-js';
import { notifyTyping } from '../../stores/activityStore';
import { attachFiles, formatSize, pendingFiles, removePending, retryUpload, takePending } from '../../stores/attachmentStore';
import { activeChannel, MAX_MESSAGE_CHARS, messages, replyingTo, sendMessage, setEditRequest, setReplyingTo } from '../../stores/chatStore';
import { discardDraft, flushDraft, readDraft, saveDraftSoon } from '../../stores/drafts';
import { connectionStatus, knownUsers, serverFeatures } from '../../stores/sessionStore';
import { currentUser } from '../../stores/authStore';
import { Avatar, Button, Icon } from '../../ui';
import { EmojiPicker } from './EmojiPicker';
import { inApp, isRecording, VoiceRecordButton, voiceMessagesAvailable } from './VoiceNote';
import { t } from '../../i18n';

/** The `@word` the caret is in, if any: where it starts and what has been typed of it. */
export function mentionAt(text: string, caret: number): { start: number; query: string } | null {
  const before = text.slice(0, caret);
  const match = /(^|[^\p{L}\p{N}])@([\p{L}\p{N}_.-]{0,32})$/u.exec(before);
  if (!match) return null;
  return { start: before.length - match[2].length - 1, query: match[2].toLowerCase() };
}

export function Composer() {
  const [text, setText] = createSignal(readDraft(activeChannel()?.id ?? ''));
  const [caret, setCaret] = createSignal(0);
  const [emoji, setEmoji] = createSignal(false);
  const [selected, setSelected] = createSignal(0);
  const [dismissedAt, setDismissedAt] = createSignal<number | null>(null);
  let box: HTMLTextAreaElement | undefined;
  let fileInput: HTMLInputElement | undefined;

  const offline = () => connectionStatus() !== 'connected';
  const voiceAvailable = () => voiceMessagesAvailable(serverFeatures(), inApp());
  const over = () => Math.max(0, text().length - MAX_MESSAGE_CHARS);
  const hasReady = () => pendingFiles().some((entry) => entry.status === 'ready');
  const busyUploading = () => pendingFiles().some((entry) => entry.status === 'uploading');
  const canSend = () => !offline() && over() === 0 && !busyUploading() && (text().trim().length > 0 || hasReady());

  const mention = createMemo(() => {
    const found = mentionAt(text(), caret());
    return found && found.start !== dismissedAt() ? found : null;
  });
  const candidates = createMemo(() => {
    const found = mention();
    if (!found) return [];
    const me = currentUser()?.id;
    return knownUsers()
      .filter((user) => user.id !== me && (user.username.toLowerCase().startsWith(found.query) || (user.display_name ?? '').toLowerCase().startsWith(found.query)))
      .slice(0, 6);
  });

  // Every change of the field is a draft of the channel it was typed in.
  const saveText = (value: string) => {
    setText(value);
    const channelId = activeChannel()?.id;
    if (channelId) saveDraftSoon(channelId, value);
  };

  // Each text channel has its own draft. Switching writes the one being left and loads the one being entered.
  createEffect(on(() => activeChannel()?.id ?? null, (channelId) => {
    flushDraft();
    setText(channelId ? readDraft(channelId) : '');
    setCaret(0);
  }, { defer: true }));
  // Opening a voice room unmounts the chat: keep what was typed.
  onCleanup(() => flushDraft());

  const syncCaret = () => setCaret(box?.selectionStart ?? text().length);
  const insertAt = (start: number, end: number, value: string) => {
    const next = text().slice(0, start) + value + text().slice(end);
    saveText(next);
    queueMicrotask(() => {
      if (!box) return;
      box.focus();
      box.setSelectionRange(start + value.length, start + value.length);
      setCaret(start + value.length);
    });
  };
  const pickMention = (login: string) => {
    const found = mention();
    if (!found) return;
    insertAt(found.start, caret(), `@${login} `);
    setSelected(0);
  };

  const submit = async () => {
    if (!canSend()) return;
    const channelId = activeChannel()?.id ?? null;
    const body = text();
    setText('');
    setCaret(0);
    // Only the draft of the channel this was sent from goes; another channel's draft stays.
    if (channelId) discardDraft(channelId);
    await sendMessage(body.trim() || '📎', takePending());
  };

  /** My last stored message in this channel, the one Up in an empty composer edits. */
  const lastOwnMessage = () => {
    const me = currentUser()?.id;
    const channelId = activeChannel()?.id;
    return [...messages()].reverse().find((message) => message.author_id === me && message.channel_id === channelId && !message.state);
  };

  const onKeyDown = (event: KeyboardEvent) => {
    const list = candidates();
    if (list.length > 0) {
      if (event.key === 'ArrowDown') { event.preventDefault(); setSelected((selected() + 1) % list.length); return; }
      if (event.key === 'ArrowUp') { event.preventDefault(); setSelected((selected() - 1 + list.length) % list.length); return; }
      if (event.key === 'Enter' || event.key === 'Tab') { event.preventDefault(); pickMention(list[selected()].username); return; }
      if (event.key === 'Escape') { event.preventDefault(); setDismissedAt(mention()!.start); return; }
    }
    // Up in an empty field, caret at the start, no modifier, no picker open: edit my last message here.
    // Not while an IME is composing text: its Up chooses a candidate, not a message to edit.
    if (event.key === 'ArrowUp' && !event.isComposing && event.keyCode !== 229 && !event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey
      && text() === '' && box?.selectionStart === 0 && box?.selectionEnd === 0 && !emoji() && list.length === 0) {
      const last = lastOwnMessage();
      if (last) {
        event.preventDefault();
        setEditRequest(last.id);
      }
      return;
    }
    if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      void submit();
    } else if (event.key === 'Escape' && replyingTo()) {
      setReplyingTo(null);
    }
  };

  const onPaste = (event: ClipboardEvent) => {
    const files = Array.from(event.clipboardData?.files ?? []);
    if (files.length > 0) {
      event.preventDefault();
      void attachFiles(files);
    }
  };

  return (
    <div class="composer-area">
      <Show when={replyingTo()}>
        {(target) => (
          <div class="reply-bar">
            <Icon name="send" size="sm" class="flip t3" />
            <span class="t2 fs13 trunc">{t('Replying to {name}: {content}', { name: target().author_name, content: target().content })}</span>
            <button type="button" class="icon-close" aria-label={t('Cancel reply')} onClick={() => setReplyingTo(null)}><Icon name="x" size="sm" /></button>
          </div>
        )}
      </Show>

      <Show when={pendingFiles().length > 0}>
        <div class="row g8 wrap attach-row" aria-label={t('Attachments')}>
          <For each={pendingFiles()}>
            {(entry) => (
              <span class={`pill file-pill ${entry.status}`} title={entry.error}>
                <Show when={entry.status === 'uploading'} fallback={<Icon name={entry.status === 'ready' ? 'file' : 'warn'} size="sm" />}><span class="spin mini" /></Show>
                <span class="trunc attach-name">{entry.name}</span>
                <Show when={entry.status === 'uploading'}><span class="t3">{t('Uploading…')}</span></Show>
                <Show when={entry.status === 'toolarge'}><span class="tr">{t('Too large: {size}. The limit is 10 MB.', { size: formatSize(entry.size) })}</span></Show>
                <Show when={entry.status === 'failed'}>
                  <span class="upload-failed">{t('Upload failed')}</span>
                  <button type="button" class="link-btn ty" style={{ margin: 0 }} onClick={() => void retryUpload(entry.key)}>{t('Retry')}</button>
                </Show>
                <button type="button" class="icon-close" aria-label={t('Remove {name}', { name: entry.name })} onClick={() => removePending(entry.key)}><Icon name="x" size="sm" /></button>
              </span>
            )}
          </For>
        </div>
      </Show>

      <div class="composer-anchor">
        <Show when={candidates().length > 0}>
          <div class="card mention-menu" role="listbox" aria-label={t('Members')}>
            <span class="sec-label mention-label">{t('Members matching "{query}"', { query: mention()!.query })}</span>
            <For each={candidates()}>
              {(user, index) => (
                <div class={`mem${index() === selected() ? ' sel' : ''}`} role="option" aria-selected={index() === selected()}
                  onMouseDown={(event) => { event.preventDefault(); pickMention(user.username); }}>
                  <Avatar name={user.display_name || user.username} id={user.id} size={24} />
                  <span class="grow trunc b">{user.display_name || user.username}</span>
                  <span class="t3 fs12">@{user.username}</span>
                </div>
              )}
            </For>
          </div>
        </Show>

        <Show when={emoji()}>
          <div class="composer-picker">
            <EmojiPicker showQuick={false} onClose={() => setEmoji(false)} onPick={(char) => { setEmoji(false); insertAt(caret(), caret(), char); }} />
          </div>
        </Show>

        <div class={`composer${over() > 0 ? ' is-error' : ''}${offline() ? ' is-offline' : ''}`}>
          <input ref={fileInput} type="file" multiple hidden onChange={(event) => { const files = Array.from(event.currentTarget.files ?? []); event.currentTarget.value = ''; void attachFiles(files); }} />
          <Button iconOnly size="sm" variant="ghost" icon="clip" label={t('Attach a file')} disabled={offline()} onClick={() => fileInput?.click()} />
          <Show when={!isRecording()}>
          <textarea
            ref={box}
            class="txt"
            rows={Math.min(8, Math.max(1, text().split('\n').length))}
            value={text()}
            disabled={offline()}
            aria-label={t('Message')}
            placeholder={offline() ? t('You are offline. Reconnecting…') : t('Message #{channel}', { channel: activeChannel()?.name ?? t('channel') })}
            onInput={(event) => {
              saveText(event.currentTarget.value);
              syncCaret();
              setSelected(0);
              setDismissedAt(null);
              const channel = activeChannel();
              if (channel && event.currentTarget.value) notifyTyping(channel.id);
            }}
            onKeyDown={onKeyDown}
            onKeyUp={syncCaret}
            onClick={syncCaret}
            onPaste={onPaste}
          />
          </Show>
          <Show when={over() > 0}><span class="tr fs12 b fit">{t('{count} over', { count: over() })}</span></Show>
          <Button iconOnly size="sm" variant="ghost" icon="smile" label={t('Emoji')} disabled={offline()} onClick={() => setEmoji(!emoji())} />
          <Show when={voiceAvailable()}><VoiceRecordButton disabled={offline()} /></Show>
          <Button iconOnly size="sm" variant="primary" icon="send" label={t('Send')} disabled={!canSend()} onClick={() => void submit()} />
        </div>
      </div>
    </div>
  );
}
