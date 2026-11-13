import { render } from 'solid-js/web';
import { t } from '../../i18n';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { DEFAULT_PREFS, setPrefs } from '../../stores/settingsStore';
import { Dock } from './Dock';

let dispose: (() => void) | undefined;

beforeEach(() => {
  setPrefs(DEFAULT_PREFS);
  const root = document.createElement('div');
  document.body.append(root);
  dispose = render(() => <Dock onShare={() => {}} onSettings={() => {}} />, root);
});

afterEach(() => {
  dispose?.();
  setPrefs(DEFAULT_PREFS);
  document.body.innerHTML = '';
});

const miniWindowButton = () => document.querySelector<HTMLButtonElement>(`button[aria-label="${t('Mini window')}"]`);

it('the mini window button is in the dock while the mini window is on', () => {
  expect(miniWindowButton()).not.toBeNull();
});

it('turned off, the mini window button leaves the dock', () => {
  setPrefs({ ...DEFAULT_PREFS, overlay: { ...DEFAULT_PREFS.overlay, enabled: false } });
  expect(miniWindowButton()).toBeNull();
});
