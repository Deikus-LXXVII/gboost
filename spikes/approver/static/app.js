'use strict';
const $ = (id) => document.getElementById(id);
const b64u = {
  dec(s) { s = s.replace(/-/g, '+').replace(/_/g, '/'); while (s.length % 4) s += '='; const b = atob(s); return Uint8Array.from(b, (c) => c.charCodeAt(0)).buffer; },
  enc(buf) { let s = ''; for (const x of new Uint8Array(buf)) s += String.fromCharCode(x); return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, ''); },
};
function el(tag, props, ...kids) {
  const e = document.createElement(tag);
  Object.assign(e, props || {});
  for (const k of kids) e.append(k);
  return e;
}
function say(text, ok) { const m = $('msg'); m.replaceChildren(); if (text) m.append(el('div', { className: 'msg' + (ok ? ' ok' : ''), textContent: text })); }
async function api(path, body) {
  const r = await fetch(path, body === undefined ? {} : { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.error || r.statusText);
  return j;
}

// ---- WebAuthn plumbing ----
function creationOptions(o) {
  const p = o.publicKey;
  p.challenge = b64u.dec(p.challenge);
  p.user.id = b64u.dec(p.user.id);
  (p.excludeCredentials || []).forEach((c) => (c.id = b64u.dec(c.id)));
  return o;
}
function requestOptions(o) {
  const p = o.publicKey;
  p.challenge = b64u.dec(p.challenge);
  (p.allowCredentials || []).forEach((c) => (c.id = b64u.dec(c.id)));
  return o;
}
function regJSON(c) {
  return { id: c.id, rawId: b64u.enc(c.rawId), type: c.type, extensions: c.getClientExtensionResults(),
    response: { attestationObject: b64u.enc(c.response.attestationObject), clientDataJSON: b64u.enc(c.response.clientDataJSON),
      transports: null } };
}
function authJSON(c) {
  const r = c.response;
  return { id: c.id, rawId: b64u.enc(c.rawId), type: c.type, extensions: c.getClientExtensionResults(),
    response: { authenticatorData: b64u.enc(r.authenticatorData), clientDataJSON: b64u.enc(r.clientDataJSON),
      signature: b64u.enc(r.signature), userHandle: r.userHandle ? b64u.enc(r.userHandle) : null } };
}

// ---- views ----
let current = null; // request id being viewed

async function render() {
  const s = await api('/api/state');
  const app = $('app');
  app.replaceChildren();
  $('tools').replaceChildren();
  if (!s.enrolled) return renderEnrol(app);
  if (current) {
    const r = s.requests.find((x) => x.id === current);
    if (r) return renderDetail(app, r);
    current = null;
  }
  renderInbox(app, s);
}

function renderEnrol(app) {
  const code = el('input', { placeholder: 'code from the server console', autocomplete: 'off', inputMode: 'numeric' });
  const btn = el('button', { textContent: 'Enrol this phone' });
  btn.onclick = async () => {
    try {
      const opts = creationOptions(await api('/api/enrol/start', { code: code.value }));
      const cred = await navigator.credentials.create(opts);
      await api('/api/enrol/finish', regJSON(cred));
      say('Passkey enrolled.', true);
      render();
    } catch (e) { say('Enrol failed: ' + e.message); }
  };
  app.append(el('div', { className: 'card' }, el('div', { textContent: 'Not enrolled. Type the one-time code shown on the server console.' }), code, el('div', { className: 'row' }, btn)));
}

function renderInbox(app, s) {
  const pend = s.requests.filter((r) => r.status === 'pending');
  app.append(el('div', { className: 'label', textContent: pend.length + ' pending' }));
  if (!s.requests.length) app.append(el('div', { className: 'card', textContent: 'Nothing here.' }));
  for (const r of s.requests) {
    const c = el('div', { className: 'card click' },
      el('b', { textContent: r.effect.kind + ' to ' + r.effect.to.join(', ') }),
      el('div', { className: 'val', textContent: r.effect.subject }),
      el('small', { textContent: r.status }));
    c.onclick = () => { current = r.id; render(); };
    app.append(c);
  }
  const tools = $('tools');
  const en = el('button', { textContent: 'Enable notifications' });
  en.onclick = enableNotifications;
  tools.append(el('div', { className: 'card' }, el('div', { className: 'label', textContent: 'Web Push (install to the home screen first)' }), el('div', { className: 'row' }, en)));
  tools.append(el('div', { className: 'label', textContent: 'Standalone: ' + (navigator.standalone === true || matchMedia('(display-mode: standalone)').matches) }));
}

function field(label, value) {
  return [el('div', { className: 'label', textContent: label }), el('div', { className: 'val', textContent: value })];
}

function renderDetail(app, r) {
  const e = r.effect;
  const body = el('div', { className: 'body val', textContent: e.body });
  const sentinel = el('div', { style: 'height:1px' });
  body.append(sentinel);
  const approve = el('button', { textContent: 'Approve (Face ID)', disabled: true });
  const decline = el('button', { className: 'decline', textContent: 'Decline' });
  const back = el('button', { className: 'decline', textContent: 'Back' });
  const pending = r.status === 'pending';
  const card = el('div', { className: 'card' },
    el('b', { textContent: 'Send email' }),
    ...field('Account', e.account),
    ...field('To', e.to.join(', ') || 'none'),
    ...field('Cc', e.cc.join(', ') || 'none'),
    ...field('Bcc', e.bcc.join(', ') || 'none'),
    ...field('Subject', e.subject),
    el('div', { className: 'label', textContent: 'Body (scroll to the end to enable Approve)' }), body,
    ...field('Expires', new Date(r.expiry * 1000).toLocaleTimeString()),
    ...field('Status', r.status),
    el('div', { className: 'label', textContent: 'Digest signed' }), el('div', { className: 'mono', textContent: r.digest }),
    el('div', { className: 'row' }, decline, approve), el('div', { className: 'row' }, back));
  app.append(card);
  back.onclick = () => { current = null; render(); };
  if (!pending) { approve.disabled = true; decline.disabled = true; return; }
  new IntersectionObserver((en) => { if (en.some((x) => x.isIntersecting)) approve.disabled = false; }, { root: body }).observe(sentinel);
  decline.onclick = async () => {
    try { await api('/api/requests/' + r.id + '/decline', {}); say('Declined.', true); } catch (x) { say(x.message); }
    current = null; render();
  };
  approve.onclick = async () => {
    approve.disabled = true;
    try {
      const st = await api('/api/requests/' + r.id + '/approve/start', {});
      const cred = await navigator.credentials.get(requestOptions(st.options));
      await api('/api/requests/' + r.id + '/approve/finish', { ceremony: st.ceremony, credential: authJSON(cred) });
      say('Approved and executed (see the server console).', true);
    } catch (x) { say('Not approved: ' + x.message); }
    current = null; render();
  };
}

async function enableNotifications() {
  try {
    if (!('serviceWorker' in navigator) || !('PushManager' in window)) throw new Error('Push not available here. Add to Home Screen first, then open from the icon.');
    const perm = await Notification.requestPermission();
    if (perm !== 'granted') throw new Error('Permission: ' + perm);
    const reg = await navigator.serviceWorker.ready;
    const key = (await api('/api/push/key')).key;
    const sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: b64u.dec(key) });
    await api('/api/push/subscribe', sub.toJSON());
    say('Notifications enabled. Close the app and trigger a request on the server.', true);
  } catch (e) { say('Notifications: ' + e.message); }
}

if ('serviceWorker' in navigator) navigator.serviceWorker.register('/sw.js').catch(() => {});
render().catch((e) => say('Error: ' + e.message));
setInterval(() => { if (!document.hidden && !current && !$('app').querySelector('input')) render().catch(() => {}); }, 5000);
document.addEventListener('visibilitychange', () => { if (!document.hidden && !current) render().catch(() => {}); });
