/** The six reactions shown first. */
export const QUICK_REACTIONS = ['👍', '❤️', '😂', '🎉', '😮', '😢'];

/** Emoji for the picker, with words to search by. Plain characters: no image files to ship. */
export const EMOJI: { char: string; words: string }[] = [
  { char: '😀', words: 'smile grin happy' }, { char: '😅', words: 'sweat relief nervous' }, { char: '😂', words: 'laugh joy lol' },
  { char: '🤣', words: 'rofl laugh floor' }, { char: '😊', words: 'blush smile' }, { char: '😍', words: 'love heart eyes' },
  { char: '🥰', words: 'love adore' }, { char: '😘', words: 'kiss' }, { char: '😎', words: 'cool sunglasses' },
  { char: '🤔', words: 'think hmm' }, { char: '🙄', words: 'eyeroll' }, { char: '😴', words: 'sleep tired' },
  { char: '😢', words: 'cry sad tear' }, { char: '😭', words: 'sob cry' }, { char: '😡', words: 'angry mad' },
  { char: '😮', words: 'wow surprise' }, { char: '😱', words: 'scream shock' }, { char: '🤯', words: 'mind blown' },
  { char: '🥳', words: 'party celebrate' }, { char: '😇', words: 'angel innocent' }, { char: '🤗', words: 'hug' },
  { char: '🤝', words: 'handshake deal' }, { char: '👍', words: 'thumbs up yes ok like' }, { char: '👎', words: 'thumbs down no dislike' },
  { char: '👏', words: 'clap applause' }, { char: '🙌', words: 'hooray hands' }, { char: '🙏', words: 'pray thanks please' },
  { char: '💪', words: 'strong muscle' }, { char: '👀', words: 'eyes look' }, { char: '👋', words: 'wave hello bye' },
  { char: '✌️', words: 'peace victory' }, { char: '🤞', words: 'fingers crossed luck' }, { char: '❤️', words: 'heart love red' },
  { char: '💛', words: 'yellow heart' }, { char: '💙', words: 'blue heart' }, { char: '💜', words: 'purple heart' },
  { char: '💔', words: 'broken heart' }, { char: '🔥', words: 'fire hot lit' }, { char: '✨', words: 'sparkles magic' },
  { char: '💯', words: 'hundred perfect' }, { char: '⭐', words: 'star' }, { char: '🎉', words: 'tada party confetti' },
  { char: '🎊', words: 'confetti' }, { char: '🎁', words: 'gift present' }, { char: '🎂', words: 'cake birthday' },
  { char: '🍕', words: 'pizza food' }, { char: '🍔', words: 'burger food' }, { char: '🍟', words: 'fries chips' },
  { char: '🍿', words: 'popcorn snacks' }, { char: '☕', words: 'coffee tea' }, { char: '🍺', words: 'beer cheers' },
  { char: '🎮', words: 'game controller gaming' }, { char: '🕹️', words: 'joystick arcade' }, { char: '🎧', words: 'headphones music' },
  { char: '🎵', words: 'music note' }, { char: '🎤', words: 'microphone sing' }, { char: '🎬', words: 'movie film' },
  { char: '💻', words: 'laptop computer code' }, { char: '🖥️', words: 'desktop screen' }, { char: '📱', words: 'phone mobile' },
  { char: '📎', words: 'paperclip attach' }, { char: '📌', words: 'pin' }, { char: '🔔', words: 'bell notification' },
  { char: '🔒', words: 'lock secure' }, { char: '🔑', words: 'key' }, { char: '🚀', words: 'rocket launch ship' },
  { char: '🏆', words: 'trophy win' }, { char: '🥇', words: 'gold medal first' }, { char: '⚡', words: 'lightning fast' },
  { char: '🌈', words: 'rainbow' }, { char: '☀️', words: 'sun' }, { char: '🌙', words: 'moon night' },
  { char: '🐦', words: 'bird finch' }, { char: '🐱', words: 'cat' }, { char: '🐶', words: 'dog' },
  { char: '🦊', words: 'fox' }, { char: '🐼', words: 'panda' }, { char: '🦄', words: 'unicorn' },
  { char: '✅', words: 'check done yes' }, { char: '❌', words: 'cross no wrong' }, { char: '⚠️', words: 'warning' },
  { char: '❓', words: 'question' }, { char: '❗', words: 'exclamation important' }, { char: '💤', words: 'zzz sleep' },
  { char: '🤖', words: 'robot bot' }, { char: '👻', words: 'ghost' }, { char: '💩', words: 'poop' },
  { char: '🫡', words: 'salute' }, { char: '🫶', words: 'heart hands' }, { char: '🥲', words: 'tear smile proud' },
];

export function searchEmoji(query: string): typeof EMOJI {
  const needle = query.trim().toLowerCase();
  if (!needle) return EMOJI;
  return EMOJI.filter((entry) => entry.words.includes(needle) || entry.char === needle);
}
