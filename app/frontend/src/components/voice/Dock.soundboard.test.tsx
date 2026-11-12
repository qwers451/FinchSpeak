import { render } from 'solid-js/web';
import { t } from '../../i18n';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { setServerFeatures } from '../../stores/sessionStore';
import { setSoundboardOpen, soundboardOpen } from '../../stores/soundboardStore';
import { Dock } from './Dock';

let dispose: (() => void) | undefined;

beforeEach(() => {
  const root = document.createElement('div');
  document.body.append(root);
  dispose = render(() => <Dock onShare={() => {}} onSettings={() => {}} />, root);
});

afterEach(() => {
  dispose?.();
  setServerFeatures([]);
  setSoundboardOpen(false);
  document.body.innerHTML = '';
});

const soundboardButton = () => document.querySelector<HTMLButtonElement>(`button[aria-label="${t('Soundboard')}"]`);

it('shows the soundboard button only when the server announces the feature', () => {
  expect(soundboardButton()).toBeNull();
  setServerFeatures(['soundboard']);
  expect(soundboardButton()).not.toBeNull();
});

it('the button opens and closes the board', () => {
  setServerFeatures(['soundboard']);
  soundboardButton()?.click();
  expect(soundboardOpen()).toBe(true);
  soundboardButton()?.click();
  expect(soundboardOpen()).toBe(false);
});
