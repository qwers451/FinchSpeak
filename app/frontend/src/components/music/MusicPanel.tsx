import { createMemo, createSignal, For, onMount, Show } from 'solid-js';
import { api } from '../../services/api';
import { describeError } from '../../i18n/errors';
import { currentUser } from '../../stores/authStore';
import { knownUsers } from '../../stores/sessionStore';
import { musicPosition, musicState, musicVolume, sendMusicCommand, setMusicVolume } from '../../stores/musicStore';
import { setAppError } from '../../stores/notificationStore';
import type { MusicQueueEntry, MusicTrack } from '../../types';
import { Button, ConfirmDialog, Field, FinchBird, Icon, Slider } from '../../ui';
import { formatDuration, t } from '../../i18n';

/** Audio formats the server accepts (the picker filters on them; the server checks again). */
const ACCEPT = '.mp3,.ogg,.flac,.wav,audio/mpeg,audio/ogg,audio/flac,audio/wav';

/** A running position as m:ss (or h:mm:ss). Track lengths use formatDuration. */
export function formatClock(ms: number): string {
  const total = Math.floor(Math.max(0, ms) / 1000);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = String(total % 60).padStart(2, '0');
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, '0')}:${seconds}` : `${minutes}:${seconds}`;
}

type Upload = { id: number; name: string; status: 'uploading' | 'done' | 'error'; message?: string };
let uploadSeq = 0;

/** The room music panel: what plays now, the queue, and where tracks come from (library or my computer). */
export function MusicPanel(props: { onClose: () => void; modal?: boolean }) {
  const [tab, setTab] = createSignal<'library' | 'computer'>('library');
  /** The position while the seek slider is held; the seek is sent only when it is released. */
  const [dragSeconds, setDragSeconds] = createSignal<number | null>(null);
  const [tracks, setTracks] = createSignal<MusicTrack[]>([]);
  const [query, setQuery] = createSignal('');
  const [uploads, setUploads] = createSignal<Upload[]>([]);
  const [confirmTrack, setConfirmTrack] = createSignal<MusicTrack | null>(null);
  let libraryInput!: HTMLInputElement;
  let computerInput!: HTMLInputElement;

  const playing = () => musicState()?.playing ?? null;
  const queue = () => musicState()?.queue ?? [];
  const myId = () => currentUser()?.id;
  const staff = () => currentUser()?.role === 'admin' || currentUser()?.role === 'moderator';
  const nameOf = (id: string) => {
    const user = knownUsers().find((known) => known.id === id);
    return user ? user.display_name || user.username : t('Someone');
  };
  const canRemove = (item: MusicQueueEntry) => staff() || item.added_by === myId();
  const canDelete = (item: MusicTrack) => staff() || item.uploader_id === myId();
  const filtered = createMemo(() => {
    const needle = query().trim().toLowerCase();
    if (!needle) return tracks();
    return tracks().filter((item) => item.title.toLowerCase().includes(needle) || (item.artist ?? '').toLowerCase().includes(needle));
  });

  onMount(() => {
    api.musicLibrary().then(setTracks).catch((error) => setAppError(describeError(error)));
  });

  const patchUpload = (id: number, patch: Partial<Upload>) => setUploads((list) => list.map((item) => (item.id === id ? { ...item, ...patch } : item)));

  /** Uploads each file in turn. Library files stay on the server; my-computer files go straight into the queue. */
  async function uploadFiles(files: File[], keep: boolean) {
    for (const file of files) {
      uploadSeq += 1;
      const id = uploadSeq;
      setUploads((list) => [...list, { id, name: file.name, status: 'uploading' }]);
      try {
        const uploaded = await api.musicUpload(file, keep);
        if (keep) setTracks((list) => [uploaded, ...list]);
        else await api.musicCommand({ kind: 'enqueue', track_id: uploaded.id });
        patchUpload(id, { status: 'done' });
      } catch (error) {
        patchUpload(id, { status: 'error', message: describeError(error) });
      }
    }
  }

  function onPicked(event: Event, keep: boolean) {
    const input = event.currentTarget as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    if (files.length > 0) void uploadFiles(files, keep);
    input.value = '';
  }

  async function deleteTrack(item: MusicTrack) {
    setConfirmTrack(null);
    try {
      await api.musicDelete(item.id);
      setTracks((list) => list.filter((known) => known.id !== item.id));
    } catch (error) {
      setAppError(describeError(error));
    }
  }

  const tabClass = (name: 'library' | 'computer') => `tab${tab() === name ? ' on' : ''}`;

  return (
    <section class="music-panel" aria-label={t('Music')}>
      <Show when={!props.modal}>
        <header class="music-head">
          <Icon name="music" size="sm" class="t3" />
          <h3 class="grow trunc">{t('Music')}</h3>
          <Button iconOnly size="sm" variant="ghost" icon="x" label={t('Close')} onClick={props.onClose} />
        </header>
      </Show>

      <div class="music-body">
        <Show when={playing()}>
          {(now) => (
            <section class="music-now">
              <h4 class="music-heading">{t('Now playing')}</h4>
              <div class="col music-titles">
                <span class="b trunc" title={now().entry.track.title}>{now().entry.track.title}</span>
                <span class="t2 fs12 trunc">
                  {[now().entry.track.artist, t('Added by {name}', { name: nameOf(now().entry.added_by) })].filter(Boolean).join(' · ')}
                </span>
              </div>
              <div class="music-progress">
                <Slider
                  value={dragSeconds() ?? Math.floor(musicPosition() / 1000)}
                  min={0}
                  max={Math.max(1, Math.floor(now().entry.track.duration_ms / 1000))}
                  label={t('Track progress')}
                  onChange={setDragSeconds}
                  onCommit={(seconds) => { setDragSeconds(null); void sendMusicCommand({ kind: 'seek', position_ms: seconds * 1000 }); }}
                />
                <span class="t3 fs12 music-clock">
                  {formatClock(musicPosition())} / {formatClock(now().entry.track.duration_ms)}
                </span>
              </div>
              <div class="row g8 music-controls">
                <Button size="sm" variant="secondary" onClick={() => void sendMusicCommand({ kind: now().paused ? 'resume' : 'pause' })}>
                  {now().paused ? t('Resume') : t('Pause')}
                </Button>
                <Button size="sm" variant="secondary" onClick={() => void sendMusicCommand({ kind: 'skip' })}>{t('Next')}</Button>
                <Show when={staff()}>
                  <Button size="sm" variant="danger-ghost" onClick={() => void sendMusicCommand({ kind: 'clear' })}>{t('Stop for everyone')}</Button>
                </Show>
              </div>
              <div class="row g8 music-volume">
                <span class="t2 fs12">{t('My music volume')}</span>
                <Slider value={Math.round(musicVolume() * 100)} min={0} max={100} cool label={t('My music volume')} onChange={(value) => void setMusicVolume(value / 100)} />
              </div>
            </section>
          )}
        </Show>

        <section class="music-queue-block">
          <h4 class="music-heading">{t('Queue')}</h4>
          <Show when={!playing() && queue().length === 0}>
            <div class="music-empty">
              <FinchBird size={72} />
              <p class="t2 fs13">{t('Nothing is playing. Add a track to start.')}</p>
            </div>
          </Show>
          <ol class="music-list" aria-label={t('Queue')}>
            <For each={queue()}>
              {(item) => (
                <li class="music-row">
                  <div class="col grow">
                    <span class="trunc" title={item.track.title}>{item.track.title}</span>
                    <span class="t3 fs12 trunc">
                      {[item.track.artist, formatDuration(Math.floor(item.track.duration_ms / 1000)), t('Added by {name}', { name: nameOf(item.added_by) })].filter(Boolean).join(' · ')}
                    </span>
                  </div>
                  <Show when={canRemove(item)}>
                    <Button iconOnly size="sm" variant="ghost" icon="x" label={t('Remove from queue')} onClick={() => void sendMusicCommand({ kind: 'remove', entry_id: item.entry_id })} />
                  </Show>
                </li>
              )}
            </For>
          </ol>
        </section>

        <section class="music-add">
          <h4 class="music-heading">{t('Add')}</h4>
          <div class="tabs" role="tablist">
            <button type="button" role="tab" class={tabClass('library')} aria-selected={tab() === 'library'} onClick={() => setTab('library')}><span class="trunc">{t('Library')}</span></button>
            <button type="button" role="tab" class={tabClass('computer')} aria-selected={tab() === 'computer'} onClick={() => setTab('computer')}><span class="trunc">{t('From my computer')}</span></button>
          </div>

          <Show when={tab() === 'library'}>
            <Field label={t('Search by title or artist')} icon="search" placeholder={t('Search by title or artist')} value={query()} onInput={setQuery} />
            <Show
              when={filtered().length > 0}
              fallback={
                <div class="music-empty">
                  <Show when={tracks().length === 0}><FinchBird size={72} /></Show>
                  <p class="t2 fs13">{tracks().length === 0 ? t('The library is empty.') : t('Nothing matches the search.')}</p>
                </div>
              }
            >
              <ul class="music-list" aria-label={t('Library')}>
                <For each={filtered()}>
                  {(item) => (
                    <li class="music-row">
                      <div class="col grow">
                        <span class="trunc" title={item.title}>{item.title}</span>
                        <span class="t3 fs12 trunc">{[item.artist, formatDuration(Math.floor(item.duration_ms / 1000))].filter(Boolean).join(' · ')}</span>
                      </div>
                      <Button size="sm" variant="secondary" onClick={() => void sendMusicCommand({ kind: 'enqueue', track_id: item.id })}>{t('Add to queue')}</Button>
                      <Show when={canDelete(item)}>
                        <Button iconOnly size="sm" variant="danger-ghost" icon="trash" label={t('Delete track')} onClick={() => setConfirmTrack(item)} />
                      </Show>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
            <Button size="sm" variant="secondary" onClick={() => libraryInput.click()}>{t('Upload to the server')}</Button>
            <input ref={libraryInput} type="file" accept={ACCEPT} multiple class="music-file-input" onChange={(event) => onPicked(event, true)} />
          </Show>

          <Show when={tab() === 'computer'}>
            <p class="t2 fs12">{t('The file is sent to the server for this session and removed after it plays.')}</p>
            <Button size="sm" variant="secondary" onClick={() => computerInput.click()}>{t('Choose files')}</Button>
            <input ref={computerInput} type="file" accept={ACCEPT} multiple class="music-file-input" onChange={(event) => onPicked(event, false)} />
          </Show>

          <Show when={uploads().length > 0}>
            <ul class="music-uploads">
              <For each={uploads()}>
                {(upload) => (
                  <li class={`music-upload ${upload.status}`}>
                    <Show when={upload.status === 'uploading'}><span class="music-spin" aria-hidden="true" /></Show>
                    <span class="trunc">
                      {upload.status === 'uploading' && t('Uploading {name}…', { name: upload.name })}
                      {upload.status === 'done' && `${upload.name} · ${t('Done')}`}
                      {upload.status === 'error' && `${upload.name}: ${upload.message}`}
                    </span>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </section>
      </div>

      <ConfirmDialog
        open={confirmTrack() !== null}
        title={t('Delete track')}
        message={t('Delete {title} from the library? It cannot be undone.', { title: confirmTrack()?.title ?? '' })}
        confirmLabel={t('Delete')}
        danger
        onConfirm={() => confirmTrack() && void deleteTrack(confirmTrack()!)}
        onCancel={() => setConfirmTrack(null)}
      />
    </section>
  );
}
