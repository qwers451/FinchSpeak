import { createMemo, createSignal, For, Show } from 'solid-js';
import { pendingMismatch, pendingTofu, setAuthError, setPendingMismatch, setPendingTofu } from '../stores/authStore';
import { api } from '../services/api';
import { Button, Field, Icon, Modal } from '../ui';
import { t } from '../i18n';

/** `AA:BB:CC:…` as groups of eight bytes, one group per line. */
export function fingerprintGroups(fingerprint: string): string[] {
  const bytes = fingerprint.split(':').filter(Boolean);
  const groups: string[] = [];
  for (let i = 0; i < bytes.length; i += 8) groups.push(bytes.slice(i, i + 8).join(':'));
  return groups;
}

/** Which byte pairs differ between two fingerprints, for the red highlight in the warning. */
export function diffBytes(expected: string, actual: string): { text: string; differs: boolean }[] {
  const a = expected.split(':');
  return actual.split(':').map((byte, i) => ({ text: byte, differs: a[i] !== byte }));
}

const hostOf = (hostname: string) => hostname;

/** First connection to a server (A3): the user compares the fingerprint, no default button. */
export function TofuModal() {
  const confirm = async () => {
    const data = pendingTofu();
    if (!data) return;
    await api.trustFingerprint(data.hostname, data.fingerprint);
    setPendingTofu(null);
    setAuthError(null);
  };

  return (
    <Modal open={!!pendingTofu()} onClose={() => setPendingTofu(null)} title={t('Trust this server?')} width="560px">
      <Show when={pendingTofu()}>
        {(data) => (
          <div class="col g20">
            <div class="row g16">
              <div class="av s56 c3"><Icon name="shield" size="lg" /></div>
              <span class="t2">{t('This is the first time you connect to {host}.', { host: hostOf(data().hostname) })}</span>
            </div>
            <div class="col g8">
              <span class="sec-label">{t('Certificate fingerprint · SHA-256')}</span>
              <div class="fp-grid" data-testid="fingerprint">
                <For each={fingerprintGroups(data().fingerprint)}>{(group) => <span>{group}</span>}</For>
              </div>
            </div>
            <div class="auth-banner info">
              <Icon name="info" class="tc" />
              <span class="fs13">{t('Compare these characters with the fingerprint the server admin sees in the Server page. If they match, nobody is listening in. FinchSpeak will warn you if this ever changes.')}</span>
            </div>
            <div class="row g12" style={{ 'justify-content': 'flex-end' }}>
              <Button variant="secondary" onClick={() => setPendingTofu(null)}>{t('Cancel')}</Button>
              <Button variant="violet" icon="check" onClick={() => void confirm()}>{t('They match — trust server')}</Button>
            </div>
          </div>
        )}
      </Show>
    </Modal>
  );
}

/** A trusted server presents another certificate (A4): loud, and the safe choice is the primary one. */
export function CertChangedModal() {
  const [typed, setTyped] = createSignal('');
  const data = pendingMismatch;
  const matches = createMemo(() => !!data() && typed().trim().toLowerCase() === data()!.hostname.toLowerCase());
  const close = () => { setPendingMismatch(null); setTyped(''); };

  const trustNew = async () => {
    const current = data();
    if (!current || !matches()) return;
    await api.forgetServerFingerprint(current.hostname);
    close();
    setAuthError(t('The old certificate was forgotten. Sign in again and compare the new fingerprint.'));
  };
  const copy = () => {
    const current = data();
    if (current) void navigator.clipboard?.writeText(`Server: ${current.hostname}\nTrusted: ${current.expected}\nPresented now: ${current.actual}`);
  };

  return (
    <Modal open={!!data()} onClose={close} title={t("This server's certificate changed")} width="680px" tone="danger" dismissable={false}>
      <Show when={data()}>
        {(current) => (
          <div class="col g20">
            <p class="fs16">
              {t('The certificate of {host} is not the one you trusted before. This can happen after a server reinstall — or when someone is intercepting your connection. Your password was not sent.', { host: current().hostname })}
            </p>
            <div class="grid" style={{ 'grid-template-columns': 'repeat(2, minmax(0, 1fr))', gap: '12px' }}>
              <div class="fp-card">
                <span class="sec-label tt">{t('You trusted')}</span>
                <div class="fp-grid plain"><For each={fingerprintGroups(current().expected)}>{(g) => <span>{g}</span>}</For></div>
              </div>
              <div class="fp-card bad">
                <span class="sec-label tr">{t('The server now presents')}</span>
                <div class="fp-grid plain" data-testid="actual-fingerprint">
                  <For each={fingerprintGroups(current().actual)}>{(g, row) => (
                    <span>
                      <For each={diffBytes(fingerprintGroups(current().expected)[row()] ?? '', g)}>
                        {(b, i) => <><span classList={{ tr: b.differs }}>{b.text}</span>{i() < g.split(':').length - 1 ? ':' : ''}</>}
                      </For>
                    </span>
                  )}</For>
                </div>
              </div>
            </div>
            <div class="col g8">
              <span class="fs13 t2">{t('Ask the server admin to read out the fingerprint shown on the Server page. Only continue if it matches the red one above.')}</span>
              <Field label={t('To continue anyway, type the server name')} value={typed()} onInput={setTyped} placeholder={current().hostname} />
            </div>
            <div class="row g12 between">
              <Button variant="ghost" icon="copy" onClick={copy}>{t('Copy details')}</Button>
              <div class="row g12">
                <span data-autofocus style={{ display: 'contents' }}><Button variant="primary" onClick={close}>{t('Disconnect — keep me safe')}</Button></span>
                <Button variant="danger-ghost" disabled={!matches()} onClick={() => void trustNew()}>{t('Trust the new certificate')}</Button>
              </div>
            </div>
          </div>
        )}
      </Show>
    </Modal>
  );
}
