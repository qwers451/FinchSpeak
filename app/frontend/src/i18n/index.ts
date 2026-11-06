import { prefs } from '../stores/settingsStore';

export type TranslationParams = Record<string, string | number>;

/** Russian UI copy keyed by its English source. User and server supplied strings stay untouched. */
export const ru: Readonly<Record<string, string>> = {
  'Settings': 'Настройки', 'Profile': 'Профиль', 'Notifications': 'Уведомления', 'Appearance': 'Внешний вид', 'Keys and about': 'Клавиши и о программе',
  'Theme': 'Тема', 'Claude Dark': 'Claude Dark', 'Finch Custom': 'Finch Custom', 'Language': 'Язык', 'Russian': 'Русский', 'English': 'Английский',
  'Density': 'Плотность', 'Comfortable': 'Свободная', 'Compact': 'Компактная', 'Compact fits more messages on the screen.': 'Компактный режим показывает больше сообщений на экране.',
  'Text size': 'Размер текста', 'Small': 'Маленький', 'Medium': 'Средний', 'Large': 'Большой', 'Reduce motion': 'Уменьшить анимацию',
  'Turns off pulsing rings and slides.': 'Отключает пульсацию и скользящие переходы.', 'Close button hides to the tray': 'Сворачивать в область уведомлений',
  'FinchSpeak keeps running (and keeps you in the call). Quit from the tray menu.': 'FinchSpeak продолжит работу, и звонок не прервётся. Выход доступен в меню значка приложения.',
  'Start with Windows': 'Запускать вместе с Windows', 'Starts hidden in the tray and signs you in.': 'Запускается свёрнутым в трей и входит в аккаунт.',
  'Could not change the startup setting': 'Не удалось изменить автозапуск',
  'Members panel': 'Список участников', 'The list of people on the right.': 'Список пользователей справа.',
  'Change picture': 'Изменить изображение', 'Upload picture': 'Загрузить изображение', 'Remove': 'Удалить', 'PNG, JPEG or WebP, up to 12 MB. You choose which part people see.': 'PNG, JPEG или WebP, до 12 МБ. Вы сами выбираете, какую часть изображения увидят другие.',
  'Picture changed.': 'Изображение изменено.', 'Display name': 'Отображаемое имя', 'Shown to everyone. Up to 32 characters.': 'Видно всем. Не более 32 символов.',
  'Username': 'Имя пользователя', 'cannot be changed': 'нельзя изменить', 'Admin': 'Администратор', 'Save name': 'Сохранить имя', 'Status': 'Статус',
  'Online': 'В сети', 'Idle': 'Неактивен', 'Do not disturb': 'Не беспокоить', 'Status text': 'Текст статуса', 'In a meeting, back at 5': 'На встрече, вернусь в 17:00',
  'Set': 'Установить', 'Change password': 'Изменить пароль', 'Current password': 'Текущий пароль', 'New password': 'Новый пароль', 'Repeat the new password': 'Повторите новый пароль',
  'The passwords do not match': 'Пароли не совпадают', 'Changing the password signs you out on your other devices.': 'После смены пароля на других устройствах потребуется войти снова.',
  'Devices': 'Устройства', 'This computer': 'Этот компьютер', 'Unknown device': 'Неизвестное устройство', 'Last active {when}': 'Был активен {when}',
  'End': 'Завершить', 'Sign out everywhere else': 'Выйти на всех остальных устройствах',
  'Signed in as': 'Вы вошли как', 'Log out': 'Выйти', 'Mentions': 'Упоминания', 'A notification when someone mentions you, even if the window is in the background.': 'Уведомлять, когда вас упоминают, даже если окно свёрнуто.',
  'Every message': 'Каждое сообщение', 'Also for channels you are not looking at.': 'В том числе в каналах, которые сейчас не открыты.',
  'Someone joins my voice room': 'Кто-то вошёл в мою голосовую комнату', 'Sounds': 'Звуки', 'A short tone for mentions and for people joining.': 'Короткий сигнал при упоминании и входе участников.',
  'Sound volume': 'Громкость звука', 'Flash the taskbar': 'Мигать на панели задач', 'While the window is in the background.': 'Когда окно свёрнуто или неактивно.',
  'Muted channels': 'Без звука', 'Muted': 'Звук отключён', '1 hour': '1 час', '8 hours': '8 часов', 'Until I turn it on': 'Пока не включу', 'Turn on': 'Включить звук',
  'Could not check for updates': 'Не удалось проверить обновления', 'The update was not installed': 'Не удалось установить обновление', 'Fingerprint copied': 'Отпечаток скопирован', 'Diagnostics copied': 'Диагностика скопирована',
  'Keys': 'Клавиши', 'Push to talk': 'Нажми, чтобы говорить', 'Held to speak; works in the background.': 'Удерживайте клавишу для разговора. Работает в фоне.',
  'Push-to-talk key': 'Клавиша активации микрофона', 'Press a key… (Esc cancels)': 'Нажмите клавишу… (Esc — отмена)', 'Choose key': 'Выбрать клавишу', 'Remove the key': 'Удалить клавишу',
  'Send the message': 'Отправить сообщение', 'New line': 'Новая строка', 'Cancel an edit, a reply or a dialog': 'Отменить изменение, ответ или закрыть диалог', 'Mention someone': 'Упомянуть пользователя',
  'About': 'О программе', 'Version': 'Версия', 'Updates': 'Обновления', 'You have the newest version': 'Установлена последняя версия', 'Check for updates': 'Проверить обновления',
  'Install {version}': 'Установить {version}', 'FinchSpeak {version} is available': 'Доступна FinchSpeak {version}', 'Install it from Settings → Keys and about.': 'Установите её в Настройки → Клавиши и о программе.', 'Leave the voice room to install.': 'Выйдите из голосовой комнаты, чтобы установить.', 'Server': 'Сервер', 'Certificate fingerprint (SHA-256)': 'Отпечаток сертификата (SHA-256)', 'Copy fingerprint': 'Скопировать отпечаток', 'Copy diagnostics': 'Скопировать диагностику',
  'unknown': 'неизвестно', 'Welcome to FinchSpeak': 'Добро пожаловать в FinchSpeak', 'Pick a channel on the left to start chatting or talking.': 'Выберите канал слева, чтобы начать переписку или разговор.',
  'Not authenticated': 'Вы не вошли в аккаунт',
  'Cancel': 'Отмена', 'System default': 'По умолчанию', ' (default)': ' (по умолчанию)',
  'This device is not connected; the system default is used instead.': 'Это устройство не подключено, используется устройство по умолчанию.',
  'Voice and audio': 'Голос и звук', 'Microphone': 'Микрофон', 'Speakers': 'Динамики', 'Voice enhancement': 'Улучшение голоса',
  'Balances quiet and loud speech and limits peaks.': 'Выравнивает тихую и громкую речь и ограничивает пиковую громкость.',
  'Input volume': 'Громкость входа', 'Speech threshold for sending': 'Порог речи для передачи', 'Microphone level': 'Уровень микрофона',
  'Voice activation: open': 'Голосовая активация: открыто', 'Voice activation: closed': 'Голосовая активация: закрыто',
  'You are muted: the meter is off.': 'Микрофон выключен: индикатор неактивен.',
  'The bar shows measured level. Activation also checks detected speech and push-to-talk; crossing the line alone does not open the microphone.': 'Полоса показывает измеренный уровень. Активация также учитывает распознанную речь и клавишу разговора: одного превышения порога недостаточно для включения микрофона.',
  'Audio diagnostics': 'Диагностика аудио',
  'Collects device names, audio configuration, and technical counters. No audio is recorded.': 'Собирает названия устройств, параметры аудио и технические счётчики. Запись звука не ведётся.',
  'Start / reset capture': 'Начать / сбросить сбор', 'Stop capture': 'Остановить сбор', 'Save report': 'Сохранить отчёт',
  'Start or reset audio diagnostics': 'Начать или сбросить диагностику аудио', 'Stop audio diagnostics': 'Остановить диагностику аудио', 'Save audio diagnostics report': 'Сохранить отчёт диагностики аудио',
  'Capturing audio diagnostics…': 'Сбор диагностики аудио…', 'Capture stopped. You can save the report.': 'Сбор остановлен. Можно сохранить отчёт.',
  'Audio diagnostics report saved.': 'Отчёт диагностики аудио сохранён.',
  'Recording 3 s…': 'Запись 3 с…', 'Playing back…': 'Воспроизведение…', 'Test microphone': 'Проверить микрофон',
  'Records three seconds with voice activation, then plays them back locally. Nobody else hears it.': 'Записывает три секунды с голосовой активацией и воспроизводит запись локально. Другие её не услышат.',
  'The microphone is open only while the key is held; it works while FinchSpeak is in the background.': 'Микрофон включён, только пока удерживается клавиша. Работает, даже когда FinchSpeak находится в фоне.',
  'Hear myself': 'Слышать себя', 'Plays processed microphone audio continuously, except during a microphone test.': 'Непрерывно воспроизводит обработанный звук микрофона, кроме времени проверки микрофона.',
  'Use headphones, or the speakers will feed back into the microphone.': 'Используйте наушники, иначе звук динамиков попадёт обратно в микрофон.', 'Done.': 'Готово.',
  'Talk. Share.': 'Говорите. Делитесь.', 'Chirp.': 'Чирикайте.', 'Your own voice rooms, chat and screen sharing, on a server you control.': 'Голосовые комнаты, переписка и демонстрация экрана на вашем сервере.',
  'FinchSpeak goldfinch with a headset': 'Золотистая птица FinchSpeak в наушниках', 'This account has been banned from the server': 'Этот аккаунт заблокирован на сервере',
  'Contact the server admin if you think this is a mistake.': 'Если вы считаете, что это ошибка, обратитесь к администратору сервера.', 'Invalid username or password': 'Неверное имя пользователя или пароль',
  'Check them and try again. Passwords are case-sensitive.': 'Проверьте данные и попробуйте снова. Пароль учитывает регистр.', 'Cannot reach the server': 'Не удаётся подключиться к серверу',
  'Check the address, your network and that the server is running.': 'Проверьте адрес и подключение к сети, а также убедитесь, что сервер работает.',
  'A valid invite code is required to register': 'Для регистрации нужен действующий код приглашения', 'Username is already taken': 'Это имя пользователя уже занято',
  'Server address': 'Адрес сервера', 'Invite code': 'Код приглашения', 'Ask the server admin for a code. It works a limited number of times and expires.': 'Попросите код у администратора сервера. У кода ограничено число использований и срок действия.',
  'Password': 'Пароль', 'Password strength: {strength}': 'Надёжность пароля: {strength}', 'Connecting securely…': 'Безопасное подключение…', 'Create account': 'Создать аккаунт',
  'Join the flock': 'Присоединиться к сообществу', 'Welcome back': 'С возвращением', 'Create your account on this server.': 'Создайте аккаунт на этом сервере.', 'Sign in to continue to your server.': 'Войдите, чтобы подключиться к серверу.',
  'Stay signed in': 'Не выходить из аккаунта', 'Retry': 'Повторить', 'Already have an account?': 'Уже есть аккаунт?', 'New here?': 'Впервые здесь?', 'Sign in': 'Войти', 'Create an account': 'Создать аккаунт', 'Encrypted with TLS · certificate pinned on first use': 'Зашифровано по TLS · сертификат закрепляется при первом подключении',
  'Welcome to #{channel}!': 'Добро пожаловать в #{channel}!', 'This is the start of the conversation. Say hello — someone is always listening.': 'Это начало переписки. Поздоровайтесь — вас услышат.',
  'New messages': 'Новые сообщения', 'Drop files to attach': 'Перетащите файлы для отправки', 'Up to 10 files, 10 MB each': 'До 10 файлов размером до 10 МБ каждый',
  'Starting FinchSpeak…': 'Запуск FinchSpeak…', 'Online — {count}': 'В сети — {count}', 'Admins — {count}': 'Администраторы — {count}', 'Moderators — {count}': 'Модераторы — {count}', 'Offline — {count}': 'Не в сети — {count}', 'Save': 'Сохранить', 'Create': 'Создать',
  'Reconnecting… try {attempt}, next one in {seconds} s': 'Повторное подключение… попытка {attempt}, следующая через {seconds} с', 'Reconnecting…': 'Повторное подключение…', 'Could not reconnect': 'Не удалось восстановить соединение', 'Connection lost': 'Соединение потеряно', 'Retry now': 'Повторить сейчас',
  'Messages': 'Сообщения', 'Loading older messages…': 'Загрузка старых сообщений…', 'select-channel': 'выбрать канал',
  'Keyboard shortcuts': 'Сочетания клавиш', 'Close': 'Закрыть', 'Jump to a channel': 'Перейти к каналу', 'Channels': 'Каналы', 'No channel like that.': 'Такого канала нет.', 'open': 'открыт', 'Back to sign in': 'Вернуться ко входу',
  'Show this list': 'Показать этот список', 'New line in a message': 'Новая строка в сообщении', 'Cancel an edit or reply, close a window': 'Отменить изменение или ответ, закрыть окно', 'Speak while it is held (set in Settings)': 'Говорить, пока клавиша нажата (настройка в профиле)',
  'You were disconnected by an admin': 'Администратор отключил вас', 'You can sign in again whenever you like.': 'Вы можете снова войти в любое время.', 'You have been banned from this server': 'Вас заблокировали на этом сервере',
  'You were signed out': 'Вы вышли из аккаунта', 'Your session was ended on another device or by an admin. Sign in again to continue.': 'Ваш сеанс завершён на другом устройстве или администратором. Войдите снова, чтобы продолжить.',
  'Your session has expired': 'Срок входа истёк', 'You have not used FinchSpeak on this computer for a long time. Sign in again.': 'Вы давно не заходили в FinchSpeak на этом компьютере. Войдите снова.',
  'Your password was changed': 'Пароль был изменён', 'Sign in with the new password.': 'Войдите с новым паролем.',
  'Today': 'Сегодня', 'Yesterday': 'Вчера', 'Search': 'Поиск', 'Search messages': 'Поиск сообщений', 'Clear search': 'Очистить поиск', 'Search results': 'Результаты поиска',
  'Searching…': 'Поиск…', '{count}{plus} found': 'Найдено: {count}{plus}', 'Only this channel': 'Только этот канал', 'All channels': 'Все каналы', 'Nothing found.': 'Ничего не найдено.',
  'It is in #{channel}; scroll up to find it.': 'Сообщение в канале #{channel}; прокрутите вверх, чтобы найти его.', 'Search failed': 'Не удалось выполнить поиск', 'That message is too far back': 'Сообщение находится слишком далеко в истории',
  'Replying to {name}: {content}': 'Ответ пользователю {name}: {content}', 'Cancel reply': 'Отменить ответ', 'Attachments': 'Вложения', 'Uploading…': 'Загрузка…',
  'Too large: {size}. The limit is 10 MB.': 'Файл слишком большой: {size}. Ограничение — 10 МБ.', 'Upload failed': 'Не удалось загрузить файл', 'Remove {name}': 'Удалить {name}',
  'Members': 'Участники', 'Members matching "{query}"': 'Участники по запросу «{query}»', 'Attach a file': 'Прикрепить файл', 'Message': 'Сообщение', 'You are offline. Reconnecting…': 'Нет подключения. Повторное подключение…',
  'Message #{channel}': 'Сообщение в канале #{channel}', 'channel': 'канал', '{count} over': 'Превышение на {count}', 'Emoji': 'Эмодзи', 'Send': 'Отправить',
  'This is the first time you connect to {host}.': 'Вы впервые подключаетесь к серверу {host}.', 'Certificate fingerprint · SHA-256': 'Отпечаток сертификата · SHA-256',
  'Compare these characters with the fingerprint the server admin sees in the Server page. If they match, nobody is listening in. FinchSpeak will warn you if this ever changes.': 'Сравните эти символы с отпечатком на странице «Сервер», который видит администратор. Если они совпадают, никто не подслушивает. FinchSpeak предупредит, если отпечаток изменится.',
  'They match — trust server': 'Совпадает — доверять серверу',
  'You trusted': 'Ранее доверенный', 'The server now presents': 'Сервер предъявляет', 'Ask the server admin to read out the fingerprint shown on the Server page. Only continue if it matches the red one above.': 'Попросите администратора прочитать отпечаток со страницы «Сервер». Продолжайте, только если он совпадает с красным выше.',
  'Copy details': 'Скопировать сведения', 'Disconnect — keep me safe': 'Отключиться для безопасности', 'Trust the new certificate': 'Доверять новому сертификату',
  'Voice room': 'Голосовая комната', '{count} in room': '{count} в комнате', 'Nobody is here yet.': 'Пока здесь никого нет.', '+ {count} more · scroll': '+ ещё {count} · прокрутите список',
  'People in the room': 'Участники комнаты', 'More people in the room': 'Остальные участники комнаты',
  'Member': 'Участник', 'Role': 'Роль', 'State': 'Состояние', 'Last seen': 'Последняя активность', 'Offline': 'Не в сети', 'Banned': 'Заблокирован', 'Kick': 'Исключить', 'Ban…': 'Заблокировать…', 'Unban': 'Разблокировать', 'now': 'сейчас',
  'You are sharing {label}': 'Вы показываете: {label}', 'your screen': 'свой экран', 'With system sound · 30 fps': 'Со звуком системы · 30 кадр/с', 'Without sound · 30 fps': 'Без звука · 30 кадр/с',
  'Weak connection': 'Нестабильное соединение', 'Stop watching': 'Прекратить просмотр', 'Back to the room': 'Вернуться в комнату', 'Drag the picture to move it. Everyone sees the part inside the circle.': 'Перетащите изображение, чтобы переместить его. Все увидят часть внутри круга.', 'Use this picture': 'Использовать это изображение',
  'Share your screen': 'Показать экран', 'Screens': 'Экраны', 'Windows': 'Окна', 'Refresh': 'Обновить', 'Could not start sharing': 'Не удалось начать демонстрацию', 'Main': 'Основной',
  'No screens found': 'Экраны не найдены', 'No windows to share': 'Нет окон для демонстрации', 'Check that a display is connected, then refresh.': 'Проверьте подключение дисплея и обновите список.', 'Minimised windows are not listed. Open the window you want and refresh.': 'Свёрнутые окна не отображаются. Откройте нужное окно и обновите список.',
  'Share system sound too': 'Передавать звук системы', 'Everything your speakers play goes out with the picture.': 'Вместе с изображением будет передаваться весь звук из динамиков.', "Other people's voices are part of that sound: they will hear themselves with a small delay. Use headphones.": 'Голоса других участников также будут передаваться с небольшой задержкой. Используйте наушники.', '30 frames per second; pictures larger than 4K are scaled down before they are sent.': '30 кадров в секунду; изображение выше 4K уменьшается перед отправкой.',
  'System sound is shared': 'Звук системы передаётся', 'System sound is not shared': 'Звук системы не передаётся', 'Volume of the shared sound': 'Громкость передаваемого звука', 'Picture in picture': 'Картинка в картинке', 'Leave full screen': 'Выйти из полноэкранного режима', 'Full screen': 'Полноэкранный режим',
  '{name} stopped sharing': '{name} прекратил демонстрацию', 'The room is still going. You can keep talking.': 'Комната продолжает работать. Вы можете общаться дальше.', '{name} left the room': '{name} покинул комнату', 'Their screen share ended with the call.': 'Демонстрация экрана завершилась вместе со звонком.', '{name} lost the connection': 'У {name} пропало соединение', 'Their screen share ended. It starts again if they come back and share.': 'Демонстрация экрана завершилась. Она продолжится, если участник вернётся и запустит её снова.', 'The voice room was closed': 'Голосовая комната закрыта', 'An admin deleted this room, so the screen share ended.': 'Администратор удалил комнату, поэтому демонстрация экрана завершилась.',
  'Server details': 'Сведения о сервере', 'Start sharing': 'Начать демонстрацию', 'Stop sharing': 'Остановить демонстрацию',
  'Rename channel': 'Переименовать канал', 'New voice channel': 'Новый голосовой канал', 'New text channel': 'Новый текстовый канал', 'Channel name': 'Название канала', 'Quiet corner': 'Тихий уголок', 'general': 'общий',
  'Text channels': 'Текстовые каналы', 'Create text channel': 'Создать текстовый канал', 'Voice channels': 'Голосовые каналы', 'Create voice channel': 'Создать голосовой канал',
  'Voice connected': 'Голос подключён', 'Voice disconnected': 'Голос отключён', 'Unmute': 'Включить микрофон', 'Mute': 'Выключить микрофон', 'Undeafen': 'Включить звук', 'Deafen': 'Выключить звук',
  'Profile and settings': 'Профиль и настройки', 'Administration': 'Администрирование', 'Move up': 'Переместить вверх', 'Move down': 'Переместить вниз', 'Rename': 'Переименовать', 'Delete': 'Удалить',
  'Move {channel} up': 'Переместить канал {channel} вверх', 'Move {channel} down': 'Переместить канал {channel} вниз', 'Rename {channel}': 'Переименовать канал {channel}', 'Delete {channel}': 'Удалить канал {channel}',
  'You were mentioned': 'Вас упомянули', 'Unread messages': 'Непрочитанные сообщения', 'Deafened': 'Звук выключен', 'Sharing screen': 'Показывает экран', 'Disconnect from voice': 'Отключиться от голосовой комнаты', 'Disconnect': 'Отключиться',
  'Trust this server?': 'Доверять этому серверу?', "This server's certificate changed": 'Сертификат сервера изменился',
  'The certificate of {host} is not the one you trusted before. This can happen after a server reinstall — or when someone is intercepting your connection. Your password was not sent.': 'Сертификат сервера {host} отличается от ранее доверенного. Это возможно после переустановки сервера или при перехвате соединения. Ваш пароль не отправлялся.',
  'To continue anyway, type the server name': 'Чтобы продолжить, введите имя сервера',
  'never': 'никогда', 'just now': 'только что', '{count} min ago': '{count} мин. назад', '{count} h ago': '{count} ч назад', '{count} d ago': '{count} дн. назад',
  'Banned until {when}': 'Заблокирован до {when}', 'Ban {name}': 'Заблокировать {name}', 'For how long? Admins cannot be banned: make them a member first.': 'На какой срок? Администраторов нельзя блокировать: сначала понизьте их до участника.', '{action}: {name}?': '{action}: {name}?',
  'People need a code to register. Give it to them yourself; it stops working after its uses or time run out.': 'Для регистрации нужен код. Передайте его пользователю лично; код перестанет работать после указанного числа использований или срока.',
  'Uses': 'Использований', 'Valid for (hours)': 'Срок действия (часы)', 'Create invitation': 'Создать приглашение', 'New code': 'Новый код', 'Copy': 'Копировать', 'Code': 'Код', 'Used': 'Использован', 'Expires': 'Истекает', 'No invitations yet.': 'Приглашений пока нет.', 'Revoke': 'Отозвать',
  'The last text channel cannot be deleted': 'Нельзя удалить последний текстовый канал', 'Create channels with the + next to the channel lists.': 'Создавайте каналы кнопкой + рядом со списком каналов.', 'All messages and files in this channel will be removed. People in a deleted voice channel are sent out.': 'Все сообщения и файлы в этом канале будут удалены. Участники удалённой голосовой комнаты будут отключены.', 'Delete channel': 'Удалить канал',
  'Running for': 'Работает', 'Ports': 'Порты', 'Uploaded files': 'Загруженные файлы', 'Certificate fingerprint': 'Отпечаток сертификата', 'Share this with people so they can compare it when their app asks them to trust the server.': 'Поделитесь отпечатком, чтобы участники могли сравнить его при подтверждении доверия к серверу.', 'Anyone can register': 'Регистрация открыта всем', 'Off: a registration needs an invitation code. Takes effect at once.': 'Если выключено, для регистрации нужен код приглашения. Изменение действует сразу.', 'Backups': 'Резервные копии', '{count} kept · last: {when} ({size})': 'Сохранено копий: {count} · последняя: {when} ({size})', 'Folder: {backup} · Logs: {logs}': 'Папка: {backup} · Журналы: {logs}', 'Back up now': 'Создать резервную копию', 'Nothing has been logged yet.': 'Журнал пока пуст.', 'Show older': 'Показать более ранние', 'Administration sections': 'Разделы администрирования',
  'The picture could not be loaded.': 'Не удалось загрузить изображение.', 'Download': 'Скачать', 'Search emoji': 'Поиск эмодзи', 'Nothing found': 'Ничего не найдено',
  'Message actions': 'Действия с сообщением', 'Add reaction': 'Добавить реакцию', 'Reply': 'Ответить', 'Copy text': 'Скопировать текст', 'Edit message': 'Изменить сообщение', 'Edit': 'Изменить', 'Delete message': 'Удалить сообщение', 'sending…': 'отправка…', 'Not sent': 'Не отправлено', 'Edited': 'Изменено', 'edited': 'изменено', 'Original message deleted': 'Исходное сообщение удалено', 'Esc to cancel · Enter to save': 'Esc — отменить · Enter — сохранить', '{names} and {count} others': '{names} и ещё {count}', 'Delete this message?': 'Удалить это сообщение?', 'It disappears for everyone. This cannot be undone.': 'Сообщение исчезнет у всех. Это действие нельзя отменить.',
  'Loading messages': 'Загрузка сообщений', 'Scroll up to load older messages.': 'Прокрутите вверх, чтобы загрузить старые сообщения.', 'Position your picture': 'Расположение изображения', 'Preview of your picture': 'Предпросмотр изображения', 'Zoom': 'Масштаб', 'Settings sections': 'Разделы настроек', 'Open profile': 'Открыть профиль', 'Done': 'Готово',
  'At most {count} files fit in one message': 'К одному сообщению можно прикрепить не более {count} файлов', 'Message not sent: {error}': 'Не удалось отправить сообщение: {error}', 'The new passwords do not match': 'Новые пароли не совпадают', 'Screen sound is not shared: {reason}': 'Звук экрана не передаётся: {reason}', 'Screen sharing stopped: {reason}': 'Демонстрация экрана остановлена: {reason}', 'A moderator sent you out. You can join again.': 'Модератор отключил вас. Вы можете присоединиться снова.',
  'An audio device was disconnected; reopening the audio devices...': 'Аудиоустройство отключилось. Повторное открытие аудиоустройств…', 'Voice processing stopped: {message}. Rejoin the voice channel to retry.': 'Обработка голоса остановлена: {message}. Переподключитесь к голосовому каналу, чтобы повторить попытку.', 'The audio devices keep failing. Check them, then rejoin the voice channel.': 'Аудиоустройства продолжают сбоить. Проверьте их и переподключитесь к голосовому каналу.', '"{wanted}" is not available; using the system default {kind} device instead.': 'Устройство «{wanted}» недоступно; используется системное устройство {kind} по умолчанию.', 'input': 'входное', 'output': 'выходное',
  'Voice controls': 'Управление голосом', 'Voice settings': 'Настройки голоса', 'Leave the room': 'Покинуть комнату', 'Live': 'В эфире', 'is sharing a screen': 'демонстрирует экран', 'Watch': 'Смотреть', 'Watching': 'Просмотр', 'Silenced by a moderator': 'Модератор отключил микрофон', 'Good connection': 'Хорошее соединение', 'Fair connection': 'Среднее соединение', 'Poor connection': 'Плохое соединение', '(you)': '(вы)', 'Speaking': 'Говорит', 'Allow to speak': 'Разрешить говорить', 'Silence': 'Отключить микрофон', 'Volume of {name}': 'Громкость: {name}', 'talking': 'говорит', 'push to talk': 'нажмите, чтобы говорить',
  'Minimize': 'Свернуть', 'Maximize': 'Развернуть', 'Restore': 'Восстановить', 'Dismiss': 'Закрыть уведомление',
  'That message is further up': 'Это сообщение находится выше', 'You were removed from the voice room': 'Вас отключили от голосовой комнаты', 'Full screen is not available': 'Полноэкранный режим недоступен', 'Picture-in-picture is not available': 'Режим «картинка в картинке» недоступен', 'Details copied': 'Сведения скопированы', 'Code copied': 'Код скопирован', 'Backup made': 'Резервная копия создана',
  '{name} is disconnected and cannot sign in for {duration}.': '{name} отключён и не сможет войти. Срок блокировки: {duration}.', '{name} is disconnected and cannot sign in until you lift the ban.': '{name} отключён и не сможет войти, пока вы не снимете блокировку.',
  'Could not join voice: {error}': 'Не удалось подключиться к голосовому каналу: {error}', 'Could not switch audio device: {error}': 'Не удалось переключить аудиоустройство: {error}', 'Choose a push-to-talk key first': 'Сначала выберите клавишу активации микрофона',
  'excellent': 'отличное', '{count} ms · {quality}': '{count} мс · {quality}',
  'good': 'хорошее', 'fair': 'среднее', 'poor': 'плохое',
  'Ban': 'Заблокировать', '1 day': '1 день', '1 week': '1 неделя', 'For good': 'Навсегда', 'a while': 'некоторое время',
  'kicked {target}': 'исключил {target}', 'banned {target}': 'заблокировал {target}', 'banned {target} for a while': 'заблокировал {target} на время', 'lifted the ban of {target}': 'снял блокировку: {target}',
  'changed the role of {target} ({details})': 'изменил роль {target} ({details})', 'deleted a message of someone else': 'удалил чужое сообщение', 'created the channel {target}': 'создал канал {target}', 'deleted the channel {target}': 'удалил канал {target}',
  'created the invitation {target}': 'создал приглашение {target}', 'revoked the invitation {target}': 'отозвал приглашение {target}', 'set registration to {details}': 'установил регистрацию: {details}', 'made a backup': 'сделал резервную копию',
  'sent {target} out of the voice room': 'отключил {target} от голосовой комнаты', 'silenced {target} in the voice room': 'заглушил {target} в голосовой комнате', 'un-silenced {target} in the voice room': 'разрешил говорить {target} в голосовой комнате',
  'Change role': 'Изменить роль', 'Confirm': 'Подтвердить', 'Active': 'Действует', 'Expired': 'Истёк', 'Used up': 'Исчерпан',
  '{name} is disconnected and has to sign in again. They can come back.': '{name} отключён и должен войти снова. Вернуться можно в любой момент.',
  '{name} can sign in again.': '{name} снова сможет войти.',
  '{name} gets every power an admin has, including banning and changing roles.': '{name} получает все права администратора, включая блокировку и смену ролей.',
  '{name} can kick people, silence them in voice and delete messages.': '{name} сможет исключать людей, отключать им микрофон в голосовых комнатах и удалять сообщения.',
  '{name} loses their staff powers.': '{name} лишится прав модератора и администратора.',
  'Delete {name}?': 'Удалить канал {name}?', 'The sharer': 'Автор показа',
  'Download {name}': 'Скачать {name}', 'Open {name}': 'Открыть {name}', 'No preview': 'Нет предпросмотра', 'You': 'Вы',
  'Delete #{channel}?': 'Удалить канал #{channel}?', 'Download failed: {error}': 'Не удалось скачать файл: {error}',
  'Name changed.': 'Имя изменено.', 'Password changed. Other devices are signed out.': 'Пароль изменён. На других устройствах выполнен выход.',
  'Use a PNG, JPEG or WebP picture': 'Выберите изображение в формате PNG, JPEG или WebP', 'That picture is too large. Pick one under 12 MB': 'Изображение слишком большое. Выберите файл размером до 12 МБ', 'That picture could not be opened. Try another file': 'Не удалось открыть изображение. Попробуйте другой файл', 'The picture could not be prepared': 'Не удалось подготовить изображение', 'The picture must be smaller than 2 MB': 'Размер изображения должен быть меньше 2 МБ',
  'Invitations': 'Приглашения', 'Log': 'Журнал', 'Moderator': 'Модератор',
  'Weak': 'Слабый', 'Fair': 'Средний', 'Good': 'Хороший', 'Strong': 'Сильный',
  'Show password': 'Показать пароль', 'Hide password': 'Скрыть пароль',
  'Someone': 'Кто-то', '{name} is typing…': '{name} печатает…', '{first} and {second} are typing…': '{first} и {second} печатают…', 'Several people are typing…': 'Несколько человек печатают…',
  'Waiting for the first picture…': 'Ожидаем первое изображение…', "Asking {name}'s computer for a full frame. Usually under a second.": 'Запрашиваем полный кадр у компьютера пользователя {name}. Обычно это занимает меньше секунды.',
  'Catching up…': 'Восстанавливаем поток…', 'Some packets were lost; waiting for a clean frame.': 'Часть пакетов потерялась. Ожидаем целый кадр.',
  'Restoring the picture…': 'Восстанавливаем изображение…', 'A frame could not be decoded; asking for a fresh one.': 'Не удалось декодировать кадр. Запрашиваем новый.',
  'The picture resumes automatically when you are back.': 'Изображение вернётся автоматически после восстановления соединения.',
  'This computer cannot play the video': 'Этот компьютер не может воспроизвести видео',
  'Its graphics driver has no H.264 decoder. Voice still works. Update the graphics driver, or ask for a window share at lower quality.': 'В графическом драйвере нет декодера H.264. Голос продолжает работать. Обновите драйвер или попросите показать окно в более низком качестве.',
  "{name}'s shared screen": 'Демонстрация экрана: {name}',
  'Unmute yourself first': 'Сначала включите микрофон',
  'Spatial audio': 'Объёмный звук', 'Spatial audio: where everyone sounds from': 'Объёмный звук: откуда слышно каждого',
  'Everyone in the room is heard from their own side. Best with headphones; drag people on the stage in the room to move them.': 'Каждого собеседника слышно со своей стороны. Лучше всего в наушниках. Чтобы переставить человека, перетащите его на сцене в комнате.',
  'Spread out again': 'Расставить заново', 'Spread everyone out again': 'Расставить всех заново',
  '{name}: drag or use the arrow keys to move left or right': '{name}: перетащите или нажмите стрелки, чтобы сдвинуть влево или вправо',
  'left': 'слева', 'right': 'справа', 'centre': 'по центру', 'Double-click to put back': 'Двойной щелчок вернёт на место', 'you': 'вы',
  'When someone joins, you will hear them from their place here.': 'Когда кто-нибудь зайдёт, его будет слышно с его места на сцене.',
  'My colors': 'Своя', 'From a picture': 'Из картинки', 'Make a theme from a picture': 'Сделать тему из картинки', 'Reading colors…': 'Подбираем цвета…', 'That is not a picture.': 'Это не картинка.', 'Could not read this picture.': 'Не удалось прочитать картинку.', 'Start from a palette': 'Начать с готовой палитры', 'Colors': 'Цвета', 'Reset colors': 'Сбросить цвета',
  'Ember': 'Угли', 'Ocean': 'Океан', 'Forest': 'Лес', 'Sakura': 'Сакура', 'Amethyst': 'Аметист', 'Paper': 'Бумага',
  'Background': 'Фон', 'Chat and main area': 'Чат и основная область', 'Side panels': 'Боковые панели', 'Channels and members': 'Каналы и участники',
  'Cards and fields': 'Карточки и поля', 'Inputs, menus, the message box': 'Поля ввода, меню, поле сообщения', 'Text': 'Текст', 'Messages and labels': 'Сообщения и подписи',
  'Accent': 'Акцент', 'Buttons and selected items': 'Кнопки и выбранные элементы', 'Signal': 'Сигнал', 'Speaking, online, live': 'Говорит, в сети, эфир',
  '{name} as a hex code': '{name}: код цвета',
  'The text is hard to read on these backgrounds. Make it lighter or the backgrounds darker (or the other way round).': 'Текст плохо читается на таком фоне. Сделайте текст светлее, а фон темнее (или наоборот).',
  'Buttons blend into the background. Pick a brighter or darker accent.': 'Кнопки сливаются с фоном. Выберите акцент поярче или потемнее.',
  'Mute microphone': 'Выключить микрофон', 'Turn sound off': 'Выключить звук',
  'Mute microphone key': 'Клавиша выключения микрофона', 'Turn sound off key': 'Клавиша выключения звука',
  'Pressed to mute or unmute; works in the background.': 'Нажатие включает или выключает микрофон. Работает в фоне.',
  'Pressed to turn all sound off or on; works in the background.': 'Нажатие включает или выключает весь звук. Работает в фоне.',
  'Could not use {key} for "{action}": another program may be using it.': 'Не удалось назначить {key} на «{action}»: возможно, клавиша занята другой программой.',
  '"{key}" cannot be used as a key: {detail}': '«{key}» нельзя использовать как клавишу: {detail}',
  'Could not use "{key}" as a key: {detail}': 'Не удалось использовать «{key}» как клавишу: {detail}',
  '"{key}" is already used by another action': 'Клавиша «{key}» уже используется для другого действия',
  'Keys are unavailable': 'Клавиши сейчас недоступны',
  'L': 'Л', 'R': 'П',
  '"{key}" is already used for "{action}"': 'Клавиша «{key}» уже назначена на «{action}»',
  'Voice audio is not running. Rejoin the voice channel to retry.': 'Звук звонка не работает. Переподключитесь к голосовой комнате, чтобы попробовать снова.',
  'The microphone test needs a working microphone and speakers': 'Для проверки нужны работающие микрофон и динамики',
  'An audio device was disconnected during the microphone test': 'Во время проверки отключилось аудиоустройство',  'The old certificate was forgotten. Sign in again and compare the new fingerprint.': 'Старый сертификат удалён из доверенных. Войдите ещё раз и сравните новый отпечаток.',
  'a channel': 'канале', '{name} mentioned you in #{channel}': '{name} упомянул вас в канале #{channel}', '{name} in #{channel}': '{name} в канале #{channel}', 'your voice room': 'вашей голосовой комнате', '{name} joined {room}': '{name} присоединился к комнате {room}',
  'Cannot reach the server: {reason}': 'Сервер недоступен: {reason}',
  'Signed in, but the voice connection failed. Check that UDP port 4433 is open.': 'Вход выполнен, но голосовое соединение не установилось. Проверьте, открыт ли UDP-порт 4433.',
  'Registered, but the voice connection failed. Check that UDP port 4433 is open.': 'Регистрация прошла, но голосовое соединение не установилось. Проверьте, открыт ли UDP-порт 4433.',
  'Registration failed: {reason}': 'Не удалось зарегистрироваться: {reason}',
  'Sign-in failed: {reason}': 'Не удалось войти: {reason}',
  'Confirm the server certificate, then sign in.': 'Подтвердите сертификат сервера, затем войдите.',
  'The server certificate has changed. Do not sign in until the admin confirms the change.': 'Сертификат сервера изменился. Не входите, пока администратор не подтвердит замену.',
  'Signed out on this computer. The server could not be reached to end the session.': 'Вы вышли на этом компьютере. Сервер недоступен, поэтому сессия на нём не завершена.',
  'The server address is not valid': 'Неверный адрес сервера',
};

/** Russian plural forms, keyed by the English `other` form that `plural()` is called with. */
export const ruPlurals: Readonly<Record<string, Partial<Record<Intl.LDMLPluralRule, string>> & { other: string }>> = {
  '{count} new messages': { one: '{count} новое сообщение', few: '{count} новых сообщения', many: '{count} новых сообщений', other: '{count} новых сообщения' },
};
const pluralRules = { ru: new Intl.PluralRules('ru-RU'), en: new Intl.PluralRules('en-GB') };

export const locale = () => prefs().appearance.language;

/** Format visible numbers using the selected interface locale. */
export function formatNumber(value: number, maximumFractionDigits?: number): string {
  return new Intl.NumberFormat(locale() === 'ru' ? 'ru-RU' : 'en-GB', maximumFractionDigits === undefined ? undefined : { maximumFractionDigits }).format(value);
}

const UNITS = {
  en: { B: 'B', KB: 'KB', MB: 'MB', GB: 'GB', kbps: 'kbps', Mbps: 'Mbps', fps: 'fps', min: 'min', h: 'h', d: 'd' },
  ru: { B: 'Б', KB: 'КБ', MB: 'МБ', GB: 'ГБ', kbps: 'кбит/с', Mbps: 'Мбит/с', fps: 'кадр/с', min: 'мин', h: 'ч', d: 'д' },
} as const;
export type UnitName = keyof typeof UNITS.en;

/** The short label of a unit in the interface language (KB / КБ, kbps / кбит/с, h / ч …). */
export const unit = (name: UnitName): string => UNITS[locale() === 'ru' ? 'ru' : 'en'][name];

/** A size in bytes as "500 B", "2 KB", "5.5 MB", "3 GB". With `smallest: 'KB'` anything under a megabyte reads in kilobytes (at least 1). */
export function formatBytes(bytes: number, smallest: 'B' | 'KB' = 'B'): string {
  if (bytes < 1024 && smallest === 'B') return `${formatNumber(bytes)} ${unit('B')}`;
  if (bytes < 1024 * 1024) return `${formatNumber(Math.max(1, Math.round(bytes / 1024)))} ${unit('KB')}`;
  if (bytes < 1024 ** 3) return `${formatNumber(bytes / 1024 / 1024, 1)} ${unit('MB')}`;
  return `${formatNumber(bytes / 1024 ** 3, 2)} ${unit('GB')}`;
}

/** A video bitrate given in kilobits per second, as "850 kbps" or "2.5 Mbps". */
export function formatBitrate(kbps: number): string {
  return kbps >= 1000 ? `${formatNumber(kbps / 1000, 1)} ${unit('Mbps')}` : `${formatNumber(kbps)} ${unit('kbps')}`;
}

/** A length of time in whole seconds as "5 min", "2 h" or "1 d 2 h" (the unit labels follow the interface language). */
export function formatDuration(seconds: number): string {
  if (seconds < 3600) return `${Math.floor(seconds / 60)} ${unit('min')}`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} ${unit('h')}`;
  return `${Math.floor(seconds / 86400)} ${unit('d')} ${Math.floor((seconds % 86400) / 3600)} ${unit('h')}`;
}

function fill(template: string, params?: TranslationParams): string {
  if (!params) return template;
  return template.replace(/\{([^{}]+)\}/g, (match, key: string) => {
    if (!(key in params)) return match;
    const value = params[key];
    return typeof value === 'number' ? formatNumber(value) : value;
  });
}

/** Translate an English source string reactively and interpolate named values. */
export function t(source: string, params?: TranslationParams): string {
  return fill(locale() === 'ru' ? (ru[source] ?? source) : source, params);
}

/**
 * A counted phrase in the right plural form for the interface language: `plural(n, { one: '{count} file', other: '{count} files' })`.
 * English uses the two forms given; Russian takes one / few / many from `ruPlurals`, found by the `other` form (a phrase without
 * an entry there falls back to the English `other` form). `{count}` is filled in with the formatted number.
 */
export function plural(count: number, forms: { one: string; other: string }, params?: TranslationParams): string {
  const values = { ...params, count };
  if (locale() === 'ru') {
    const table = ruPlurals[forms.other];
    return table ? fill(table[pluralRules.ru.select(count)] ?? table.other, values) : fill(forms.other, values);
  }
  return fill(pluralRules.en.select(count) === 'one' ? forms.one : forms.other, values);
}
