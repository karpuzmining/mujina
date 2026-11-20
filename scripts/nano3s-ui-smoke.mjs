#!/usr/bin/env node
// Headless click-through of the dashboard popups against an in-process mock.
// Requires puppeteer-core (NODE_PATH) and Chrome (CHROME, default /usr/local/bin/google-chrome).
// No pool user, worker, or wallet is sent. Placeholder hosts only.
import http from 'node:http';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const puppeteer = await import('puppeteer-core').then((m) => m.default).catch(() => {
  const require = createRequire('/tmp/shot/node_modules/puppeteer-core/package.json');
  return require('puppeteer-core');
});

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../mujina-miner/assets');
const chrome = process.env.CHROME || '/usr/local/bin/google-chrome';
const rows = [];

function pools(n) {
  return {
    pools: ['alpha.example:3333', 'bravo.example:3333', 'charlie.example:4444', 'delta.example:3333']
      .slice(0, n).map((host, i) => ({
        url: 'stratum+tcp://' + host,
        weight: Math.floor(100 / n),
        connected: true,
        state: 'connected',
        hashrate_10m: 3.2e12,
        latency_ms: null,
        accepted: 1,
        rejected: 0,
        difficulty: 94,
        best_share: null,
        best_share_alltime: null,
      })),
  };
}

const state = {
  system: {
    mode: 'embedded', platform: 'nano3s', psu_bypass: false, immersion: false,
    device: { model: 'Avalon Nano 3S', can_reboot: true, can_restart_miner: false },
    boards: [{
      name: 'NANO3S-0',
      caps: {
        tuner: '/api/v0/boards/NANO3S-0/tuner',
        manual_tuning: '/api/v0/boards/NANO3S-0/tuning',
        fan: 'harness', fan_curve: true, temps: ['asic', 'outlet'], chip_grid: 'per_chip',
      },
    }],
    power: { ceiling_w: 137, psu_contract_w: 140, power_sensor: 'ok', power_source: 'measured' },
    firmware: { name: 'mujina-minerd', version: 'v5.1.0', update: 'bundle', build: 'sim' },
    pool: { apply: 'live' },
  },
  display: { theme: 'cyberpunk' },
  tuning: { live_preset: '338,358,378,398', pll_freq_mhz: [338, 358, 378, 398], voltage_mv: 3496, power_target_w: 95 },
  curve: { points: [{ temp_c: 40, duty_pct: 20 }, { temp_c: 70, duty_pct: 80 }] },
  psu: { mode: 'stock', override_max_w: 112, psu_bypass: false },
  led: { effect: 'auto', color: '#00f0ff', brightness: 80, speed: 40 },
  tuner: { enabled: false, profile: null },
  writes: [],
};

function bodyOf(req) {
  return new Promise((resolve) => {
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => {
      const raw = Buffer.concat(chunks).toString();
      try { resolve(raw ? JSON.parse(raw) : {}); } catch { resolve({}); }
    });
  });
}

const pages = {
  '/': 'dashboard.html', '/dashboard': 'dashboard.html', '/tuning': 'tuning.html',
  '/pool': 'pool.html', '/led': 'led.html', '/firmware': 'firmware.html', '/info': 'info.html',
};

function send(res, code, body, type) {
  const data = Buffer.isBuffer(body) ? body : Buffer.from(typeof body === 'string' ? body : JSON.stringify(body));
  res.writeHead(code, { 'Content-Type': type, 'Content-Length': data.length });
  res.end(data);
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  const p = url.pathname;
  if (req.method === 'PATCH' || req.method === 'POST' || req.method === 'PUT') {
    const body = await bodyOf(req);
    state.writes.push({ method: req.method, path: p, body });
    if (p.endsWith('/fan') && 'immersion' in body) state.system.immersion = !!body.immersion;
    if (p.endsWith('/psu')) {
      Object.assign(state.psu, body);
      if ('psu_bypass' in body) state.system.psu_bypass = !!body.psu_bypass;
    }
    if (p.endsWith('/fan-curve') && body.points) state.curve.points = body.points;
    if (p.endsWith('/tuning')) Object.assign(state.tuning, body);
    if (p.endsWith('/display') && body.theme) state.display.theme = body.theme;
    if (p.endsWith('/led')) Object.assign(state.led, body);
    if (p.endsWith('/tuner') || p.endsWith('/auto-tune')) Object.assign(state.tuner, body);
    send(res, 200, { ok: true }, 'application/json');
    return;
  }
  if (p === '/api/v0/pools') return send(res, 200, pools(Number(url.searchParams.get('n') || 2)), 'application/json');
  if (p === '/api/v0/system') return send(res, 200, state.system, 'application/json');
  if (p === '/api/v0/display') return send(res, 200, state.display, 'application/json');
  if (p === '/api/v0/boards') return send(res, 200, [{ name: 'NANO3S-0' }], 'application/json');
  if (p === '/api/v0/health') return send(res, 200, { ok: true }, 'application/json');
  if (p.startsWith('/api/') && p.endsWith('/fan-curve')) return send(res, 200, state.curve, 'application/json');
  if (p.startsWith('/api/') && p.endsWith('/tuning')) return send(res, 200, state.tuning, 'application/json');
  if (p.startsWith('/api/') && p.endsWith('/psu')) return send(res, 200, state.psu, 'application/json');
  if (p.startsWith('/api/') && p.endsWith('/led')) return send(res, 200, state.led, 'application/json');
  if (p.startsWith('/api/') && (p.endsWith('/tuner') || p.endsWith('/auto-tune'))) return send(res, 200, state.tuner, 'application/json');
  if (p === '/data') {
    return send(res, 200, {
      ok: true, miner_name: 'simulated', uptime: '1m', paused: false,
      fleet: {
        boards_online: 1, hashrate_ghs: 6100, hashrate_ths: 6.1, total_power_w: 118,
        efficiency_jth: 19.4, avg_freq_mhz: 420, avg_core_v: 3.678, avg_asic_c: 79,
        avg_outlet_c: 46, avg_fail_rate_pct: 17, avg_fan_rpm: 3200,
        power_source: 'measured', power_sensor: 'ok', series: [6000, 6100],
      },
      boards: [{
        board_name: 'NANO3S-0', model: 'Avalon Nano 3S', hashrate_ghs: 6100, hashrate_ths: 6.1,
        freq_mhz: 420, core_v: 3.678, asic_c: 79, outlet_c: 46, fail_rate_pct: 17,
        chips: Array.from({ length: 12 }, (_, i) => ({ index: i, temp_c: 70 + (i === 6 ? 18 : 0), ghs: 500, spd_dh: 4 })),
        fans: [{ rpm: 3200, percent: 55 }], series: [6000, 6100],
      }],
    }, 'application/json');
  }
  if (p === '/nano3s-detail') {
    return send(res, 200, {
      ipc_connected: true, asics_total: 12, ghsmm: 8200, ghsspd: 6100, temp_max: 88,
      ina_bus_v: 27.6, ina_current_a: 4.2, ina_power_w: 116, err_crc: 0,
      chips: Array.from({ length: 12 }, (_, i) => ({ chip: i, temp_c: 70, volt_mv: 3600, pll_freq: [338, 358, 378, 398], ghsspd: 500, spd_dh: 4 })),
    }, 'application/json');
  }
  if (p === '/auth.js') return send(res, 200, '', 'text/javascript');
  if ((p === '/led' || p === '/firmware' || p === '/info') && url.searchParams.get('embed') !== '1') {
    return send(res, 200, `<script>location.replace('/dashboard#${p.slice(1)}')</script>`, 'text/html');
  }
  const file = pages[p];
  if (!file) return send(res, 404, 'no', 'text/plain');
  send(res, 200, readFileSync(path.join(root, file)), 'text/html');
});

function note(control, result, detail) {
  rows.push({ control, result, detail });
  console.log(result.padEnd(14), control, '—', detail);
}

await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`;
const browser = await puppeteer.launch({
  executablePath: chrome,
  headless: 'new',
  args: ['--no-sandbox', '--disable-dev-shm-usage'],
});
const page = await browser.newPage();
page.on('dialog', (d) => d.accept());

async function ready() {
  await page.goto(base + '/dashboard', { waitUntil: 'networkidle0', timeout: 20000 });
  await page.waitForFunction(() => document.body.innerText.includes('alpha.example'), { timeout: 8000 });
}

try {
  await ready();
  const menus = await page.$$eval('.menubtn', (els) => els.filter((e) => getComputedStyle(e).display !== 'none').map((e) => e.textContent.trim()));
  const expect = ['Dashboard', 'Power', 'Tuning', 'Fan', 'Fan Curve', 'Pool', 'LED', 'Firmware', 'Display', 'Info'];
  note('top menu', JSON.stringify(menus) === JSON.stringify(expect) ? 'pass' : 'fail', menus.join(', '));

  for (const key of ['power', 'tuning', 'fan', 'curve', 'display', 'led', 'firmware', 'info']) {
    await page.evaluate((k) => openMenu(k), key);
    const shown = await page.$eval('#menu-' + key, (el) => el.classList.contains('show'));
    note(key + ' popup', shown ? 'pass' : 'fail', shown ? 'sheet open' : 'sheet stayed closed');
    await page.evaluate(() => closeMenus());
  }

  await page.click('.menubtn[data-menu="pool"]');
  await page.waitForFunction(() => location.pathname === '/pool', { timeout: 5000 });
  const poolState = await page.evaluate(() => ({
    empty: [...document.querySelectorAll('[data-k="user"]')].every((el) => el.value === ''),
  }));
  note('pool page', poolState.empty ? 'pass' : 'fail', poolState.empty ? 'standalone /pool, worker field empty' : 'worker field was prefilled');

  await ready();
  await page.evaluate(() => openMenu('tuning'));
  await Promise.all([
    page.waitForNavigation({ waitUntil: 'domcontentloaded', timeout: 8000 }),
    page.click('#advancedTuning'),
  ]);
  await page.waitForSelector('#diagnostics');
  const anchors = await page.evaluate(() => ['psu-bypass', 'diagnostics', 'per-chip'].every((id) => !!document.getElementById(id)));
  note('advanced tuning', anchors ? 'pass' : 'fail', anchors ? '/tuning has bypass, diagnostics, per-chip' : 'anchors missing');

  await ready();
  state.writes.length = 0;
  await page.evaluate(() => openMenu('fan'));
  await page.click('#immersionToggle');
  await page.waitForFunction(() => document.getElementById('immersionMsg') && document.getElementById('immersionMsg').textContent.includes('Immersion on'));
  const imm = state.writes.find((w) => w.path.endsWith('/fan') && w.body.immersion === true);
  await page.reload({ waitUntil: 'networkidle0' });
  await page.waitForFunction(() => document.body.innerText.includes('alpha.example'));
  await page.evaluate(() => openMenu('fan'));
  const immBack = await page.$eval('#immersionToggle', (el) => el.checked);
  note('immersion', imm && immBack ? 'pass' : 'fail', `PATCH ${!!imm}, read-back ${immBack}`);

  state.writes.length = 0;
  await page.evaluate(() => openMenu('curve'));
  await page.waitForSelector('#curveSection', { visible: true });
  await page.$eval('#curvePoints input', (el) => { el.value = '45'; el.dispatchEvent(new Event('input', { bubbles: true })); });
  await page.click('#curveApply');
  await page.waitForFunction(() => document.getElementById('curveMsg') && /saved|fail|temperature|fan/i.test(document.getElementById('curveMsg').textContent));
  const curve = state.writes.find((w) => w.path.endsWith('/fan-curve'));
  const curveOk = curve && curve.body.points && curve.body.points[0].temp_c === 45;
  await page.reload({ waitUntil: 'networkidle0' });
  await page.waitForFunction(() => document.body.innerText.includes('alpha.example'));
  await page.evaluate(() => openMenu('curve'));
  await page.waitForFunction(() => {
    const el = document.querySelector('#curvePoints input');
    return el && el.value === '45';
  }, { timeout: 4000 }).catch(() => {});
  const curveBack = await page.$eval('#curvePoints input', (el) => el.value);
  note('fan curve', curveOk && curveBack === '45' ? 'pass' : 'fail', `request ${curveOk}, reload ${curveBack}`);

  state.writes.length = 0;
  await page.evaluate(() => openMenu('tuning'));
  await page.click('#psuBypassToggle');
  await page.waitForFunction(() => document.getElementById('psuBypassMsg') && document.getElementById('psuBypassMsg').textContent.includes('PSU bypass on'));
  const psu = state.writes.find((w) => w.path.endsWith('/psu') && w.body.psu_bypass === true);
  await page.reload({ waitUntil: 'networkidle0' });
  await page.waitForFunction(() => document.body.innerText.includes('alpha.example'));
  await page.evaluate(() => openMenu('tuning'));
  const psuBack = await page.$eval('#psuBypassToggle', (el) => el.checked);
  note('psu bypass popup', psu && psuBack ? 'pass' : 'fail', `PATCH ${!!psu}, read-back ${psuBack}`);

  state.writes.length = 0;
  state.system.psu_bypass = false;
  await page.goto(base + '/tuning', { waitUntil: 'domcontentloaded' });
  await page.waitForSelector('#psuBypassToggle');
  await page.click('#psuBypassToggle');
  await new Promise((r) => setTimeout(r, 300));
  const psuPage = state.writes.find((w) => w.path.endsWith('/psu') && w.body.psu_bypass === true);
  note('psu bypass /tuning', psuPage ? 'pass' : 'fail', psuPage ? 'PATCH from the advanced page' : 'no PATCH');

  state.writes.length = 0;
  await ready();
  await page.evaluate(() => openMenu('tuning'));
  await page.select('#tunePreset', '420,440,460,480');
  await page.click('#tuneApply');
  await page.waitForFunction(() => {
    const m = document.getElementById('tuneMsg');
    return m && m.textContent.length > 0;
  });
  const tune = state.writes.find((w) => w.path.endsWith('/tuning'));
  note('live tuning', tune ? 'pass' : 'fail', tune ? JSON.stringify(tune.body) : 'no PATCH');

  state.writes.length = 0;
  await page.evaluate(() => openMenu('display'));
  await page.select('#uitheme', 'minimal');
  await page.waitForFunction(() => document.body.classList.contains('theme-minimal'));
  const disp = state.writes.find((w) => w.path === '/api/v0/display' && w.body.theme === 'minimal');
  await page.reload({ waitUntil: 'networkidle0' });
  await page.waitForFunction(() => document.body.classList.contains('theme-minimal'), { timeout: 4000 }).catch(() => {});
  const dispBack = await page.evaluate(() => document.body.className);
  note('display theme', disp && dispBack.includes('minimal') ? 'pass' : 'fail', `PATCH ${!!disp}, reload ${dispBack}`);

  state.writes.length = 0;
  await page.evaluate(() => openMenu('led'));
  const frame = page.frames().find((f) => f.url().includes('/led'));
  if (!frame) note('led effect', 'fail', 'iframe did not load');
  else {
    await frame.waitForSelector('[data-effect]', { timeout: 5000 });
    await frame.click('[data-effect="breathe"]');
    await new Promise((r) => setTimeout(r, 400));
    const led = state.writes.find((w) => w.path.endsWith('/led') && w.body.effect === 'breathe');
    await page.reload({ waitUntil: 'networkidle0' });
    await page.evaluate(() => openMenu('led'));
    const frame2 = page.frames().find((f) => f.url().includes('/led'));
    await frame2.waitForFunction(() => {
      const el = document.querySelector('[data-effect="breathe"]');
      return el && el.classList.contains('on');
    }, { timeout: 4000 }).catch(() => {});
    const ledBack = await frame2.$eval('[data-effect="breathe"]', (el) => el.className);
    note('led effect', led && ledBack.includes('active') ? 'pass' : 'fail', `PATCH ${!!led}, reload class ${ledBack}`);
  }

  await page.evaluate(() => openMenu('power'));
  const powerOpen = await page.$eval('#menu-power', (el) => el.classList.contains('show'));
  note('power controls', powerOpen ? 'pass' : 'fail', 'sheet opens; reboot and ceiling writes are left for the hardware script');
  await page.evaluate(() => openMenu('firmware'));
  const fw = await page.$eval('#menu-firmware iframe', (el) => el.src.includes('/firmware?embed=1'));
  note('firmware popup', fw ? 'pass' : 'fail', 'embed iframe; upload is hardware-only');
  await page.evaluate(() => openMenu('info'));
  const info = await page.$eval('#menu-info iframe', (el) => el.src.includes('/info?embed=1'));
  note('info popup', info ? 'pass' : 'fail', 'embed iframe');

  note('pool latency', 'shipped as —', 'card renders an em dash until a rolling median is wired');
  note('all-time best share', 'shipped as —', 'card renders an em dash until persistence is wired');
} catch (err) {
  note('smoke', 'fail', err.message);
} finally {
  await browser.close();
  server.close();
}

const failed = rows.filter((r) => r.result === 'fail').length;
console.log('\n| control | result | detail |');
console.log('| --- | --- | --- |');
for (const r of rows) console.log(`| ${r.control} | ${r.result} | ${r.detail.replace(/\|/g, '/')} |`);
process.exit(failed ? 1 : 0);
