#!/usr/bin/env node
import fs from 'node:fs';
import http from 'node:http';
import https from 'node:https';
import os from 'node:os';
import path from 'node:path';

const WEB_BASE = 'https://www.alphaxiv.org';
const API_BASE = 'https://api.alphaxiv.org';
const CLERK_BASE = 'https://clerk.alphaxiv.org';
const CLERK_API_VERSION = '2025-04-10';
const CLERK_JS_VERSION = '5.125.10';

const args = parseArgs(process.argv.slice(2));
let commandId = 0;

function parseArgs(argv) {
  const parsed = {
    output: path.join(os.homedir(), '.config', 'alphaxiv', 'clerk-cookie.txt'),
    limit: 5,
    interval: '7 Days',
    openIfNeeded: true,
    validateFeed: true,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--output') parsed.output = argv[++i];
    else if (arg === '--limit') parsed.limit = Number(argv[++i]);
    else if (arg === '--interval') parsed.interval = argv[++i];
    else if (arg === '--no-open') parsed.openIfNeeded = false;
    else if (arg === '--no-validate-feed') parsed.validateFeed = false;
    else if (arg === '-h' || arg === '--help') {
      console.log(`Usage: alphaxiv_auth_refresh.mjs [--output PATH] [--limit N] [--interval "7 Days"] [--no-open] [--no-validate-feed]`);
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  return parsed;
}

function printJson(value) {
  console.log(JSON.stringify(value, null, 2));
}

function displayPath(value) {
  const home = os.homedir();
  if (value === home) return '~';
  if (value.startsWith(`${home}${path.sep}`)) return `~${path.sep}${path.relative(home, value)}`;
  return value;
}

function devtoolsCandidates() {
  const home = os.homedir();
  if (process.platform === 'darwin') {
    return [
      path.join(home, 'Library/Application Support/Google/Chrome/DevToolsActivePort'),
      path.join(home, 'Library/Application Support/Google/Chrome Canary/DevToolsActivePort'),
      path.join(home, 'Library/Application Support/Chromium/DevToolsActivePort'),
    ];
  }
  if (process.platform === 'linux') {
    return [
      path.join(home, '.config/google-chrome/DevToolsActivePort'),
      path.join(home, '.config/chromium/DevToolsActivePort'),
    ];
  }
  return [];
}

function readDevtoolsEndpoint() {
  for (const file of devtoolsCandidates()) {
    try {
      const lines = fs.readFileSync(file, 'utf8').trim().split(/\n/);
      const port = Number(lines[0]);
      const wsPath = lines[1];
      if (port > 0 && wsPath) return { port, wsPath, file };
    } catch {
      // Continue to next profile.
    }
  }
  throw new Error('Chrome CDP is not enabled. Run web-access-cdp on, then open alphaXiv and stay logged in.');
}

function requestJson(url, { headers = {}, timeoutMs = 20000 } = {}) {
  const client = url.startsWith('https:') ? https : http;
  return new Promise((resolve, reject) => {
    const req = client.get(url, { headers, timeout: timeoutMs }, (res) => {
      let body = '';
      res.setEncoding('utf8');
      res.on('data', (chunk) => { body += chunk; });
      res.on('end', () => {
        let data = null;
        try { data = body ? JSON.parse(body) : null; } catch { /* Keep null. */ }
        resolve({ status: res.statusCode, ok: res.statusCode >= 200 && res.statusCode < 300, data });
      });
    });
    req.on('timeout', () => {
      req.destroy(new Error(`request timed out: ${url}`));
    });
    req.on('error', reject);
  });
}

function sendCdp(ws, method, params = {}, sessionId = null) {
  return new Promise((resolve, reject) => {
    const id = ++commandId;
    const message = sessionId ? { id, method, params, sessionId } : { id, method, params };
    const timer = setTimeout(() => reject(new Error(`CDP timeout: ${method}`)), 30000);
    const onMessage = (event) => {
      const data = JSON.parse(event.data);
      if (data.id !== id) return;
      clearTimeout(timer);
      ws.removeEventListener('message', onMessage);
      if (data.error) reject(new Error(`${method}: ${JSON.stringify(data.error)}`));
      else resolve(data.result || {});
    };
    ws.addEventListener('message', onMessage);
    ws.send(JSON.stringify(message));
  });
}

function openWebSocket(url) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url);
    ws.addEventListener('open', () => resolve(ws));
    ws.addEventListener('error', () => reject(new Error('failed to connect to Chrome CDP WebSocket')));
  });
}

async function waitForLoad(ws, sessionId, timeoutMs = 15000) {
  await sendCdp(ws, 'Page.enable', {}, sessionId).catch(() => {});
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const result = await sendCdp(ws, 'Runtime.evaluate', {
      expression: 'document.readyState',
      returnByValue: true,
    }, sessionId).catch(() => null);
    if (result?.result?.value === 'complete') return true;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  return false;
}

async function findOrOpenAlphaxivTarget(ws) {
  const targets = await sendCdp(ws, 'Target.getTargets');
  const pages = (targets.targetInfos || []).filter((target) => target.type === 'page');
  const existing = pages.find((target) =>
    typeof target.url === 'string' &&
    target.url.includes('alphaxiv.org') &&
    !target.url.startsWith('devtools:')
  );
  if (existing) return { targetId: existing.targetId, opened: false };

  if (!args.openIfNeeded) {
    throw new Error('No alphaXiv tab found. Open https://www.alphaxiv.org/ while logged in, or omit --no-open.');
  }

  const created = await sendCdp(ws, 'Target.createTarget', {
    url: `${WEB_BASE}/?sort=Recommended`,
    background: true,
  });
  return { targetId: created.targetId, opened: true };
}

function decodeJwtPayload(jwt) {
  if (!jwt || typeof jwt !== 'string') return {};
  const parts = jwt.split('.');
  if (parts.length < 2) return {};
  const padded = parts[1] + '='.repeat((4 - (parts[1].length % 4)) % 4);
  try {
    return JSON.parse(Buffer.from(padded, 'base64url').toString('utf8'));
  } catch {
    return {};
  }
}

function isoFromMs(value) {
  return typeof value === 'number' ? new Date(value).toISOString() : null;
}

function isoFromSeconds(value) {
  return typeof value === 'number' ? new Date(value * 1000).toISOString() : null;
}

function summarizeCookie(cookie) {
  return {
    name: cookie.name,
    domain: cookie.domain,
    expires_at: cookie.expires && cookie.expires > 0 ? new Date(cookie.expires * 1000).toISOString() : null,
    http_only: Boolean(cookie.httpOnly),
    secure: Boolean(cookie.secure),
    same_site: cookie.sameSite || null,
  };
}

async function validateClerk(cookieHeader) {
  const url = `${CLERK_BASE}/v1/client?${new URLSearchParams({
    __clerk_api_version: CLERK_API_VERSION,
    _clerk_js_version: CLERK_JS_VERSION,
  })}`;
  const response = await requestJson(url, {
    headers: {
      'User-Agent': 'Mozilla/5.0',
      'Accept': '*/*',
      'Origin': WEB_BASE,
      'Referer': `${WEB_BASE}/`,
      'Cookie': cookieHeader,
    },
  });
  const sessions = response.data?.response?.sessions || [];
  const session = sessions.find((entry) => entry?.status === 'active') || sessions[0] || {};
  const jwt = session.last_active_token?.jwt || null;
  const payload = decodeJwtPayload(jwt);
  return {
    status: response.status,
    ok: response.ok,
    session,
    jwt,
    tokenPayload: payload,
  };
}

async function validateRecommended(jwt) {
  if (!jwt || !args.validateFeed) return null;
  const url = `${API_BASE}/papers/v3/feed?${new URLSearchParams({
    pageNum: '0',
    sort: 'Recommended',
    pageSize: String(args.limit),
    interval: args.interval,
    topics: '[]',
  })}`;
  const response = await requestJson(url, {
    headers: {
      'User-Agent': 'Mozilla/5.0',
      'Accept': '*/*',
      'Origin': WEB_BASE,
      'Referer': `${WEB_BASE}/`,
      'Authorization': `Bearer ${jwt}`,
    },
  });
  const papers = response.data?.papers || [];
  return {
    status: response.status,
    ok: response.ok,
    count: Array.isArray(papers) ? papers.length : null,
    titles: Array.isArray(papers) ? papers.slice(0, 3).map((paper) => paper.title).filter(Boolean) : [],
  };
}

async function main() {
  const endpoint = readDevtoolsEndpoint();
  const ws = await openWebSocket(`ws://127.0.0.1:${endpoint.port}${endpoint.wsPath}`);
  try {
    const target = await findOrOpenAlphaxivTarget(ws);
    const attached = await sendCdp(ws, 'Target.attachToTarget', { targetId: target.targetId, flatten: true });
    const sessionId = attached.sessionId;
    if (target.opened) await waitForLoad(ws, sessionId);

    const cookiesResult = await sendCdp(ws, 'Network.getCookies', {
      urls: [`${WEB_BASE}/`, `${CLERK_BASE}/`],
    }, sessionId);
    const cookies = (cookiesResult.cookies || []).filter((cookie) =>
      /alphaxiv\.org$/.test(cookie.domain) &&
      /^(?:__session|__client|clerk_active_context)/.test(cookie.name)
    );

    if (!cookies.some((cookie) => cookie.name === '__client')) {
      throw new Error('No Clerk __client cookie found. Open alphaXiv in Chrome, log in, and retry.');
    }

    const outputPath = path.resolve(args.output.replace(/^~(?=$|\/)/, os.homedir()));
    fs.mkdirSync(path.dirname(outputPath), { recursive: true });
    if (fs.existsSync(outputPath) && fs.statSync(outputPath).size > 0) {
      const stamp = new Date().toISOString().replace(/[-:T.Z]/g, '').slice(0, 14);
      fs.copyFileSync(outputPath, `${outputPath}.backup-${stamp}`);
    }

    const cookieHeader = cookies.map((cookie) => `${cookie.name}=${cookie.value}`).join('; ');
    fs.writeFileSync(outputPath, `${cookieHeader}\n`, { mode: 0o600 });
    fs.chmodSync(outputPath, 0o600);

    const clerk = await validateClerk(cookieHeader);
    const recommended = await validateRecommended(clerk.jwt);

    printJson({
      source: 'alphaxiv',
      action: 'auth-refresh',
      ok: Boolean(clerk.ok && clerk.jwt && (!recommended || recommended.ok)),
      wrote: displayPath(outputPath),
      chrome: {
        target_opened: target.opened,
      },
      cookies: cookies.map(summarizeCookie),
      auth: {
        clerk_status: clerk.status,
        session_status: clerk.session.status || null,
        session_id_hint: clerk.session.id ? String(clerk.session.id).slice(0, 8) : null,
        session_expires_at: isoFromMs(clerk.session.expire_at),
        session_abandon_at: isoFromMs(clerk.session.abandon_at),
        short_token_available: Boolean(clerk.jwt),
        short_token_issued_at: isoFromSeconds(clerk.tokenPayload.iat),
        short_token_expires_at: isoFromSeconds(clerk.tokenPayload.exp),
      },
      recommended,
    });

    await sendCdp(ws, 'Target.detachFromTarget', {}, sessionId).catch(() => {});
  } finally {
    ws.close();
  }
}

main().catch((error) => {
  printJson({
    source: 'alphaxiv',
    action: 'auth-refresh',
    ok: false,
    error: {
      message: error.message,
    },
  });
  process.exit(1);
});
