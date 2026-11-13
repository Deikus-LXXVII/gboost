import { fetch } from 'bastion:plugin/http@0.1.0';

export function prepare(tool, params) {
  if (tool === 'mail') {
    // fetch throws on Err; jco turns result<_, string> into exceptions
    const r = fetch(params);
    return { tag: 'mail-send', val: { to: ['a@example.com'], subject: `status ${r.status}`, body: r.body } };
  }
  if (tool === 'note') return { tag: 'note-write', val: { path: 'n.md', text: params } };
  throw `unknown tool ${tool}`;
}
