import { createMemo, createSignal, For, Show } from 'solid-js';
import { currentUser } from '../../stores/authStore';
import { deleteMessage, discardFailed, editMessage, MAX_MESSAGE_CHARS, messages, retryMessage, toggleReaction } from '../../stores/chatStore';
import { isMentioned } from '../../stores/activityStore';
import { knownUsers } from '../../stores/sessionStore';
import type { Message } from '../../types';
import { Avatar, Button, ConfirmDialog, Icon } from '../../ui';
import { Attachments } from './AttachmentView';
import { clock } from './chatFormat';
import { RichText } from './RichText';
import { EmojiPicker } from './EmojiPicker';
import { t } from '../../i18n';

type Props = {
  message: Message;
  /** Same author as the message above, shortly after: no avatar and name. */
  compact: boolean;
  onReply: (message: Message) => void;
  onJump: (messageId: string) => void;
};

const countLabel = (count: number) => (count > 99 ? '99+' : String(count));

export function MessageItem(props: Props) {
  const [editing, setEditing] = createSignal(false);
  const [draft, setDraft] = createSignal('');
  const [picker, setPicker] = createSignal(false);
  const [pickerUp, setPickerUp] = createSignal(false);
  let row: HTMLDivElement | undefined;
  const togglePicker = () => {
    // Open upwards when there is no room below the message.
    if (!picker() && row) setPickerUp(row.getBoundingClientRect().top + 340 > window.innerHeight);
    setPicker(!picker());
  };
  const [confirming, setConfirming] = createSignal(false);

  const me = () => currentUser();
  const msg = () => props.message;
  const mine = () => msg().author_id === me()?.id;
  const stored = () => !msg().state;
  const canDelete = () => stored() && (mine() || me()?.role === 'admin');
  const mentioned = () => !mine() && isMentioned(msg());
  const author = createMemo(() => knownUsers().find((user) => user.id === msg().author_id));
  const lookupLogin = (login: string) => knownUsers().find((user) => user.username.toLowerCase() === login)?.id;
  const nameOf = (userId: string) => {
    if (userId === me()?.id) return 'You';
    return knownUsers().find((user) => user.id === userId)?.display_name
      ?? messages().find((message) => message.author_id === userId)?.author_name ?? 'Someone';
  };

  const startEdit = () => {
    setDraft(msg().content);
    setEditing(true);
  };
  const finishEdit = async (save: boolean) => {
    const text = draft();
    setEditing(false);
    if (save && text.trim() && text.trim() !== msg().content) await editMessage(msg().id, text);
  };
  const copy = () => void navigator.clipboard?.writeText(msg().content);

  const Tools = () => (
    <div class="hover-tools" role="toolbar" aria-label={t('Message actions')}>
      <button type="button" aria-label={t('Add reaction')} title={t('Add reaction')} onClick={togglePicker}><Icon name="smile" size="sm" /></button>
      <button type="button" aria-label={t('Reply')} title={t('Reply')} onClick={() => props.onReply(msg())}><Icon name="send" size="sm" /></button>
      <button type="button" aria-label={t('Copy text')} title={t('Copy text')} onClick={copy}><Icon name="copy" size="sm" /></button>
      <Show when={mine()}><button type="button" aria-label={t('Edit message')} title={t('Edit')} onClick={startEdit}><Icon name="edit" size="sm" /></button></Show>
      <Show when={canDelete()}><button type="button" aria-label={t('Delete message')} title={t('Delete')} onClick={() => setConfirming(true)}><Icon name="trash" size="sm" /></button></Show>
    </div>
  );

  return (
    <div
      class={`msg${mine() ? ' own' : ''}${mentioned() ? ' mention' : ''}${msg().state === 'sending' ? ' sending' : ''}${msg().state === 'failed' ? ' failed' : ''}${props.compact ? ' compact' : ''}`}
      data-message-id={msg().id}
      ref={row}
    >
      <Show when={!props.compact} fallback={<span class="msg-gutter" />}>
        <Avatar name={msg().author_name} id={msg().author_id} src={author()?.avatar_url} />
      </Show>
      <div class="col grow" style={{ 'min-width': 0 }}>
        <Show when={!props.compact}>
          <div class="row g8">
            <span class="name">{msg().author_name}</span>
            <span class="time">{msg().state === 'sending' ? t('sending…') : clock(msg().timestamp)}</span>
            <Show when={msg().state === 'sending'}><span class="spin mini" /></Show>
            <Show when={msg().state === 'failed'}><span class="tr fs12 b">{t('Not sent')}</span></Show>
            <Show when={msg().edited_at}><span class="edited" title={t('Edited')}>({t('edited')})</span></Show>
            <Show when={author()?.role === 'admin'}><span class="tag admin">{t('Admin')}</span></Show>
          </div>
        </Show>

        <Show when={msg().reply_to}>
          {(quote) => (
            <button type="button" class={`reply-quote${quote().deleted ? ' gone' : ''}`} disabled={quote().deleted} onClick={() => props.onJump(quote().message_id)}>
              <Icon name="send" size="sm" class="flip" />
              <Show when={!quote().deleted} fallback={<span class="t3">{t('Original message deleted')}</span>}>
                <strong>{quote().author_name}</strong><span class="trunc">{quote().snippet}</span>
              </Show>
            </button>
          )}
        </Show>

        <Show when={editing()} fallback={
          <div class="body"><RichText text={msg().content} lookup={lookupLogin} /></div>
        }>
          <div class="col g8">
            <div class="composer is-focus" style={{ margin: 0 }}>
              <textarea
                class="txt edit-box"
                rows={Math.min(8, Math.max(1, draft().split('\n').length))}
                value={draft()}
                aria-label={t('Edit message')}
                ref={(el) => queueMicrotask(() => { el.focus(); el.setSelectionRange(el.value.length, el.value.length); })}
                onInput={(event) => setDraft(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Escape') { event.preventDefault(); void finishEdit(false); }
                  else if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); void finishEdit(true); }
                }}
              />
            </div>
            <div class="row g8">
              <span class="t3 fs12 grow">{t('Esc to cancel · Enter to save')}</span>
              <Show when={draft().length > MAX_MESSAGE_CHARS}><span class="tr fs12 b">{t('{count} over', { count: draft().length - MAX_MESSAGE_CHARS })}</span></Show>
              <Button size="sm" variant="secondary" onClick={() => void finishEdit(false)}>{t('Cancel')}</Button>
              <Button size="sm" variant="primary" disabled={!draft().trim() || draft().length > MAX_MESSAGE_CHARS} onClick={() => void finishEdit(true)}>{t('Save')}</Button>
            </div>
          </div>
        </Show>

        <Show when={msg().attachments.length > 0}><Attachments attachments={msg().attachments} /></Show>

        <Show when={msg().state === 'failed'}>
          <div class="row g12" style={{ 'margin-top': '4px' }}>
            <Show when={msg().failure}><span class="t3 fs12 trunc">{msg().failure}</span></Show>
            <button type="button" class="link-btn ty" style={{ margin: 0 }} onClick={() => void retryMessage(msg().id)}>{t('Retry')}</button>
            <button type="button" class="link-btn t2" style={{ margin: 0 }} onClick={() => discardFailed(msg().id)}>{t('Delete')}</button>
          </div>
        </Show>

        <Show when={(msg().reactions?.length ?? 0) > 0}>
          <div class="row g6 wrap" style={{ 'margin-top': '6px' }}>
            <For each={msg().reactions}>
              {(reaction) => {
                const reacted = () => reaction.user_ids.includes(me()?.id ?? '');
                return (
                  <button type="button" class={`react${reacted() ? ' mine' : ''}`} aria-pressed={reacted()}
                    aria-label={`${reaction.emoji} ${reaction.user_ids.length}`} onClick={() => void toggleReaction(msg(), reaction.emoji)}>
                    <span>{reaction.emoji}</span><span>{countLabel(reaction.user_ids.length)}</span>
                    <span class="tooltip react-tip" role="tooltip">
                      <span style={{ 'font-size': '16px' }}>{reaction.emoji}</span>
                      <span>{reaction.user_ids.length > 5 ? t('{names} and {count} others', { names: reaction.user_ids.slice(0, 5).map(nameOf).join(', '), count: reaction.user_ids.length - 5 }) : reaction.user_ids.slice(0, 5).map(nameOf).join(', ')}</span>
                    </span>
                  </button>
                );
              }}
            </For>
            <Show when={stored()}>
              <button type="button" class="pill add-react" aria-label={t('Add reaction')} onClick={togglePicker}><Icon name="smile" size="sm" /></button>
            </Show>
          </div>
        </Show>
      </div>

      <Show when={stored() && !editing()}><Tools /></Show>
      <Show when={picker()}>
        <div class={`picker-anchor${pickerUp() ? ' up' : ''}`}>
          <EmojiPicker onClose={() => setPicker(false)} onPick={(emoji) => { setPicker(false); void toggleReaction(msg(), emoji); }} />
        </div>
      </Show>

      <ConfirmDialog
        open={confirming()}
        danger
        title={t('Delete this message?')}
        message={<><div class="msg-preview"><span class="trunc">{msg().content}</span></div><div style={{ 'margin-top': '8px' }}>{t('It disappears for everyone. This cannot be undone.')}</div></>}
        confirmLabel={t('Delete')}
        onCancel={() => setConfirming(false)}
        onConfirm={() => { setConfirming(false); void deleteMessage(msg().id); }}
      />
    </div>
  );
}
