/* @refresh reload */
import { render } from 'solid-js/web';
import './styles/finch.css';
import './styles/app.css';
import './styles/custom-tokens.css';
import './styles/themes.css';
import './styles/motion.css';
import './styles/polish.css';
import './styles/user-theme.css';
import './styles/spatial.css';
import './styles/nest.css';
import './styles/music.css';
import './styles/roomTools.css';
import './styles/soundboard.css';
import './styles/voiceNote.css';

const root = document.getElementById('root');

if (import.meta.env.DEV && !(root instanceof HTMLElement)) {
  throw new Error(
    'Root element not found. Did you forget to add it to your index.html? Or maybe the id attribute got misspelled?',
  );
}

// The mini window opens this same bundle with ?view=overlay: it gets only its own view, none of the app's stores.
if (import.meta.env.DEV && new URLSearchParams(window.location.search).has('qa')) {
  // The visual-audit entry: fixture screens on the dev server only; the branch is removed from the production build.
  void import('./qa/qa').then(({ mountQa }) => mountQa(root!));
} else if (new URLSearchParams(window.location.search).get('view') === 'overlay') {
  document.documentElement.dataset.view = 'overlay';
  void import('./components/overlay/OverlayView').then(({ OverlayWindow }) => render(() => <OverlayWindow />, root!));
} else {
  void import('./App').then(({ App }) => render(() => <App />, root!));
  void import('./stores/overlayStore').then(({ startOverlaySync }) => startOverlaySync());
}
