// QA 1.7.1: visual pass (themes x sizes) and two-client checks on the release build.
// Based on target/release-qa-v1.7.0/qa17.mjs. Run with Node 22+ on Windows after building isolated QA clients.
// Outputs, temporary profiles, and binaries live only under target/release-qa-v1.7.1.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { createSocket } from 'node:dgram';
import { closeSync, existsSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import https from 'node:https';
import net from 'node:net';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const qa = path.join(repo, 'target', 'release-qa-v1.7.1');
mkdirSync(qa, { recursive: true });
const expectedVersion = process.env.FINCH_QA_VERSION ?? '1.7.1';
const run = path.join(qa, 'qa-1.7.1-' + randomBytes(3).toString('hex'));
const shots = path.join(run, 'shots');
mkdirSync(run);
mkdirSync(shots, { recursive: true });
const processes = [];
const sockets = [];
const results = [];
const notes = [];
let fatal = null;
const wait = async (what, predicate, ms = 15000) => {
  let error;
  for (let end = Date.now() + ms; Date.now() < end;) {
    try { const value = await predicate(); if (value) return value; } catch (e) { error = e; }
    await delay(150);
  }
  throw new Error('Timed out waiting for ' + what + (error ? ': ' + error.message : ''));
};
async function check(name, fn) {
  console.log('START ' + name);
  let timer;
  try {
    const timeout = new Promise((_, reject) => {
      timer = setTimeout(() => reject(Object.assign(new Error('step timed out'), { fatalTimeout: true })), 170000);
      timer.unref();
    });
    const detail = (await Promise.race([fn(), timeout])) ?? {};
    const row = { name, ok: true, ...detail };
    results.push(row);
    console.log('CHECK ' + JSON.stringify(row).slice(0, 700));
    return detail;
  } catch (error) {
    process.exitCode = 1;
    results.push({ name, ok: false, error: String(error?.stack ?? error).slice(0, 2000) });
    console.log('FAILED ' + name + ': ' + error.message);
    // A timed-out operation cannot be cancelled safely: stop the run instead of overlapping tests.
    if (error.fatalTimeout) throw error;
    return null;
  } finally { clearTimeout(timer); }
}
const note = (text) => { notes.push(text); console.log('NOTE ' + text); };
function localProcess(name, exe, cwd, env = {}, args = [], visible = false) {
  const fd = openSync(path.join(run, name + '.log'), 'w');
  const proc = spawn(exe, args, { cwd, env: { ...process.env, ...env }, windowsHide: !visible, stdio: ['ignore', fd, fd] });
  closeSync(fd);
  proc.on('error', (error) => { fatal = name + ': ' + error.message; process.exitCode = 1; console.error(fatal); });
  processes.push(proc);
  return proc;
}
const tcpPort = () => new Promise((resolve, reject) => {
  const server = net.createServer().once('error', reject).listen(0, '127.0.0.1', () => {
    const port = server.address().port;
    server.close(() => resolve(port));
  });
});
const udpPort = () => new Promise((resolve, reject) => {
  const socket = createSocket('udp4').once('error', reject).bind(0, '127.0.0.1', () => {
    const port = socket.address().port;
    socket.close(() => resolve(port));
  });
});

// Attaches to a WebView page: the main window by default, or the first page whose URL matches.
async function browser(port, match = (url) => url.startsWith('http://tauri.localhost') && !url.includes('view=overlay')) {
  const url = await wait('WebView on port ' + port, async () => {
    const pages = await (await fetch('http://127.0.0.1:' + port + '/json/list', {signal:AbortSignal.timeout(2000)})).json();
    return pages.find((page) => page.type === 'page' && match(page.url))?.webSocketDebuggerUrl;
  }, 30000);
  const ws = new WebSocket(url);
  sockets.push(ws);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('CDP connection timed out')), 15000);
    timer.unref();
    ws.addEventListener('open', () => { clearTimeout(timer); resolve(); }, { once: true });
    ws.addEventListener('error', (event) => { clearTimeout(timer); reject(new Error('CDP connection failed: ' + event.message)); }, { once: true });
  });
  let id = 0;
  const pending = new Map();
  ws.addEventListener('message', ({ data }) => {
    const reply = JSON.parse(data);
    pending.get(reply.id)?.(reply);
  });
  ws.addEventListener('close', () => {
    for (const finish of pending.values()) finish({ error: { message: 'CDP socket closed' } });
    pending.clear();
  });
  const send = (method, params) => new Promise((resolve, reject) => {
    const request = ++id;
    const timer = setTimeout(() => { pending.delete(request); reject(new Error(method + ' timed out')); }, 120000);
    timer.unref();
    pending.set(request, (reply) => {
      clearTimeout(timer);
      pending.delete(request);
      if (reply.error) reject(new Error(JSON.stringify(reply.error).slice(0, 800)));
      else resolve(reply.result);
    });
    try { ws.send(JSON.stringify({ id: request, method, params })); }
    catch (error) { clearTimeout(timer); pending.delete(request); reject(error); }
  });
  async function evalInPage(fn, ...args) {
    const expression = '(' + fn.toString() + ')(...' + JSON.stringify(args) + ')';
    const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails).slice(0, 1200));
    return r.result.value;
  }
  await wait('Tauri IPC', () => evalInPage(() => !!window.__TAURI_INTERNALS__));
  return {
    eval: evalInPage,
    send,
    point: async (x, y, type = 'click') => {
      await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
      if (type === 'hover') return;
      await send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', buttons: 1, clickCount: 1 });
      await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', buttons: 0, clickCount: 1 });
    },
    invoke: (command, args) => evalInPage((command, args) => window.__TAURI_INTERNALS__.invoke(command, args), command, args ?? {}),
    // Full viewport capture (captureBeyondViewport false). Returns the file path and the PNG size.
    capture: async (file) => {
      const r = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
      const buf = Buffer.from(r.data, 'base64');
      writeFileSync(file, buf);
      return { file, width: buf.readUInt32BE(16), height: buf.readUInt32BE(20), hash: createHash('sha256').update(buf).digest('hex').slice(0, 16) };
    },
  };
}

/** Writes a sine WAV in Node (16-bit PCM). freqs gives one frequency per channel. */
function makeWav({ seconds, sampleRate, channels, freqs }) {
  const frames = Math.round(seconds * sampleRate);
  const dataBytes = frames * channels * 2;
  const buf = Buffer.alloc(44 + dataBytes);
  buf.write('RIFF', 0); buf.writeUInt32LE(36 + dataBytes, 4); buf.write('WAVE', 8);
  buf.write('fmt ', 12); buf.writeUInt32LE(16, 16); buf.writeUInt16LE(1, 20); buf.writeUInt16LE(channels, 22);
  buf.writeUInt32LE(sampleRate, 24); buf.writeUInt32LE(sampleRate * channels * 2, 28);
  buf.writeUInt16LE(channels * 2, 32); buf.writeUInt16LE(16, 34);
  buf.write('data', 36); buf.writeUInt32LE(dataBytes, 40);
  for (let i = 0; i < frames; i++) {
    for (let c = 0; c < channels; c++) {
      const env = Math.min(1, (frames - i) / (sampleRate * 0.05), i / (sampleRate * 0.02));
      const v = Math.round(Math.sin(2 * Math.PI * freqs[c] * i / sampleRate) * 0.3 * env * 32767);
      buf.writeInt16LE(v, 44 + (i * channels + c) * 2);
    }
  }
  return buf;
}

// Minimal Ogg/Opus file: OpusHead, OpusTags, and silent 20 ms Opus packets. The server does not parse it.
const crc32Ogg = (buf) => {
  let crc = 0;
  for (const byte of buf) {
    crc ^= byte << 24;
    for (let k = 0; k < 8; k++) crc = (crc & 0x80000000) ? ((crc << 1) ^ 0x04c11db7) : (crc << 1);
  }
  return crc >>> 0;
};
function oggPage(packets, seq, granule, flags) {
  const segments = [];
  for (const p of packets) {
    let len = p.length;
    while (len >= 255) { segments.push(255); len -= 255; }
    segments.push(len);
  }
  const header = Buffer.alloc(27 + segments.length);
  header.write('OggS', 0);
  header[5] = flags;
  header.writeBigUInt64LE(BigInt(granule), 6);
  header.writeUInt32LE(0x51a7e5, 14);
  header.writeUInt32LE(seq, 18);
  header[26] = segments.length;
  segments.forEach((s, i) => { header[27 + i] = s; });
  const page = Buffer.concat([header, ...packets]);
  page.writeUInt32LE(crc32Ogg(page), 22);
  return page;
}
function makeOggOpus(seconds) {
  const head = Buffer.alloc(19);
  head.write('OpusHead', 0); head[8] = 1; head[9] = 1; head.writeUInt16LE(312, 10); head.writeUInt32LE(48000, 12);
  const vendor = Buffer.from('finchspeak-qa');
  const tags = Buffer.alloc(16 + vendor.length);
  tags.write('OpusTags', 0); tags.writeUInt32LE(vendor.length, 8); vendor.copy(tags, 12);
  const silence = Buffer.from([0xf8, 0xff, 0xfe]);
  const count = Math.round(seconds * 50);
  const pages = [oggPage([head], 0, 0, 0x02), oggPage([tags], 1, 0, 0x00)];
  for (let first = 0, seq = 2; first < count; first += 100, seq++) {
    const n = Math.min(100, count - first);
    const last = first + n >= count;
    pages.push(oggPage(Array(n).fill(silence), seq, (first + n) * 960, last ? 0x04 : 0x00));
  }
  return Buffer.concat(pages);
}

const resizeScript = `param([string]$Pid_, [string]$Dw, [string]$Dh)
Add-Type @"
using System; using System.Runtime.InteropServices;
public struct QaRect { public int Left; public int Top; public int Right; public int Bottom; }
public class QaSize {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out QaRect r);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
}
"@
$h = (Get-Process -Id ([int]$Pid_)).MainWindowHandle
if ([QaSize]::IsZoomed($h) -or [QaSize]::IsIconic($h)) { [QaSize]::ShowWindow($h, 9) | Out-Null; Start-Sleep -Milliseconds 300 }
$r = New-Object QaRect
[QaSize]::GetWindowRect($h, [ref]$r) | Out-Null
$w = ($r.Right - $r.Left) + [int]$Dw
$hh = ($r.Bottom - $r.Top) + [int]$Dh
[QaSize]::SetWindowPos($h, [IntPtr]::Zero, $r.Left, $r.Top, $w, $hh, 0x0004 -bor 0x0010) | Out-Null
Write-Output ("OK " + $w + "x" + $hh)
`;
const minScript = `param([string]$Pid_, [string]$Action)
Add-Type @"
using System; using System.Runtime.InteropServices;
public class QaMin {
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
}
"@
$h = (Get-Process -Id ([int]$Pid_)).MainWindowHandle
[QaMin]::ShowWindow($h, $(if ($Action -eq 'min') { 6 } else { 9 })) | Out-Null
Start-Sleep -Milliseconds 500
Write-Output ("OK iconic=" + [QaMin]::IsIconic($h))
`;
const resizeScriptPath = path.join(run, 'resize.ps1');
const minScriptPath = path.join(run, 'minimize.ps1');
writeFileSync(resizeScriptPath, resizeScript);
writeFileSync(minScriptPath, minScript);
const psFile = (file, args) => {
  const result = spawnSync('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', file, ...args], { windowsHide: true, encoding: 'utf8', timeout: 15000 });
  if (result.error || result.status !== 0) throw new Error('Window helper failed: ' + (result.error?.message ?? result.stderr));
  return result.stdout.trim();
};

const prefix = randomBytes(2).toString('hex');
const serverPort = await tcpPort();
const quicPort = await udpPort();
const serverUrl = 'https://127.0.0.1:' + serverPort;
writeFileSync(path.join(run, 'finch-server.toml'), '[server]\nhost="127.0.0.1"\nhttp_port=' + serverPort + '\nquic_port=' + quicPort + '\n[auth]\nregistration="open"\n');

const PAPER = { background: '#f6f3ee', sidebar: '#ece7df', surface: '#ffffff', text: '#2b2824', accent: '#c4622d', signal: '#2f8f6b' };
const password = 'FinchSpeak_QA_password9';
const summary = { combos: [], sounds: {}, voice: {}, flock: {}, health: {}, overlay: {} };
const procs = {};

// Click through CDP so pointer/mousedown/outside handlers participate, rather than calling .click().
async function clickElement(c, selector, text, kind = 'click') {
  const point = await c.eval((selector, text) => {
    const el = [...document.querySelectorAll(selector)].find((e) =>
      (!text || e.textContent.trim() === text) && e.getClientRects().length && !e.closest('[inert]') && !e.disabled);
    if (!el) return null;
    el.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    const r = el.getBoundingClientRect();
    const x = r.x + r.width / 2, y = r.y + r.height / 2;
    const hit = document.elementFromPoint(x, y);
    return el === hit || el.contains(hit) ? { x, y } : null;
  }, selector, text);
  assert(point, 'Missing, obscured, or disabled control: ' + selector + ' ' + (text ?? ''));
  await c.point(point.x, point.y, kind);
  return true;
}
const clickByLabel = (c, label) => clickElement(c, 'button[aria-label=' + JSON.stringify(label) + ']');
const clickByText = (c, text, selector = 'button') => clickElement(c, selector, text);
const leafClick = async (c, name) => {
  await wait('sidebar channel ' + name, () => c.eval((name) => [...document.querySelectorAll('aside.sidebar .ch')].some((el) => el.textContent.includes(name)), name));
  const selector = await c.eval((name) => {
    const el = [...document.querySelectorAll('aside.sidebar .ch')].find((e) => [...e.querySelectorAll('*')].some((child) => child.children.length === 0 && child.textContent.trim() === name));
    if (!el) return null;
    el.dataset.qaChannel = 'selected';
    return '[data-qa-channel="selected"]';
  }, name);
  assert(selector, 'No sidebar channel ' + name);
  try { await clickElement(c, selector); }
  finally { await c.eval(() => document.querySelector('[data-qa-channel]')?.removeAttribute('data-qa-channel')); }
};
const pressEscape = (c) => c.eval(() => (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
async function closeAll(c) {
  for (let i = 0; i < 6; i++) {
    await pressEscape(c);
    await c.eval(() => {
      const dialogs = [...document.querySelectorAll('[role="dialog"]')];
      const button = dialogs.at(-1)?.querySelector('.modal-heading button[aria-label="Закрыть"]');
      button?.click();
    });
    await delay(180);
    if (await c.eval(() => !document.querySelector('[role="dialog"], [role="menu"], [role="listbox"]'))) return;
  }
  throw new Error('A dialog/menu did not close');
}
const innerSize = (c) => c.eval(() => [window.innerWidth, window.innerHeight, window.devicePixelRatio]);
const visible = (c, selector) => c.eval((selector) => [...document.querySelectorAll(selector)].some((el) => el.getClientRects().length && getComputedStyle(el).visibility !== 'hidden' && !el.closest('[inert]')), selector);
const requireVisible = (c, selector) => wait('visible ' + selector, () => visible(c, selector));
async function openSettings(c, label = 'Уведомления') {
  await closeAll(c);
  await clickByLabel(c, 'Профиль и настройки');
  await requireVisible(c, '[role="dialog"] .setnav[aria-label="Разделы настроек"]');
  await settingsPage(c, label);
}
async function settingsPage(c, label) {
  await clickByText(c, label, '[role="dialog"] nav[aria-label="Разделы настроек"] button');
  await wait('selected settings page ' + label, () => c.eval((label) => [...document.querySelectorAll('[role="dialog"] nav[aria-label="Разделы настроек"] button[aria-current="page"]')].some((e) => e.textContent.trim() === label), label));
}
async function openHealth(c) {
  await closeAll(c);
  await clickByLabel(c, 'Администрирование');
  await requireVisible(c, '[role="dialog"] nav[aria-label="Разделы администрирования"]');
  await clickByText(c, 'Здоровье', '[role="dialog"] nav[aria-label="Разделы администрирования"] button');
  await requireVisible(c, '[role="dialog"] .health-grid .health-tile');
}
async function composerSend(c, content) {
  await clickElement(c, '.composer textarea.txt');
  await c.send('Input.insertText', { text: content });
  await c.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 });
  await c.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 });
}
const channelMessages = async (c, channelId) => {
  const page = await c.invoke('fetch_channel_messages', { channelId, before: null, beforeId: null, limit: 100 });
  return Array.isArray(page) ? page : (page.messages ?? page.items ?? []);
};

// Resizes the main window until innerWidth/innerHeight match the target (CSS px). Returns the final size.
async function resizeTo(c, pid, W, H) {
  let size;
  for (let i = 0; i < 6; i++) {
    size = await innerSize(c);
    if (size[0] === W && size[1] === H) break;
    const dw = Math.round((W - size[0]) * size[2]);
    const dh = Math.round((H - size[1]) * size[2]);
    psFile(resizeScriptPath, ['-Pid_', String(pid), '-Dw', String(dw), '-Dh', String(dh)]);
    await delay(700);
  }
  size = await innerSize(c);
  assert.deepEqual(size.slice(0, 2), [W, H], 'Window failed to resize');
  return size;
}

// Sets the saved look and reloads the client, so the theme and text size apply from storage.
async function reloadAuthenticated(c) {
  const marker=randomBytes(8).toString('hex');
  await c.eval((marker)=>{window.__qaReloadMarker=marker;},marker);
  await c.send('Page.reload',{ignoreCache:true});
  await wait('authenticated sidebar after reload',()=>c.eval((marker)=>window.__qaReloadMarker!==marker && !!window.__TAURI_INTERNALS__ && !!document.querySelector('aside.sidebar .user-panel') && !!document.querySelector('aside.sidebar .ch'),marker),30000);
}
async function applyLook(c, theme, textSize) {
  const colors = theme === 'user' ? PAPER : undefined;
  const prefs = { appearance: { theme, textSize, language: 'ru', showMembers: true, claudeDefault17: true, ...(colors ? { customColors: colors } : {}) } };
  await c.eval((json) => { localStorage.setItem('finchspeak.prefs', json); localStorage.setItem('finchspeak.nestView', '0'); return true; }, JSON.stringify(prefs));
  await reloadAuthenticated(c);
  await delay(500);
}

const shotsMeta = [];
let previousHash = null;
async function shot(c, name, expectSize) {
  let required;
  if (name.endsWith('-voice-music')) required = '[role="dialog"] .music-panel';
  else if (name.endsWith('-soundboard')) required = '[role="dialog"] .soundboard-panel';
  else if (name.endsWith('-settings-notifications')) required = '[role="dialog"] [role="combobox"][aria-label="Набор звуков"]';
  else if (name.endsWith('-settings-appearance')) required = '[role="dialog"] .settings-page';
  else if (name.endsWith('-settings-keys')) required = '[role="dialog"] .settings-page';
  else if (name.endsWith('-admin-health')) required = '[role="dialog"] .health-grid .health-tile';
  else if (name.endsWith('-finch-tree')) required = '.tree-layout .tree-scene';
  else if (name.endsWith('-members')) {
    const narrow = await c.eval(()=>matchMedia('(max-width: 940px)').matches);
    if(narrow) {
      assert.equal(await visible(c,'aside.members'),false,'Members should collapse at the responsive breakpoint');
      required = '.vc[data-peer-id]';
    } else required = 'aside.members .mem';
  }
  else if (name.endsWith('-chat')) required = '.chat .composer textarea';
  if (required) await requireVisible(c, required);
  const size = await innerSize(c);
  const file = path.join(shots, name + '.png');
  const info = await c.capture(file);
  const same = previousHash === info.hash;
  previousHash = info.hash;
  const row = { name, file, png: [info.width, info.height], inner: size.slice(0, 2), expected: expectSize, hash: info.hash, identical_to_previous: same };
  shotsMeta.push(row);
  console.log('SHOT ' + JSON.stringify(row));
  if (same) note('IDENTICAL to previous shot: ' + name);
  if (expectSize) assert.deepEqual(size.slice(0, 2), expectSize, 'Screenshot viewport mismatch: ' + name);
  return row;
}

// A5 observes real DOM/CSS/WAAPI state; it does not override animation APIs or production stores.
async function replaceInput(c, selector, text) {
  await clickElement(c,selector);
  await c.send('Input.dispatchKeyEvent',{type:'keyDown',key:'a',code:'KeyA',modifiers:2,windowsVirtualKeyCode:65});
  await c.send('Input.dispatchKeyEvent',{type:'keyUp',key:'a',code:'KeyA',modifiers:2,windowsVirtualKeyCode:65});
  if(text) await c.send('Input.insertText',{text});
  else {
    await c.send('Input.dispatchKeyEvent',{type:'keyDown',key:'Backspace',code:'Backspace',windowsVirtualKeyCode:8});
    await c.send('Input.dispatchKeyEvent',{type:'keyUp',key:'Backspace',code:'Backspace',windowsVirtualKeyCode:8});
  }
}

async function motionSequence(c, name, action, options = {}) {
  const { selector = '.vc,.dock .ctl,.ctl-ico,.btn,.overlay,.modal,.ctx-menu,.select-listbox,.av-speaking-ring,.av-speaking-glow,.av-bars i,.react,.badge,.rolling,.flock-card,.flock-timer,.ripple-dot,.sb-said,.finch-bird,.bird-eye,.bird-wave,.nest,.nest-bird,.spark polyline,.seat,.skel', offsets = [0,32,75,150,260,450,750] } = options;
  const row = {name, human_review:'pending', frames:[], started_at:new Date().toISOString(), status:'capturing'};
  summary.motion.sequences.push(row);
  await c.eval((selector) => {
    const state = {started:performance.now(),running:true,samples:[],dropped:0};
    window.qaMotion = state;
    const label = (target) => {
      const element = target instanceof Element ? target : target?.element;
      return element ? {tag:element.tagName,classes:element.getAttribute('class'),peer:element.getAttribute('data-peer-id'),flip:element.getAttribute('data-flip'),message:element.getAttribute('data-message-id'),aria:element.getAttribute('aria-label'),is_tile:element.matches('.vc')} : {tag:String(target)};
    };
    const sample = () => {
      if(!state.running) return;
      const animations=document.getAnimations().slice(0,100).map((animation)=>{
        const effect=animation.effect;
        const timing=effect?.getComputedTiming();
        return {type:animation.constructor.name,name:animation.animationName??null,transition_property:animation.transitionProperty??null,playState:animation.playState,currentTime:animation.currentTime,startTime:animation.startTime,pseudo:effect?.pseudoElement??effect?.target?.type??null,target:label(effect?.target),timing:timing?{duration:timing.duration,delay:timing.delay,iterations:timing.iterations,progress:timing.progress}:null,keyframes:effect?.getKeyframes().map((frame)=>({offset:frame.offset,transform:frame.transform,opacity:frame.opacity,clipPath:frame.clipPath,translate:frame.translate,left:frame.left,top:frame.top}))};
      });
      const elements=[...document.querySelectorAll(selector)].filter((el)=>el.getClientRects().length).slice(0,90).map((el)=>{
        const style=getComputedStyle(el), rect=el.getBoundingClientRect();
        return {...label(el),rect:{x:rect.x,y:rect.y,width:rect.width,height:rect.height},transform:style.transform,opacity:style.opacity,visibility:style.visibility,pressed:el.getAttribute('aria-pressed'),checked:el.getAttribute('aria-checked'),exiting:el.getAttribute('data-exiting')};
      });
      if(state.samples.length<900) state.samples.push({elapsed_ms:performance.now()-state.started,animations,elements,viewport:[innerWidth,innerHeight]}); else state.dropped++;
      state.raf=requestAnimationFrame(sample);
    };
    sample();
  },selector);
  const started=Date.now();
  const operation=Promise.resolve().then(action);
  // Attach immediately so a fast action failure cannot become an unhandled rejection during PNG capture.
  operation.catch(()=>{});
  try {
    for(const requested of offsets) {
      await delay(Math.max(0,started+requested-Date.now()));
      const captureStarted=Date.now()-started;
      const image=await c.capture(path.join(shots,'A5-'+name+'-'+requested+'.png'));
      const frame={requested_offset_ms:requested,capture_started_ms:captureStarted,capture_finished_ms:Date.now()-started,...image};
      row.frames.push(frame);
      shotsMeta.push({name:'A5-'+name+'-'+requested,...frame});
    }
    row.detail=(await operation)??{};
    row.status='captured';
    return row;
  } catch(error) {
    row.status='failed'; row.error=error.message;
    await operation.catch(()=>{});
    throw error;
  } finally {
    row.diagnostics=await c.eval(()=>{
      const state=window.qaMotion;
      if(!state)return null;
      state.running=false;cancelAnimationFrame(state.raf);
      return {samples:state.samples,dropped_samples:state.dropped};
    }).catch((error)=>({error:error.message}));
    row.elapsed_ms=Date.now()-started;
    writeFileSync(path.join(run,'A5-'+name+'.json'),JSON.stringify(row,null,2));
    writeFileSync(path.join(run,'A5-motion-summary.json'),JSON.stringify(summary.motion,null,2));
  }
}
async function pressRelease(c, selector, holdMs = 160) {
  const point=await c.eval((selector)=>{
    const el=document.querySelector(selector);
    if(!el||el.disabled)return null;
    el.scrollIntoView({block:'nearest'});
    const r=el.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};
  },selector);
  assert(point,'Missing press target '+selector);
  await c.send('Input.dispatchMouseEvent',{type:'mouseMoved',...point});
  await c.send('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',buttons:1,clickCount:1});
  try {await delay(holdMs);}
  finally {await c.send('Input.dispatchMouseEvent',{type:'mouseReleased',...point,button:'left',buttons:0,clickCount:1});}
}

try {
  assert.equal(process.platform, 'win32', 'This native QA harness requires Windows');
  for (const rel of ['server/bin/finch-server.exe', 'sender/finch-app.exe', 'viewer/finch-app.exe']) assert(existsSync(path.join(qa, rel)), 'Build/copy the isolated QA binary first: ' + rel);
  let senderPort = await tcpPort();
  while(senderPort === serverPort) senderPort = await tcpPort();
  let viewerPort = await tcpPort();
  while (viewerPort === senderPort || viewerPort === serverPort) viewerPort = await tcpPort();
  localProcess('server', path.join(qa, 'server', 'bin', 'finch-server.exe'), run);
  await wait('isolated local server', () => new Promise((resolve, reject) => {
    const req = https.get(serverUrl + '/health', { rejectUnauthorized: false, timeout: 1500 }, (response) => {
      let data = '';
      response.on('data', (part) => data += part);
      response.on('end', () => resolve(response.statusCode === 200));
    });
    req.on('timeout', () => req.destroy(new Error('Health timeout')));
    req.on('error', reject);
  }));
  const der = readFileSync(path.join(run, 'data', 'tls', 'cert.der'));
  const fingerprint = createHash('sha256').update(der).digest('hex').match(/../g).join(':').toUpperCase();

  for (const spec of [{ name: 'sender', exe: path.join(qa, 'sender', 'finch-app.exe'), port: senderPort }, { name: 'viewer', exe: path.join(qa, 'viewer', 'finch-app.exe'), port: viewerPort }]) {
    const profile = path.join(run, spec.name);
    mkdirSync(profile);
    procs[spec.name] = localProcess(spec.name, spec.exe, profile, {
      FINCHSPEAK_PROFILE_DIR: profile, WEBVIEW2_USER_DATA_FOLDER: path.join(profile, 'WebView2'),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-address=127.0.0.1 --remote-debugging-port=' + spec.port,
    });
  }
  const sender = await browser(senderPort);
  const viewer = await browser(viewerPort);
  for(const c of [sender,viewer]) await c.send('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-motion',value:'no-preference'}]});
  const identifiers = [await sender.invoke('plugin:app|identifier'),await viewer.invoke('plugin:app|identifier')];
  assert.notEqual(identifiers[0],identifiers[1],'QA clients need distinct compiled Tauri identifiers');
  for(const identifier of identifiers) assert.notEqual(identifier,'com.finchspeak.app','Never use the installed app identifier for QA');
  summary.identifiers=identifiers;
  const names = { sender: 'sender' + prefix, viewer: 'viewer' + prefix };
  for (const [c, name] of [[sender, names.sender], [viewer, names.viewer]]) {
    await c.invoke('trust_fingerprint', { hostname: '127.0.0.1', fingerprint });
    await c.invoke('register', { serverUrl, username: name, displayName: 'QA ' + name.replace(prefix, ''), password, inviteCode: null, remember: true });
    // IPC registration populates Rust state, not Solid authStore. Reload through the real App startup.
    await c.eval((serverUrl,username)=>{
      localStorage.setItem('finchspeak.login',JSON.stringify({serverUrl,username}));
      localStorage.setItem('finchspeak.nestView','0');
    },serverUrl,name);
    await reloadAuthenticated(c);
  }
  await check('isolated QA clients and server have expected version', async () => {
    const health = await new Promise((resolve, reject) => https.get(serverUrl + '/health', { rejectUnauthorized: false, timeout:2000 }, (r) => { let d = ''; r.on('data', (p) => d += p); r.on('end', () => resolve(d)); }).on('timeout',function(){this.destroy(new Error('Version health request timed out'));}).on('error', reject));
    const versions = [await sender.invoke('app_version'), await viewer.invoke('app_version')];
    assert.deepEqual(versions, [expectedVersion, expectedVersion]);
    assert.equal(JSON.parse(health).version, expectedVersion);
    return { versions, health: health.slice(0, 200) };
  });

  // Channels, rooms, seeded chat, reactions, sounds and a library track.
  const channels = await sender.invoke('list_channels');
  const text = channels.find((c) => c.channel_type !== 'voice');
  const room = channels.find((c) => c.channel_type === 'voice' && c.name === 'stream-qa') ?? await sender.invoke('create_channel', { name: 'stream-qa', channelType: 'voice' });
  const room2 = channels.find((c) => c.channel_type === 'voice' && c.name === 'qa-room-2') ?? await sender.invoke('create_channel', { name: 'qa-room-2', channelType: 'voice' });
  assert(text, 'no text channel');

  await check('seed chat: text messages, uploaded voice fixture, reactions', async () => {
    const msgs = [
      [sender, 'Всем привет! Сегодня проверяем сборку 1.7.1.'],
      [viewer, 'Привет! Саундборд и мини-окно уже видно.'],
      [sender, 'Голосовое ниже, проверьте плеер.'],
      [viewer, 'Слышно и видно, всё на месте. Длинный текст для проверки переноса строк в узком окне: проверяем, что сообщение не выходит за край панели и аккуратно переносится на следующую строку.'],
      [sender, 'Ок, теперь реакции.'],
    ];
    for (const [c, content] of msgs) await c.invoke('send_text_message', { channelId: text.id, content, attachments: null, clientMsgId: null, replyTo: null });
    const ogg = makeOggOpus(3.2);
    writeFileSync(path.join(run, 'voice-message.ogg'), ogg);
    const waveform = Array.from({ length: 64 }, (_, i) => Math.round(40 + 180 * Math.abs(Math.sin(i / 3.5))));
    const att = await viewer.invoke('upload_voice_note', { data: Array.from(ogg), durationMs: 3200, waveform });
    await viewer.invoke('send_text_message', { channelId: text.id, content: '', attachments: [att.id], clientMsgId: null, replyTo: null });
    await delay(800);
    const page = await sender.invoke('fetch_channel_messages', { channelId: text.id, before: null, beforeId: null, limit: 50 });
    const list = Array.isArray(page) ? page : (page.messages ?? page.items ?? []);
    const first = list.find((m) => m.content?.startsWith('Всем привет'));
    const last = list.find((m) => m.content === 'Ок, теперь реакции.');
    const voiceMsg = list.find((m) => m.attachments?.length && m.attachments.some((a) => a.duration_ms || a.durationMs));
    assert(first && last && voiceMsg,'Seed text/voice messages did not persist');
    if (first) await viewer.invoke('set_reaction', { messageId: first.id, emoji: '👍', on: true });
    if (last) await viewer.invoke('set_reaction', { messageId: last.id, emoji: '🔥', on: true });
    if (first) await sender.invoke('set_reaction', { messageId: first.id, emoji: '🎉', on: true });
    return { messages: list.length, voice_message: !!voiceMsg, voice_duration_ms: voiceMsg ? (voiceMsg.attachments[0].duration_ms ?? voiceMsg.attachments[0].durationMs) : null, reactions_set: 3 };
  });

  // Soundboard: three sounds (1 s, 24 kHz mono 16-bit WAV).
  const boardSounds = [];
  await check('soundboard: upload three sounds', async () => {
    for (const [name, emoji, f] of [['Гонг', '🔔', 330], ['Хлопок', '👏', 660], ['Гудок', '📯', 880]]) {
      const wav = makeWav({ seconds: 1, sampleRate: 24000, channels: 1, freqs: [f] });
      const s = await sender.invoke('upload_board_sound', { data: Array.from(wav), name, emoji });
      boardSounds.push({ ...s, bytes: wav.length });
    }
    return { sounds: boardSounds.map((s) => s.name) };
  });

  for (const c of [sender, viewer]) await c.invoke('set_voice_state', { muted: false, deafened: false }).catch(() => {});
  await sender.invoke('join_voice_channel', { channelId: room.id, shortId: room.short_id });
  await viewer.invoke('join_voice_channel', { channelId: room.id, shortId: room.short_id });
  await delay(1000);
  await wait('viewer and sender in the room', async () => (await sender.eval(() => document.querySelectorAll('aside.members .mem').length)) >= 2, 15000);
  // Music: a library track, queued so the music panel shows a playing track.
  await check('music: library track queued (for the music panel)', async () => {
    const wav = makeWav({ seconds: 20, sampleRate: 48000, channels: 2, freqs: [440, 660] });
    const b64 = wav.toString('base64');
    const track = await sender.eval(async (b64) => {
      const bin = atob(b64);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      return window.__TAURI_INTERNALS__.invoke('music_upload', bytes.buffer, { headers: { 'x-finch-file-name': encodeURIComponent('Тихий вечер.wav'), 'x-finch-keep': 'true' } });
    }, b64);
    await sender.invoke('music_command', { command: { kind: 'enqueue', track_id: track.id } });
    return { track: track.id };
  });


  // Matrix: 4 themes x 3 sizes. Text size Large for 900x600, Medium for the rest.
  const combos = [];
  for (const theme of ['claude', 'bullfinch', 'custom', 'user']) {
    for (const [w, h, ts] of [[900, 600, 'large'], [1280, 800, 'medium'], [1920, 1080, 'medium']]) combos.push({ theme, w, h, ts });
  }
  for (const { theme, w, h, ts } of combos) {
    const tag = `${theme}-${w}x${h}`;
    const combo = { tag, screens: [] };
    summary.combos.push(combo);
    await check('visual ' + tag, async () => {
      const sizeS = await resizeTo(sender, procs.sender.pid, w, h);
      const sizeV = await resizeTo(viewer, procs.viewer.pid, w, h);
      note(`resize ${tag}: sender inner=${sizeS.slice(0, 2)} viewer inner=${sizeV.slice(0, 2)}`);
      await applyLook(sender, theme, ts);
      await applyLook(viewer, theme, ts);
      const sizeAfter = await innerSize(sender);
      assert.deepEqual(sizeAfter.slice(0, 2), [w, h]);
      if (!(await sender.eval(() => !!document.querySelector('.dock')))) await sender.invoke('join_voice_channel', { channelId: room.id, shortId: room.short_id });
      await delay(600);

      // 1. chat
      await closeAll(sender);
      await leafClick(sender, text.name);
      await delay(900);
      combo.screens.push(await shot(sender, `${tag}-chat`, [w, h]));

      // 2. Music is now an app-level Modal: it can open while the text channel stays selected.
      await closeAll(sender);
      await clickByLabel(sender, 'Музыка');
      await requireVisible(sender, '[role="dialog"] .music-panel');
      combo.screens.push(await shot(sender, tag + '-voice-music', [w, h]));
      await closeAll(sender);

      // 3. Sounds is also a Modal; pressing the room button again does not close it.
      await clickByLabel(sender, 'Звуки');
      await requireVisible(sender, '[role="dialog"] .soundboard-panel');
      combo.screens.push(await shot(sender, tag + '-soundboard', [w, h]));
      await closeAll(sender);

      // 4. Actual voice tiles; members are intentionally hidden by CSS at <=940px.
      await leafClick(sender, room.name);
      await wait('two voice tiles', () => sender.eval(() => document.querySelectorAll('.vc[data-peer-id]').length >= 2));
      combo.screens.push(await shot(sender, tag + '-members', [w, h]));

      // 5. Select by labels, not indices: current source places Voice at 3 and Keys at 4.
      await openSettings(sender);
      for (const [label, suffix] of [['Уведомления', 'notifications'], ['Внешний вид', 'appearance'], ['Клавиши и о программе', 'keys']]) {
        await settingsPage(sender, label);
        await delay(200);
        if (suffix === 'appearance') await requireVisible(sender, '[role="dialog"] .settings-page');
        combo.screens.push(await shot(sender, tag + '-settings-' + suffix, [w, h]));
      }
      await closeAll(sender);

      // 6. Administration has its own labelled navigation and real health tiles.
      await openHealth(sender);
      combo.screens.push(await shot(sender, tag + '-admin-health', [w, h]));
      await closeAll(sender);

      // 7. flock call card on the viewer
      await closeAll(viewer);
      await sender.invoke('call_flock', { targetChannel: room2.id });
      await wait('flock card on viewer', () => viewer.eval(() => !!document.querySelector('.flock-card')), 10000);
      await delay(500);
      const flockShot = await viewer.capture(path.join(shots, `${tag}-flock-card.png`));
      shotsMeta.push({ name: `${tag}-flock-card`, viewer: true, png: [flockShot.width, flockShot.height], hash: flockShot.hash });
      combo.screens.push({ name: `${tag}-flock-card`, hash: flockShot.hash });
      await clickByText(viewer, 'Остаться', '.flock-card button');
      await wait('flock card dismissed', () => viewer.eval(() => !document.querySelector('.flock-card')));
      await delay(400);

      // 8. Finch mode tree, then back to classic
      await closeAll(sender);
      await clickElement(sender, 'button[title="Перейти в режим Finch"]');
      await delay(1600);
      combo.screens.push(await shot(sender, `${tag}-finch-tree`, [w, h]));
      await clickElement(sender, 'button[title="Перейти в классический режим"]');
      await requireVisible(sender, 'aside.sidebar');
      await wait('Finch tree closed', () => sender.eval(() => !document.querySelector('.tree-layout')));

      return { screens: combo.screens.length, size: sizeAfter.slice(0, 2) };
    });
  }
  summary.shots = shotsMeta;
  writeFileSync(path.join(run, 'shots-meta.json'), JSON.stringify(shotsMeta, null, 2));

  // ---- Task 14: two clients (back to Claude Dark, 1280x800, Medium) -----------------------------
  const base = await resizeTo(sender, procs.sender.pid, 1280, 800);
  await resizeTo(viewer, procs.viewer.pid, 1280, 800);
  await applyLook(sender, 'claude', 'medium');
  await applyLook(viewer, 'claude', 'medium');
  await delay(500);
  await closeAll(sender);
  await closeAll(viewer);
  note('task14 base size ' + base.slice(0, 2));

  // Hooks on the viewer: sound-played, flock-call, notify (invoke), and the sender's sound-played for the limit.
  const installHooks = (c) => c.eval(() => {
    window.qaEvents = { 'sound-played': [], 'flock-call': [] };
    window.qaNotify = [];
    const tauri = window.__TAURI_INTERNALS__;
    if (!window.qaHooked) {
      window.qaHooked = true;
      const original = tauri.invoke;
      tauri.invoke = function (cmd, args, opts) {
        if (cmd === 'notify') window.qaNotify.push({ title: args?.notice?.title, body: args?.notice?.body, target: args?.notice?.target, at: Date.now() });
        return original.call(this, cmd, args, opts);
      };
    }
    if (window.qaListening) return true;
    window.qaListening = true;
    const listen = async (event) => tauri.invoke('plugin:event|listen', {
      event, target: { kind: 'Any' },
      handler: tauri.transformCallback(({ payload }) => window.qaEvents[event].push({ payload, at: Date.now() })),
    });
    return Promise.all([listen('sound-played'), listen('flock-call')]).then(() => true);
  });
  await check('task14 hooks installed on viewer', () => installHooks(viewer).then(() => ({ ok: true })));

  await check('14a soundboard reaches the other client: event, file, 3 s limit', async () => {
    const sound = boardSounds[0];
    const viewerSelf = await viewer.eval(() => window.__TAURI_INTERNALS__.invoke('get_current_user'));
    await sender.invoke('play_board_sound', { soundId: sound.id });
    const got = await wait('sound-played on viewer', () => viewer.eval(() => window.qaEvents['sound-played'].at(-1) ?? null), 8000);
    assert.equal(got.payload.sound_id, sound.id, 'wrong sound id');
    const bytes = await viewer.eval(async (id) => {
      try {
        const r = await window.__TAURI_INTERNALS__.invoke('download_board_sound', { soundId: id });
        return r instanceof ArrayBuffer ? r.byteLength : (r?.byteLength ?? r?.length ?? -2);
      } catch (e) { return -1; }
    }, sound.id);
    // Rate limit: a second play right away must be refused (or dropped), a play after 3 s must pass.
    const before = await viewer.eval(() => window.qaEvents['sound-played'].length);
    const burst = await sender.invoke('play_board_sound', { soundId: sound.id }).then(() => 'accepted', (e) => 'refused: ' + String(e).slice(0, 120));
    await delay(1200);
    const afterBurst = await viewer.eval(() => window.qaEvents['sound-played'].length);
    await delay(2200);
    await sender.invoke('play_board_sound', { soundId: boardSounds[1].id }).catch(() => {});
    await wait('third sound on viewer', () => viewer.eval((n) => window.qaEvents['sound-played'].length > n, afterBurst), 6000).catch(() => null);
    const finalCount = await viewer.eval(() => window.qaEvents['sound-played'].length);
    summary.sounds = { event_received: true, sound_id_matches: true, download_bytes: bytes, expected_wav_bytes: sound.bytes, burst_second_play: burst, events_after_first: before, events_after_burst_1_2s: afterBurst, events_final: finalCount, viewer_user: viewerSelf?.id ? 'ok' : 'n/a' };
    assert(bytes === sound.bytes, 'downloaded bytes ' + bytes + ' != ' + sound.bytes);
    assert.equal(afterBurst,before,'Soundboard burst was delivered despite rate limit');
    assert(finalCount>afterBurst,'Soundboard did not recover after 3 seconds');
    return summary.sounds;
  });

  await check('14b actual 3-second microphone hold sends a new voice message to viewer', async () => {
    await closeAll(sender); await closeAll(viewer);
    await leafClick(sender, text.name); await leafClick(viewer, text.name);
    await requireVisible(sender, '.composer .voice-mic:not(:disabled)');
    const senderUser = await sender.invoke('get_current_user');
    const before = await channelMessages(viewer, text.id);
    const previous = new Set(before.map((m) => m.id));
    const mic = await sender.eval(() => {
      const el = document.querySelector('.voice-mic'); el.scrollIntoView({block:'nearest'});
      const r = el.getBoundingClientRect(); return { x:r.x+r.width/2, y:r.y+r.height/2 };
    });
    const started = Date.now();
    let heldMs;
    await sender.send('Input.dispatchMouseEvent', { type:'mousePressed', ...mic, button:'left', buttons:1, clickCount:1 });
    try {
      await requireVisible(sender, '.voice-recording');
      // The original mic element is gone: window.pointerup owns the rest of this press.
      await delay(3000);
    } finally {
      heldMs = Date.now()-started;
      await sender.send('Input.dispatchMouseEvent', { type:'mouseReleased', ...mic, button:'left', buttons:0, clickCount:1 });
    }
    await wait('native recording stopped', () => sender.eval(() => !document.querySelector('.voice-recording') && !!document.querySelector('.voice-mic')), 30000);
    const delivered = await wait('new real voice attachment delivered', async () => (await channelMessages(viewer, text.id)).find((m) => !previous.has(m.id) && m.author_id === senderUser.id && m.attachments?.some((a) => a.content_type === 'audio/ogg' && a.duration_ms >= 2000)), 20000);
    const att = delivered.attachments.find((a) => a.content_type === 'audio/ogg' && a.duration_ms >= 2000);
    await requireVisible(viewer, '[data-message-id=' + JSON.stringify(delivered.id) + '] .voice-note');
    await wait('voice player audio loaded', () => viewer.eval((id) => {
      const audio = document.querySelector('[data-message-id="' + id + '"] .voice-note audio');
      return audio && audio.readyState >= 1 && Number.isFinite(audio.duration) && audio.duration >= 2;
    }, delivered.id), 15000);
    const voiceShot = await shot(viewer, 'task14-real-voice-message-viewer', [1280, 800]);
    summary.voice = { message_id:delivered.id, attachment_id:att.id, duration_ms:att.duration_ms, hold_ms:heldMs, viewer_player_loaded:true, shot:voiceShot.name };
    return summary.voice;
  });

  await check('A1 sound set is picked with real pointer events, persists, and settings reopen', async () => {
    await openSettings(sender);
    const combo = '[role="combobox"][aria-label="Набор звуков"]';
    for (const label of ['Птицы', 'Мягкие']) {
      await clickElement(sender, combo);
      await requireVisible(sender, '[role="listbox"][aria-label="Набор звуков"]');
      await sender.eval(() => document.querySelector('.message-list')?.dispatchEvent(new Event('scroll')));
      await requireVisible(sender, '[role="listbox"][aria-label="Набор звуков"]');
      await clickByText(sender, label, '[role="listbox"] [role="option"]');
      await wait('picked sound set ' + label, () => sender.eval((label) => document.querySelector('[role="combobox"][aria-label="Набор звуков"]')?.textContent.trim() === label, label));
      await requireVisible(sender, '[role="dialog"] .settings');
    }
    assert.equal(await sender.eval(() => JSON.parse(localStorage.getItem('finchspeak.prefs')).notifications.soundSet), 'soft');
    await clickByText(sender, 'Послушать', '[role="dialog"] button');
    await closeAll(sender);
    await openSettings(sender);
    assert.equal(await sender.eval(() => document.querySelector('[role="combobox"][aria-label="Набор звуков"]')?.textContent.trim()), 'Мягкие');
    await shot(sender, 'A1-sound-set-reopened', [1280,800]);
    await closeAll(sender);
    return { stored:'soft', reopened:'Мягкие', listen_clicked:true };
  });

  await check('A4 three composer messages keep one DOM row and one entrance on both clients', async () => {
    await leafClick(sender, text.name); await leafClick(viewer, text.name);
    const texts = [1,2,3].map((i) => 'QA-enter-' + prefix + '-' + i);
    for (const c of [sender,viewer]) await c.eval((texts) => {
      const records = texts.map((text) => ({ text, first:null, seen:false, samples:0, duplicate:0, replacements:0, gaps:0, animations:new Set() }));
      window.qaRows = { records, running:true, started:performance.now() };
      const frame = () => {
        if (!window.qaRows.running) return;
        for (const rec of records) {
          const rows = [...document.querySelectorAll('.message-list .msg')].filter((row) => row.querySelector('.body')?.textContent === rec.text);
          rec.samples++;
          if (rows.length > 1) rec.duplicate++;
          if (rows.length === 0 && rec.seen) rec.gaps++;
          for (const row of rows) {
            if (!rec.first) rec.first = row;
            else if (row !== rec.first) rec.replacements++;
            rec.seen = true;
            for (const animation of row.getAnimations()) if (animation.animationName?.includes('msg')) rec.animations.add(animation);
          }
        }
        window.qaRows.raf = requestAnimationFrame(frame);
      };
      frame();
    }, texts);
    const frames=[];
    try {
      for (const [index,content] of texts.entries()) {
        const start=Date.now();
        await composerSend(sender,content);
        for (const offset of [0,16,32,48,64,80,100,125,150,180,220,300]) {
          await delay(Math.max(0,start+offset-Date.now()));
          const capture=await shot(sender,'A4-message-'+(index+1)+'-frame-'+offset,[1280,800]);
          frames.push({message:index+1, requested_offset_ms:offset, actual_offset_ms:Date.now()-start, ...capture});
        }
        for (const c of [sender,viewer]) await wait('exactly one stored row '+content, () => c.eval((content) => {
          const rows=[...document.querySelectorAll('.message-list .msg')].filter((row)=>row.querySelector('.body')?.textContent===content);
          return rows.length===1 && !rows[0].matches('.sending,.failed') && !rows[0].dataset.messageId.startsWith('local-');
        },content));
      }
      await delay(500);
      const sampled=[];
      for (const c of [sender,viewer]) sampled.push(await c.eval(() => window.qaRows.records.map((r)=>({text:r.text, seen:r.seen, samples:r.samples, duplicates:r.duplicate, replacements:r.replacements, gaps:r.gaps, entrance_animations:r.animations.size}))));
      writeFileSync(path.join(run,'A4-frames.json'),JSON.stringify({frames,sampled},null,2));
      for (const rows of sampled) for (const r of rows) {
        assert(r.seen && r.samples>0,'Row was never sampled: '+r.text);
        assert.equal(r.duplicates,0,'Duplicate row '+r.text);
        assert.equal(r.replacements,0,'Optimistic row was remounted '+r.text);
        assert.equal(r.gaps,0,'Row disappeared after appearing '+r.text);
        assert.equal(r.entrance_animations,1,'Expected one entrance, without restarts: '+r.text);
      }
      const persisted=await channelMessages(viewer,text.id);
      for(const content of texts) assert.equal(persisted.filter((m)=>m.content===content).length,1,'Duplicate persisted message');
      writeFileSync(path.join(run,'A4-frames.json'),JSON.stringify({frames,sampled},null,2));
      return {messages:texts.length,frames:frames.length,clients:sampled};
    } finally {
      for(const c of [sender,viewer]) await c.eval(()=>{if(window.qaRows){window.qaRows.running=false;cancelAnimationFrame(window.qaRows.raf);}}).catch(()=>{});
    }
  });

  // A5: capture observable motion in the native WebView. Final states, not transient frame deadlines, are gates.
  summary.motion = {human_review:'pending',sequences:[],coverage:[
    [1,'Chat new/own/wave','A4 frame series'],[2,'Reaction pop','reaction-pop'],[3,'Badge counter pop','badge-counter'],[4,'Rolling numbers/text','reaction-pop,health-sparkline'],
    [5,'Voice join','viewer-join'],[6,'Voice leave','viewer-leave'],[7,'Tile rearrangement/resize','resize-stable-tiles'],[8,'Theme circle','theme-circle'],
    [9,'Dock icon turn/spring','dock-mute-press'],[10,'Button hover/press','button-hover-press'],[11,'Toggle/slider','toggle-slider'],[12,'Speaking ring/pulse/waves/bars/glow','speaker-*'],
    [13,'Modal/toast/menu enter/exit','modal-enter,modal-exit,menu-enter,menu-exit,toast-enter,toast-exit'],[14,'Flock card/timer','flock-card'],
    [15,'Soundboard ripple/name bubble','soundboard-ripple,soundboard-bubble'],[16,'Mascot sway/blink/waves','mascot'],[17,'Finch nest sway/hop','nest-hover,nest-hop'],
    [18,'Health sparkline','health-sparkline'],[19,'Loading skeletons','channel-loading'],[20,'Spatial seats','spatial-seat']
  ].map(([id,animation,scenario])=>({id,animation,scenario,status:'pending',human_review:'pending'})),
  limitations:['Two clients cannot reproduce a larger roster, multiple simultaneous joins, a middle-slot exit, or an interrupted multi-tile reorder.',
    'PNG capture completion times depend on WebView/CDP encoding; requested offsets are not guaranteed frame intervals.',
    'Theme circle uses the real View Transitions API when available; fallback theme changes are valid and recorded.',
    'Speaker samples use the real settings SpeakingPreview; ambient microphone-driven decoration is recorded only if naturally visible.',
    'Skeletons and other brief animations may finish before a PNG; rAF diagnostics record observations without requiring a timing-specific transient.',
    'Screenshots and animation metadata do not establish subjective smoothness. Human feel review is pending.']};
  const motionChecks = [
    ['viewer leave/join and stable resize',async()=>{
      await closeAll(sender);await closeAll(viewer);await leafClick(sender,room.name);
      await wait('two tiles before motion',()=>sender.eval(()=>document.querySelectorAll('.vc:not(.vc-leave)').length===2));
      try {
        await motionSequence(sender,'viewer-leave',async()=>{
          await viewer.invoke('leave_voice_channel');
          await wait('viewer left tile grid',()=>sender.eval(()=>document.querySelectorAll('.vc').length===1));
          return {tiles:1};
        });
        await motionSequence(sender,'viewer-join',async()=>{
          await viewer.invoke('join_voice_channel',{channelId:room.id,shortId:room.short_id});
          await wait('viewer joined tile grid',()=>sender.eval(()=>document.querySelectorAll('.vc:not(.vc-leave)').length===2));
          return {tiles:2};
        });
        await delay(300);
        const before=await sender.eval(()=>[...document.querySelectorAll('.vc')].map((el)=>el.dataset.flip));
        const series=await motionSequence(sender,'resize-stable-tiles',async()=>{
          await resizeTo(sender,procs.sender.pid,1000,720);
          await resizeTo(sender,procs.sender.pid,1440,900);
          const after=await sender.eval(()=>[...document.querySelectorAll('.vc')].map((el)=>el.dataset.flip));
          assert.deepEqual(after,before,'Resize changed tile identity/order');
          return {same_tile_order:true,final_size:[1440,900]};
        },{offsets:[0,40,100,220,450,800,1200]});
        assert(series.diagnostics?.samples?.length>0,'No resize animation diagnostics were collected');
        const falseGlides=series.diagnostics?.samples?.flatMap((sample)=>sample.animations.filter((animation)=>animation.type==='Animation'&&animation.target.is_tile&&animation.keyframes?.some((frame)=>frame.transform?.includes('translate'))))??[];
        assert.equal(falseGlides.length,0,'Resize without reorder started a WAAPI tile glide');
        series.detail.false_tile_glides=falseGlides.length;
      } finally {
        await viewer.invoke('join_voice_channel',{channelId:room.id,shortId:room.short_id});
        await resizeTo(sender,procs.sender.pid,1280,800);
      }
    }],
    ['Dock mute press/release and button/modal/menu motion',async()=>{
      await closeAll(sender);await leafClick(sender,room.name);await requireVisible(sender,'.dock .ctl');
      const wasMuted=await sender.eval(()=>document.querySelector('.dock .ctl').getAttribute('aria-pressed'));
      try {
        await motionSequence(sender,'dock-mute-press',async()=>{
          await pressRelease(sender,'.dock .ctl:first-child');
          await wait('Dock mute state changed',()=>sender.eval((old)=>document.querySelector('.dock .ctl').getAttribute('aria-pressed')!==old,wasMuted));
          return {initial_pressed:wasMuted,final_pressed:await sender.eval(()=>document.querySelector('.dock .ctl').getAttribute('aria-pressed'))};
        });
      } finally {if(await sender.eval(()=>document.querySelector('.dock .ctl').getAttribute('aria-pressed'))!==wasMuted)await clickElement(sender,'.dock .ctl:first-child');}
      await motionSequence(sender,'button-hover-press',async()=>{
        await clickElement(sender,'.room-tools button[aria-label="Музыка"]',undefined,'hover');
        await delay(100);await pressRelease(sender,'.room-tools button[aria-label="Музыка"]');
        await requireVisible(sender,'[role="dialog"] .music-panel');
        return {music_open:true};
      });
      await motionSequence(sender,'modal-exit',async()=>{
        await clickElement(sender,'[role="dialog"] .modal-heading button[aria-label="Закрыть"]');
        await wait('music modal removed after exit',()=>sender.eval(()=>!document.querySelector('.music-panel')));
        return {dialog_closed:true};
      });
      await motionSequence(sender,'modal-enter',async()=>{await clickByLabel(sender,'Звуки');await requireVisible(sender,'[role="dialog"] .soundboard-panel');return {soundboard_open:true};});
      await closeAll(sender);
      await clickElement(sender,'aside.sidebar .ch > span.trunc',text.name,'hover');
      await motionSequence(sender,'menu-enter',async()=>{await clickByLabel(sender,'Действия: '+text.name);await requireVisible(sender,'[role="menu"]');return {menu_open:true};});
      await motionSequence(sender,'menu-exit',async()=>{await pressEscape(sender);await wait('context menu removed',()=>sender.eval(()=>!document.querySelector('[role="menu"]')));return {menu_closed:true,exit_policy:'observed; instant unmount is valid'};});
    }],
    ['Theme circle, toggle/slider, speaker previews, toast',async()=>{
      await openSettings(sender,'Внешний вид');
      const supportsCircle=await sender.eval(()=>typeof document.startViewTransition==='function');
      await motionSequence(sender,'theme-circle',async()=>{
        await clickByText(sender,'Снегирь','[role="radiogroup"][aria-label="Тема"] [role="radio"]');
        await wait('UI theme changed without reload',()=>sender.eval(()=>document.documentElement.dataset.theme==='bullfinch'));
        return {view_transitions_available:supportsCircle,circle_status:supportsCircle?'available; inspect captured pseudo-animation':'unsupported; valid instant/blended fallback',theme:'bullfinch',reload:false};
      });
      for(const [style,label] of [['pulse','Пульс'],['ring','Кольцо'],['waves','Волны'],['bars','Эквалайзер'],['glow','Свечение']]) {
        await motionSequence(sender,'speaker-'+style,async()=>{
          await clickByText(sender,label,'[role="radiogroup"][aria-label="Анимация речи"] [role="radio"]');
          await requireVisible(sender,'[role="dialog"] .speaking-animation-control .av.speaking');
          await sender.eval(()=>document.querySelector('.speaking-animation-control .av').scrollIntoView({block:'center'}));
          assert.equal(await sender.eval(()=>document.documentElement.dataset.speakingStyle),style);
          return {style,source:'real Settings SpeakingPreview',ambient_voice_required:false};
        },{selector:'.speaking-animation-control,.av.speaking,.av-speaking-ring,.av-speaking-glow,.av-bars i'});
      }
      await settingsPage(sender,'Уведомления');
      const toggle='[role="dialog"] [role="switch"][aria-label="Звуки"]';
      const original=await sender.eval((selector)=>document.querySelector(selector).getAttribute('aria-checked'),toggle);
      const slider='[role="dialog"] input[type="range"][aria-label="Громкость звука"]';
      await motionSequence(sender,'toggle-slider',async()=>{
        await clickElement(sender,toggle);
        await wait('sound switch toggled',()=>sender.eval((selector,old)=>document.querySelector(selector).getAttribute('aria-checked')!==old,toggle,original));
        await clickElement(sender,slider,undefined,'hover');
        await sender.eval((selector)=>document.querySelector(selector).focus(),slider);
        const old=await sender.eval((selector)=>document.querySelector(selector).value,slider);
        await sender.send('Input.dispatchKeyEvent',{type:'keyDown',key:'ArrowLeft',code:'ArrowLeft',windowsVirtualKeyCode:37});
        await sender.send('Input.dispatchKeyEvent',{type:'keyUp',key:'ArrowLeft',code:'ArrowLeft',windowsVirtualKeyCode:37});
        assert.notEqual(await sender.eval((selector)=>document.querySelector(selector).value,slider),old,'Slider value did not change');
        await clickElement(sender,toggle);
        return {switch_restored:true,slider_changed:true};
      });
      await settingsPage(sender,'Клавиши и о программе');
      await motionSequence(sender,'toast-enter',async()=>{await clickByText(sender,'Скопировать отпечаток','[role="dialog"] button');await requireVisible(sender,'.toast');return {toast_visible:true};});
      await motionSequence(sender,'toast-exit',async()=>{await clickElement(sender,'.toast button.icon-close');await wait('toast removed',()=>sender.eval(()=>!document.querySelector('.toast')));return {toast_closed:true};});
      await settingsPage(sender,'Внешний вид');
      await clickByText(sender,'Claude Dark','[role="radiogroup"][aria-label="Тема"] [role="radio"]');
      await wait('Claude restored',()=>sender.eval(()=>document.documentElement.dataset.theme==='claude'));
      await closeAll(sender);
    }],
    ['Chat reaction/rolling/badge and loading observations',async()=>{
      await closeAll(sender);await closeAll(viewer);
      await motionSequence(sender,'channel-loading',async()=>{await leafClick(sender,text.name);await requireVisible(sender,'.chat .composer textarea');return {chat_ready:true,skeleton_observation:'opportunistic; a fast response may not paint a skeleton'};});
      await leafClick(viewer,text.name);
      const reaction=await sender.eval(()=>{const button=[...document.querySelectorAll('.react')].find((button)=>button.getAttribute('aria-label')?.startsWith('👍 '));button?.scrollIntoView({block:'center'});return !!button;});
      assert(reaction,'Seed reaction missing');
      await motionSequence(sender,'reaction-pop',async()=>{
        const before=await sender.eval(()=>[...document.querySelectorAll('.react')].find((button)=>button.getAttribute('aria-label')?.startsWith('👍 ')).getAttribute('aria-label'));
        await clickElement(sender,'.react[aria-label^="👍 "]');
        await wait('reaction count changed',()=>sender.eval((old)=>[...document.querySelectorAll('.react')].find((button)=>button.getAttribute('aria-label')?.startsWith('👍 '))?.getAttribute('aria-label')!==old,before));
        return {reaction_changed:true};
      });
      await leafClick(sender,room.name);
      await motionSequence(sender,'badge-counter',async()=>{
        for(let n=1;n<=2;n++) {await composerSend(viewer,'A5-unread-'+prefix+'-'+n);await delay(180);}
        await wait('unread badge exists',()=>sender.eval((name)=>[...document.querySelectorAll('aside.sidebar .ch')].find((el)=>el.querySelector('span.trunc')?.textContent===name)?.querySelector('.badge'),text.name));
        return {unread_badge:true,new_messages:2};
      });
    }],
    ['Flock card/timer and soundboard ripple/name bubble',async()=>{
      await closeAll(sender);await closeAll(viewer);await leafClick(sender,room.name);
      await motionSequence(viewer,'flock-card',async()=>{await sender.invoke('call_flock',{targetChannel:room2.id});await requireVisible(viewer,'.flock-card .flock-timer');return {card_visible:true,timer:'30-second strip sampled, not full-duration timing assertion'};},{offsets:[0,50,120,250,500,1000,1600]});
      await clickByText(viewer,'Остаться','.flock-card button');
      await leafClick(viewer,room.name);
      await clickByLabel(sender,'Звуки');await requireVisible(sender,'.soundboard-play');
      await Promise.all([
        motionSequence(sender,'soundboard-ripple',async()=>{await clickElement(sender,'.soundboard-play');return {sound_clicked:true};}),
        motionSequence(viewer,'soundboard-bubble',async()=>{await requireVisible(viewer,'.vc .sb-said');return {bubble_delivered:true};})
      ]);
      await closeAll(sender);
    }],
    ['Finch nest sway/hop and mascot',async()=>{
      await closeAll(sender);await leafClick(sender,room.name);
      await clickElement(sender,'button[title="Перейти в режим Finch"]');await requireVisible(sender,'.nest');
      const nestSelector='.nest[aria-label^="'+room.name+',"]';
      await motionSequence(sender,'nest-hover',async()=>{await clickElement(sender,nestSelector,undefined,'hover');return {nest_hovered:true};});
      try {
        await viewer.invoke('leave_voice_channel');
        await wait('viewer left nest',()=>sender.eval((selector)=>document.querySelector(selector)?.querySelectorAll('.nest-bird').length===1,nestSelector));
        await motionSequence(sender,'nest-hop',async()=>{await viewer.invoke('join_voice_channel',{channelId:room.id,shortId:room.short_id});await wait('viewer bird joined nest',()=>sender.eval((selector)=>document.querySelector(selector)?.querySelectorAll('.nest-bird').length===2,nestSelector));return {birds:2};});
      } finally {await viewer.invoke('join_voice_channel',{channelId:room.id,shortId:room.short_id});}
      await clickByLabel(sender,'Вернуться к дереву');
      await clickElement(sender,'button[title="Перейти в классический режим"]');
      await requireVisible(sender,'.finch-bird');
      await motionSequence(sender,'mascot',async()=>{await requireVisible(sender,'.finch-bird');return {mascot_visible:true,blink_note:'short sample may not reach the five-second blink cycle'};},{offsets:[0,80,200,500,1000,2000,5100]});
      await leafClick(sender,room.name);
    }],
    ['Server health drawing/rolling and spatial seat movement',async()=>{
      await closeAll(sender);
      await motionSequence(sender,'health-sparkline',async()=>{await openHealth(sender);await wait('health sparkline data',()=>sender.eval(()=>document.querySelectorAll('.spark polyline').length>0),12000);return {health_tiles:true,polylines:await sender.eval(()=>document.querySelectorAll('.spark polyline').length)};},{offsets:[0,60,140,280,600,1200,5500]});
      await closeAll(sender);await leafClick(sender,room.name);
      await clickElement(sender,'.dock .ctl[aria-label="Настройки голоса"]');
      const spatial='[role="switch"][aria-label="Объёмный звук"]';
      await requireVisible(sender,spatial);
      const originally=await sender.eval((selector)=>document.querySelector(selector).getAttribute('aria-checked'),spatial);
      if(originally!=='true')await clickElement(sender,spatial);
      await closeAll(sender);await requireVisible(sender,'.spatial-stage .seat');
      const before=await sender.eval(()=>document.querySelector('.spatial-stage .seat').getAttribute('style'));
      await motionSequence(sender,'spatial-seat',async()=>{
        await sender.eval(()=>document.querySelector('.spatial-stage .seat').focus());
        await sender.send('Input.dispatchKeyEvent',{type:'keyDown',key:'ArrowRight',code:'ArrowRight',windowsVirtualKeyCode:39});
        await sender.send('Input.dispatchKeyEvent',{type:'keyUp',key:'ArrowRight',code:'ArrowRight',windowsVirtualKeyCode:39});
        await wait('spatial seat transform changed',()=>sender.eval((old)=>document.querySelector('.spatial-stage .seat').getAttribute('style')!==old,before));
        return {seat_changed:true,input:'native keyboard',audio_perception:'human check pending'};
      });
      await clickElement(sender,'.dock .ctl[aria-label="Настройки голоса"]');
      if(originally!=='true')await clickElement(sender,spatial);
      await closeAll(sender);
    }]
  ];
  for(const [name,perform] of motionChecks) await check('A5 '+name,perform);
  for(const coverage of summary.motion.coverage) {
    if(coverage.id===1) coverage.status=results.some((result)=>result.name.startsWith('A4 ')&&result.ok)?'captured in A4':'A4 failed/not captured';
    else {
      const names=coverage.scenario.split(',');
      const sequences=summary.motion.sequences.filter((sequence)=>names.some((name)=>name.endsWith('*')?sequence.name.startsWith(name.slice(0,-1)):sequence.name===name));
      coverage.status=sequences.length?sequences.every((sequence)=>sequence.status==='captured')?'captured; human review pending':'capture failed; inspect JSON':'not reached/captured';
      coverage.sequences=sequences.map((sequence)=>sequence.name);
      coverage.observed_animation_types=[...new Set(sequences.flatMap((sequence)=>sequence.diagnostics?.samples?.flatMap((sample)=>sample.animations.map((animation)=>animation.type))??[]))];
      coverage.observed_animation_names=[...new Set(sequences.flatMap((sequence)=>sequence.diagnostics?.samples?.flatMap((sample)=>sample.animations.map((animation)=>animation.name??animation.transition_property??animation.type))??[]))];
      coverage.transient_observation=coverage.observed_animation_names.length?'See target-specific rAF diagnostics; global animation names may include unrelated background motion.':'No animation observed; instant fallback or a transient missed by capture is not a smoothness verdict.';
    }
  }
  summary.motion.checks=results.filter((result)=>result.name.startsWith('A5 '));
  summary.motion.assertions_passed=summary.motion.checks.every((result)=>result.ok);
  writeFileSync(path.join(run,'A5-motion-summary.json'),JSON.stringify(summary.motion,null,2));

  await check('A7 channel menu survives unrelated scroll and rename stays clickable', async () => {
    await closeAll(sender);
    await clickElement(sender,'aside.sidebar .ch > span.trunc',text.name,'hover');
    await delay(160);
    await clickByLabel(sender, 'Действия: '+text.name);
    await requireVisible(sender,'[role="menu"]');
    await sender.eval(()=>document.querySelector('.message-list').dispatchEvent(new Event('scroll')));
    await delay(220);
    await requireVisible(sender,'[role="menu"]');
    await clickByText(sender,'Переименовать','[role="menu"] button');
    await requireVisible(sender,'[role="dialog"] input');
    assert(await sender.eval(()=>[...document.querySelectorAll('[role="dialog"] .modal-title')].some((e)=>e.textContent==='Переименовать канал')),'Rename action did not open its dialog');
    await shot(sender,'A7-channel-rename-clickable',[1280,800]);
    await closeAll(sender);
    return {unrelated_scroll_preserved:true,rename_dialog:true};
  });

  await check('A7 participant move submenu survives crossing and moves viewer', async () => {
    await closeAll(sender); await closeAll(viewer);
    await leafClick(sender,room.name);
    await wait('viewer tile',()=>sender.eval(()=>[...document.querySelectorAll('.vc')].some((e)=>e.textContent.includes('QA viewer'))));
    await sender.eval(()=>{const tile=[...document.querySelectorAll('.vc')].find((e)=>e.textContent.includes('QA viewer')); tile.querySelector('.participant-actions').dataset.qaMove='true';});
    await clickElement(sender,'[data-qa-move]');
    await requireVisible(sender,'[role="menu"] button[aria-haspopup="menu"]');
    await clickElement(sender,'[role="menu"] button[aria-haspopup="menu"]',undefined,'hover');
    await wait('move submenu',()=>sender.eval(()=>document.querySelectorAll('[role="menu"]').length===2));
    // Reproduce crossing a sibling en route to the submenu; entering the child must cancel its close timer.
    await sender.eval(()=>{
      const panels=[...document.querySelectorAll('[role="menu"]')];
      const sibling=[...panels[0].querySelectorAll('button')].find((b)=>!b.hasAttribute('aria-haspopup'));
      sibling.dispatchEvent(new PointerEvent('pointerenter',{pointerType:'mouse'}));
      panels[1].dispatchEvent(new PointerEvent('pointerenter',{pointerType:'mouse'}));
      document.querySelector('aside.members')?.dispatchEvent(new Event('scroll'));
    });
    await delay(240);
    assert.equal(await sender.eval(()=>document.querySelectorAll('[role="menu"]').length),2,'Submenu closed during crossing/unrelated scroll');
    await shot(sender,'A7-move-submenu',[1280,800]);
    try {
      await clickByText(sender,room2.name,'[role="menu"] button');
      await wait('viewer moved via submenu',()=>viewer.eval((name)=>document.querySelector('.voice-open')?.textContent.includes(name),room2.name),15000);
      return {crossing_delay_ms:240,unrelated_scroll_preserved:true,viewer_moved_to:room2.name};
    } finally {
      await viewer.invoke('join_voice_channel',{channelId:room.id,shortId:room.short_id});
      await wait('viewer restored to original room',()=>viewer.eval((name)=>document.querySelector('.voice-open')?.textContent.includes(name),room.name),15000);
      await closeAll(sender);
    }
  });

  // A7 remaining callers: test existing UI, explicitly document requested menus absent from this build.
  summary.popovers={human_review:'pending',items:[],safety:'No passwords, deletion, autostart, native URI registration or system-tray actions are touched.'};
  const saveSweep=()=>writeFileSync(path.join(run,'A7-popover-sweep.json'),JSON.stringify(summary.popovers,null,2));
  async function sweepItem(name, source, perform) {
    const item={name,source,status:'running',shots:[]};summary.popovers.items.push(item);saveSweep();
    await check('A7 sweep '+name,async()=>{
      try {
        item.detail=await perform(item);
        item.status='passed';
        return {source,...item.detail,shots:item.shots};
      } catch(error) {item.status='failed';item.error=error.message;throw error;}
      finally {saveSweep();}
    });
  }
  async function sweepShot(item,c,name,selector) {
    await requireVisible(c,selector);
    const row=await shot(c,'A7-sweep-'+name,[1280,800]);item.shots.push(row.file);return row;
  }
  await sweepItem('missing requested popup menus','MessageItem.tsx, Composer.tsx, Sidebar.tsx, Titlebar.tsx',async(item)=>{
    await closeAll(sender);await leafClick(sender,text.name);
    const controls=await sender.eval(()=>({composer:[...document.querySelectorAll('.composer button')].map((button)=>({label:button.getAttribute('aria-label'),title:button.title,text:button.textContent.trim()})),brand:document.querySelector('aside.sidebar .brand')?.outerHTML,message_toolbars:document.querySelectorAll('.msg [role="toolbar"]').length,message_popup_triggers:document.querySelectorAll('.msg [aria-haspopup="menu"],.composer [aria-haspopup="menu"],aside.sidebar .brand button').length}));
    assert.equal(controls.message_popup_triggers,0,'UI changed: inspect newly added popup triggers before marking them missing');
    for(const [name,reason] of [
      ['message-popup-menu','MessageItem renders a hover toolbar (Reply/Copy/Edit/Delete/Add reaction); no context-menu caller or More popup exists. The real toolbar is swept separately.'],
      ['composer-more-menu','Composer renders Attach/Emoji/Voice/Send; no More/Ещё menu or popup caller exists.'],
      ['server-menu','Sidebar brand is a static image/span; there is no server-menu trigger. Administration is a separate button/modal, covered by the existing health checks.']
    ]) summary.popovers.items.push({name,status:'missing-in-UI',source:item.source,reason});
    await sweepShot(item,sender,'missing-menu-context','.chat .composer');
    return {missing:['message-popup-menu','composer-more-menu','server-menu'],observed_controls:controls};
  });
  await sweepItem('message hover actions: Reply and cancel','components/chat/MessageItem.tsx: Tools',async(item)=>{
    await closeAll(sender);await leafClick(sender,text.name);
    await requireVisible(sender,'.message-list .msg .body');
    await sender.eval(()=>{const row=[...document.querySelectorAll('.msg')].find((row)=>row.querySelector('.body')?.textContent.trim());if(!row)throw new Error('No text message for action toolbar');row.dataset.qaActions='true';row.scrollIntoView({block:'center'});});
    const row='.msg[data-qa-actions="true"]';
    try {
      await clickElement(sender,row+' .body',undefined,'hover');
      await sweepShot(item,sender,'message-actions',row+' .hover-tools[role="toolbar"]');
      await clickElement(sender,row+' button[aria-label="Ответить"]');
      await sweepShot(item,sender,'message-reply','.reply-bar');
      await clickByLabel(sender,'Отменить ответ');
      await wait('reply bar closed',()=>sender.eval(()=>!document.querySelector('.reply-bar')));
      return {panel:'hover toolbar, not popup menu',safe_action:'Reply then Cancel reply',reply_cancelled:true};
    } finally {await sender.eval(()=>document.querySelector('[data-qa-actions]')?.removeAttribute('data-qa-actions'));}
  });
  await sweepItem('message reaction emoji picker: open and Escape','components/chat/MessageItem.tsx -> EmojiPicker',async(item)=>{
    await closeAll(sender);await leafClick(sender,text.name);
    await requireVisible(sender,'.message-list .msg .body');
    await sender.eval(()=>{const row=[...document.querySelectorAll('.msg')].find((row)=>row.querySelector('.body')?.textContent.trim());row.dataset.qaPicker='true';row.scrollIntoView({block:'center'});});
    try {
      await clickElement(sender,'.msg[data-qa-picker] .body',undefined,'hover');
      await clickElement(sender,'.msg[data-qa-picker] .hover-tools button[aria-label="Добавить реакцию"]');
      await sweepShot(item,sender,'message-emoji','.picker-anchor .emoji-picker[role="dialog"]');
      await pressEscape(sender);await wait('message emoji dismissed',()=>sender.eval(()=>!document.querySelector('.picker-anchor .emoji-picker')));
      return {safe_action:'Escape',closed:true};
    } finally {await sender.eval(()=>document.querySelector('[data-qa-picker]')?.removeAttribute('data-qa-picker'));}
  });
  await sweepItem('composer emoji: pick into draft and clear','components/chat/Composer.tsx -> EmojiPicker',async(item)=>{
    await closeAll(sender);await leafClick(sender,text.name);await replaceInput(sender,'.composer textarea','');
    await clickElement(sender,'.composer button[aria-label="Эмодзи"]');
    await sweepShot(item,sender,'composer-emoji','.composer-picker .emoji-picker');
    await clickElement(sender,'.composer-picker .emoji-grid button');
    await wait('emoji inserted without send',()=>sender.eval(()=>!document.querySelector('.composer-picker')&&document.querySelector('.composer textarea')?.value.length>0));
    await sweepShot(item,sender,'composer-emoji-picked','.composer textarea');
    await replaceInput(sender,'.composer textarea','');
    assert.equal(await sender.eval(()=>document.querySelector('.composer textarea').value),'');
    return {safe_action:'Insert emoji into unsent draft, then clear',sent:false,closed:true};
  });
  await sweepItem('mentions: type real login and select candidate','components/chat/Composer.tsx: mention-menu',async(item)=>{
    await closeAll(sender);await leafClick(sender,text.name);
    await replaceInput(sender,'.composer textarea','@'+names.viewer);
    await sweepShot(item,sender,'mentions','.mention-menu[role="listbox"] [role="option"]');
    const candidate=await sender.eval(()=>document.querySelector('.mention-menu [role="option"]')?.textContent);
    assert(candidate?.includes(names.viewer),'Mention candidate is not viewer');
    await clickElement(sender,'.mention-menu [role="option"]');
    await wait('mention selected into draft',()=>sender.eval((login)=>!document.querySelector('.mention-menu')&&document.querySelector('.composer textarea')?.value.includes('@'+login+' '),names.viewer));
    await sweepShot(item,sender,'mention-picked','.composer textarea');
    await replaceInput(sender,'.composer textarea','');
    return {safe_action:'Pick known viewer into unsent draft, then clear',candidate:names.viewer,sent:false,closed:true};
  });
  await sweepItem('search: input, actual result, safe open','components/chat/SearchBox.tsx',async(item)=>{
    await closeAll(sender);await leafClick(sender,text.name);
    await replaceInput(sender,'.search-box input[aria-label="Поиск сообщений"]','проверяем');
    await sweepShot(item,sender,'search-results','.search-results[role="listbox"] .search-hit');
    const result=await sender.eval(()=>document.querySelector('.search-hit')?.textContent.trim());
    await clickElement(sender,'.search-results .search-hit');
    await wait('search result opened and panel closed',()=>sender.eval(()=>!document.querySelector('.search-results')&&[...document.querySelectorAll('.msg .body')].some((el)=>el.textContent.toLowerCase().includes('проверяем')&&el.getBoundingClientRect().bottom>0&&el.getBoundingClientRect().top<innerHeight)));
    await sweepShot(item,sender,'search-opened','.chat .message-list');
    assert.equal(await sender.eval(()=>document.querySelector('.search-box input').value),'');
    return {safe_action:'Open existing search hit',sample:result,closed:true};
  });
  await sweepItem('voice input/output Select: open and Escape without device changes','components/voice/VoiceSettingsModal.tsx: DevicePicker -> Select',async(item)=>{
    await openSettings(sender,'Голос и звук');
    const fields=[];
    for(const [name,label] of [['microphone','Микрофон'],['speakers','Динамики']]) {
      const combo='[role="dialog"] [role="combobox"][aria-label="'+label+'"]';
      const previous=await sender.eval((selector)=>document.querySelector(selector)?.textContent.trim(),combo);
      await clickElement(sender,combo);
      const list='[role="listbox"][aria-label="'+label+'"]';
      await sweepShot(item,sender,'voice-device-'+name,list+' [role="option"]');
      const options=await sender.eval((selector)=>[...document.querySelectorAll(selector+' [role="option"]')].map((el)=>({label:el.textContent.trim(),selected:el.getAttribute('aria-selected'),disabled:el.getAttribute('aria-disabled')})),list);
      await pressEscape(sender);await wait('voice device list dismissed',()=>sender.eval((selector)=>!document.querySelector(selector),list));
      assert.equal(await sender.eval((selector)=>document.querySelector(selector)?.textContent.trim(),combo),previous,'Device selection changed during Escape-only sweep');
      await requireVisible(sender,'[role="dialog"] .settings');
      fields.push({name,selected:previous,options,safe_action:'Escape; no option selected'});
    }
    await closeAll(sender);return {fields,device_configuration_changed:false,closed:true};
  });
  await sweepItem('flock destination picker: open and Escape','components/VoicePanel.tsx: openFlockMenu',async(item)=>{
    await closeAll(sender);await leafClick(sender,room.name);
    await clickByText(sender,'Позвать за собой','.voice-room .mhead button');
    await sweepShot(item,sender,'flock-picker','[role="menu"]');
    const rooms=await sender.eval(()=>[...document.querySelectorAll('[role="menu"] button')].map((button)=>button.textContent.trim()));
    assert(rooms.includes(room2.name),'Expected destination missing from flock picker');
    await pressEscape(sender);await wait('flock picker closed',()=>sender.eval(()=>!document.querySelector('[role="menu"]')));
    return {destinations:rooms,safe_action:'Escape; no call emitted',closed:true};
  });
  await sweepItem('channel quick switcher: native Ctrl+K and open current text channel','components/overlays/Overlays.tsx',async(item)=>{
    await closeAll(sender);
    await sender.send('Input.dispatchKeyEvent',{type:'keyDown',key:'k',code:'KeyK',modifiers:2,windowsVirtualKeyCode:75});
    await sender.send('Input.dispatchKeyEvent',{type:'keyUp',key:'k',code:'KeyK',modifiers:2,windowsVirtualKeyCode:75});
    await sweepShot(item,sender,'channel-switcher','[role="dialog"] [role="listbox"][aria-label="Каналы"]');
    await replaceInput(sender,'[role="dialog"] input[aria-label="Название канала"]',text.name);
    await clickElement(sender,'[role="dialog"] [role="listbox"] [role="option"]');
    await wait('quick switcher selected text channel',()=>sender.eval((name)=>!document.querySelector('[role="dialog"]')&&document.querySelector('.chat .mhead h3')?.textContent===name,text.name));
    return {safe_action:'Open existing text channel',channel:text.name,closed:true};
  });
  summary.popovers.complete=true;saveSweep();

  await check('14c flock call: viewer gets a new card/event and notify when unfocused', async () => {
    await installHooks(viewer);
    await viewer.eval(() => { window.qaNotify.length = 0; window.qaEvents['flock-call'].length = 0; });
    psFile(minScriptPath, ['-Pid_', String(procs.viewer.pid), '-Action', 'min']);
    try {
      await delay(800);
      const focused = await viewer.eval(() => document.hasFocus());
      await sender.invoke('call_flock', { targetChannel: room2.id });
      await wait('new flock-call event on viewer', () => viewer.eval(() => window.qaEvents['flock-call'].length > 0), 8000);
      const card = await wait('flock card on viewer', () => viewer.eval(() => document.querySelector('.flock-card')?.textContent.trim()), 8000);
      const notified = await wait('notify invoke on viewer', () => viewer.eval(() => window.qaNotify.length > 0 ? window.qaNotify : null), 8000);
      assert.equal(focused,false,'Viewer must be unfocused for notification test');
      assert(notified.length>0,'No native notify invocation');
      summary.flock = {focused_when_minimized:focused,card_text:card,notify:notified,event_count:await viewer.eval(()=>window.qaEvents['flock-call'].length)};
    } finally {
      psFile(minScriptPath, ['-Pid_', String(procs.viewer.pid), '-Action', 'restore']);
      await delay(500);
    }
    await clickByText(viewer,'Остаться','.flock-card button');
    return summary.flock;
  });

  await check('14d health tab filled on the admin client', async () => {
    const health = await sender.invoke('server_health');
    await openHealth(sender);
    const text2 = await sender.eval(() => document.querySelector('[role="dialog"] .health-grid').textContent.trim());
    const rows = await sender.eval(() => document.querySelectorAll('[role="dialog"] .health-tile').length);
    assert(rows >= 10,'Health tiles not populated');
    assert(/\d/.test(text2),'Health tiles have no numbers');
    const hShot = await shot(sender, 'task14-admin-health', [1280, 800]);
    summary.health = { keys: Object.keys(health ?? {}).slice(0, 20), cells: rows, has_numbers: /\d/.test(text2), sample: text2.slice(0, 240), shot: hShot.name };
    assert(health && Object.keys(health).length > 3, 'health payload empty');
    await closeAll(sender);
    return summary.health;
  });

  await check('14e mini window opens with both people and hides when disabled', async () => {
    await sender.invoke('overlay_set_enabled', { enabled: true });
    await sender.invoke('overlay_show');
    const page = await wait('overlay page', async () => {
      const pages = await (await fetch('http://127.0.0.1:' + senderPort + '/json/list', {signal:AbortSignal.timeout(2000)})).json();
      return pages.find((p) => p.type === 'page' && p.url.includes('view=overlay'));
    }, 15000);
    const overlay = await browser(senderPort, (url) => url.includes('view=overlay'));
    await wait('overlay native window visible',()=>overlay.invoke('plugin:window|is_visible',{label:'overlay'}));
    const seen = await wait('both people in the mini window', async () => {
      const t=await overlay.eval(()=>document.body.textContent);
      return t.includes('QA sender') && t.includes('QA viewer') ? {sender:true,viewer:true,text:t.slice(0,200)} : null;
    },12000);
    await delay(600);
    const oShot = await overlay.capture(path.join(shots, 'task14-mini-window.png'));
    shotsMeta.push({ name: 'task14-mini-window', png: [oShot.width, oShot.height], hash: oShot.hash });
    await sender.invoke('overlay_set_enabled', { enabled: false });
    const gone = await wait('overlay native window hidden', async () => !(await overlay.invoke('plugin:window|is_visible',{label:'overlay'})),8000).then(()=>true);
    // A hidden reusable WebView may remain in /json/list. Native visibility is the assertion.
    summary.overlay = { page_found: !!page, both_people: seen.sender && seen.viewer, text_sample: seen.text, hidden_after_disable: gone, shot: 'task14-mini-window.png' };
    assert(seen.sender && seen.viewer, 'mini window misses a person: ' + seen.text);
    assert(gone, 'mini window still open after overlay_set_enabled(false)');
    return summary.overlay;
  });
} catch (error) {
  fatal = String(error?.stack ?? error);
  process.exitCode = 1;
  console.log('FATAL ' + error.message);
  notes.push('FATAL ' + error.message);
} finally {
  for (const ws of sockets) try { ws.close(); } catch {}
  for (const proc of processes.reverse()) if(proc.pid && proc.exitCode === null && proc.signalCode === null) spawnSync('taskkill', ['/PID', String(proc.pid), '/T', '/F'], { windowsHide: true, timeout:15000 });
  const passed = results.filter((r) => r.ok).length;
  writeFileSync(path.join(run,'shots-meta.json'),JSON.stringify(shotsMeta,null,2));
  writeFileSync(path.join(run, 'qa-1.7.1-results.json'), JSON.stringify({ expectedVersion, fatal, results, notes, summary, shots: shotsMeta }, null, 2));
  console.log('SUMMARY ' + passed + '/' + results.length + ' checks passed; shots ' + shotsMeta.length + '; run dir ' + run);
  for (const r of results.filter((r) => !r.ok)) console.log('FAILED ' + r.name + ': ' + r.error);
  if (fatal || results.some((r)=>!r.ok)) process.exitCode=1;
  // CDP WebSocket close handshakes must not leave this test runner hanging after cleanup.
  setTimeout(()=>process.exit(process.exitCode ?? 0),2000).unref();
}

