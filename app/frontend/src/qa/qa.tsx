import { render } from 'solid-js/web';
import { api } from '../services/api';
import { setCurrentUser } from '../stores/authStore';
import { setActiveChannel, setChannels, setMessages } from '../stores/chatStore';
import { setConnectionStatus, setKnownUsers, setLocalPeerId, setServerFeatures } from '../stores/sessionStore';
import { setActiveVoiceChannel, setVoiceParticipants } from '../stores/voiceStore';
import { applyMusicState, setMusicPanelOpen } from '../stores/musicStore';
import { setSoundboardOpen } from '../stores/soundboardStore';
import { setNestView } from '../stores/nestView';
import { setProfileOpen } from '../stores/profileState';
import { setAdminOpen } from '../stores/adminStore';
import { updateAppearance } from '../stores/settingsStore';
import { Sidebar } from '../components/Sidebar';
import { MembersPanel } from '../components/MembersPanel';
import { ChatArea } from '../components/ChatArea';
import { VoicePanel } from '../components/VoicePanel';
import { TreeView } from '../components/tree/TreeView';
import { AdminModal } from '../components/admin/AdminModal';
import { SettingsModal } from '../components/settings/SettingsModal';
import { RoomToolWindows } from '../components/voice/RoomTools';
import { ContextMenuHost, openContextMenu } from '../ui/ContextMenu';
import { IconSprite, Titlebar, ToastHost } from '../ui';
import { Show } from 'solid-js';
import * as fx from './fixtures';

/** Dev-only entry (`?qa=<screen>&theme=&lang=&size=`): the real components with fixture state, no server. Not in the production bundle. */

const qaApi = api as unknown as Record<string, unknown>;
const isVoice = (screen: string) => /^(voice|music|sound|ctx|nest)/.test(screen);

function setup(screen: string) {
  setCurrentUser(fx.users[0]);
  setKnownUsers(fx.users);
  setChannels(fx.channels);
  setServerFeatures(['music', 'soundboard', 'flock']);
  setConnectionStatus('connected');
  setLocalPeerId(1);

  const count = screen.endsWith('12') ? 12 : screen.endsWith('1') ? 1 : 4;
  if (isVoice(screen)) {
    const room = fx.channels[3];
    setActiveVoiceChannel(room);
    setVoiceParticipants(fx.participants(count, room.id));
    setActiveChannel(room);
  } else {
    setActiveChannel(fx.channels[0]);
    setMessages(fx.messages());
    setVoiceParticipants(fx.participants(3, 'v-2'));
  }

  const lib = screen === 'music-lib50' ? 50 : screen === 'music-empty' ? 0 : 6;
  qaApi.musicLibrary = async () => fx.library(lib);
  qaApi.soundboard = async () => fx.sounds;
  qaApi.musicUpload = (file: File) => (file.name.startsWith('fail')
    ? Promise.reject(new Error('Файл не поддерживается сервером: слишком большой размер и неизвестный формат аудио'))
    : new Promise(() => {}));

  if (screen.startsWith('music')) {
    const queue = screen === 'music-empty' ? [] : fx.queueEntries(screen === 'music-lib50' ? 10 : 4);
    applyMusicState({
      channel_id: 'v-1', server_ms: 1000,
      state: queue.length
        ? { queue: queue.slice(1), playing: { entry: queue[0], position_ms: 30_000, at_server_ms: 1000, paused: false } }
        : { queue: [], playing: null },
    });
    setMusicPanelOpen(true);
  } else if (isVoice(screen)) {
    applyMusicState({ channel_id: 'v-1', server_ms: 1000, state: { queue: [], playing: { entry: fx.queueEntries(1)[0], position_ms: 30_000, at_server_ms: 1000, paused: false } } });
  }
  if (screen === 'soundboard') setSoundboardOpen(true);
  if (screen === 'nest') setNestView(true);
  if (screen.startsWith('settings')) setProfileOpen(true);
  if (screen === 'admin') setAdminOpen(true);
}

/** After the first paint: starts uploads and opens menus, which need real DOM nodes. */
function afterPaint(screen: string) {
  const pick = (name: string) => {
    const input = document.querySelector<HTMLInputElement>('.music-file-input');
    if (!input) return;
    const data = new DataTransfer();
    data.items.add(new File([new Uint8Array(10)], name, { type: 'audio/mpeg' }));
    input.files = data.files;
    input.dispatchEvent(new Event('change', { bubbles: true }));
  };
  if (screen === 'music-upload') pick('очень-длинное-имя-загружаемого-файла-с-музыкой-для-проверки-усечения.mp3');
  if (screen === 'music-error') pick('fail-очень-длинное-имя-загружаемого-файла-с-музыкой.mp3');
  if (screen === 'ctx-long') {
    openContextMenu({ x: 300, y: 200 }, [
      { kind: 'header', label: 'Всеволод Иванович Мирошниченко-Задунайский' },
      { kind: 'item', label: 'Очень длинный пункт меню, который не помещается в обычную ширину меню', icon: 'vol', onSelect: () => {} },
      { kind: 'item', label: 'Удалить', icon: 'trash', danger: true, onSelect: () => {} },
      { kind: 'separator' },
      { kind: 'slider', label: 'Громкость участника', min: 0, max: 200, step: 5, value: 100, onChange: () => {}, format: (v) => `${v}%` },
      { kind: 'submenu', label: 'Позвать в комнату', icon: 'vol', items: [{ kind: 'item', label: 'Комната', onSelect: () => {} }] },
    ]);
  }
  if (screen === 'ctx-participant') {
    const target = document.querySelector<HTMLElement>('.vp');
    target?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 200, clientY: 300 }));
  }
}

function QaShell(props: { screen: string }) {
  return (
    <div class="shell">
      <IconSprite />
      <Titlebar />
      <Show when={props.screen !== 'nest'} fallback={<TreeView />}>
        <div class="app-layout">
          <Sidebar />
          <Show when={isVoice(props.screen)} fallback={<ChatArea />}><VoicePanel /></Show>
          <MembersPanel />
        </div>
      </Show>
      <AdminModal />
      <SettingsModal />
      <RoomToolWindows />
      <ToastHost />
      <ContextMenuHost />
    </div>
  );
}

export function mountQa(root: HTMLElement) {
  const params = new URLSearchParams(window.location.search);
  const screen = params.get('qa') || 'chat';
  updateAppearance({
    theme: (params.get('theme') ?? 'claude') as 'claude',
    language: params.get('lang') === 'en' ? 'en' : 'ru',
    textSize: (params.get('size') ?? 'medium') as 'medium',
    reduceMotion: true,
  });
  setup(screen);
  render(() => <QaShell screen={screen} />, root);
  setTimeout(() => afterPaint(screen), 300);
  setTimeout(() => { (window as unknown as { __qaReady: boolean }).__qaReady = true; }, 900);
}
