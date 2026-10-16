import { For, Match, Show, Switch } from 'solid-js';
import { splitMentions } from './chatFormat';
import { parseMarkdown, type Block, type Inline } from './markdown';

/** Open a web address in the person's own browser (never inside the app window). */
export async function openLink(href: string) {
  if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(href);
  } else {
    window.open(href, '_blank', 'noopener,noreferrer');
  }
}

type Lookup = (login: string) => string | undefined;

function Inlines(props: { nodes: Inline[]; lookup: Lookup }) {
  return (
    <For each={props.nodes}>
      {(node) => (
        <Switch>
          <Match when={node.type === 'text' && node}>
            {(n) => <For each={splitMentions((n() as Extract<Inline, { type: 'text' }>).text, props.lookup)}>{(part) => (part.mention ? <span class="mention-chip">{part.text}</span> : <>{part.text}</>)}</For>}
          </Match>
          <Match when={node.type === 'bold' && node}>{(n) => <strong><Inlines nodes={(n() as Extract<Inline, { type: 'bold' }>).children} lookup={props.lookup} /></strong>}</Match>
          <Match when={node.type === 'italic' && node}>{(n) => <em><Inlines nodes={(n() as Extract<Inline, { type: 'italic' }>).children} lookup={props.lookup} /></em>}</Match>
          <Match when={node.type === 'code' && node}>{(n) => <code class="md-code">{(n() as Extract<Inline, { type: 'code' }>).text}</code>}</Match>
          <Match when={node.type === 'link' && node}>
            {(n) => {
              const link = () => n() as Extract<Inline, { type: 'link' }>;
              return (
                <a class="md-link" href={link().href} rel="noopener noreferrer" title={link().href}
                  onClick={(event) => { event.preventDefault(); void openLink(link().href); }}>
                  <Inlines nodes={link().children} lookup={props.lookup} />
                </a>
              );
            }}
          </Match>
        </Switch>
      )}
    </For>
  );
}

/** A message's text with the small Markdown subset and `@mention` chips. */
export function RichText(props: { text: string; lookup: Lookup }) {
  const blocks = (): Block[] => parseMarkdown(props.text);
  return (
    <For each={blocks()}>
      {(block) => (
        <Switch>
          <Match when={block.type === 'paragraph' && block}>{(b) => <div class="md-p"><Inlines nodes={(b() as Extract<Block, { type: 'paragraph' }>).children} lookup={props.lookup} /></div>}</Match>
          <Match when={block.type === 'quote' && block}>{(b) => <blockquote class="md-quote"><Inlines nodes={(b() as Extract<Block, { type: 'quote' }>).children} lookup={props.lookup} /></blockquote>}</Match>
          <Match when={block.type === 'code' && block}>
            {(b) => <pre class="md-pre"><Show when={(b() as Extract<Block, { type: 'code' }>).language}><span class="md-lang">{(b() as Extract<Block, { type: 'code' }>).language}</span></Show><code>{(b() as Extract<Block, { type: 'code' }>).text}</code></pre>}
          </Match>
        </Switch>
      )}
    </For>
  );
}
