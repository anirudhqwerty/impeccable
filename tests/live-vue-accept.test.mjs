import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { chromium } from 'playwright';
import { bootFixtureSession, FIXTURES_DIR } from './live-e2e/session.mjs';
import { waitForHandshake, pickElement, clickGo, waitForCycling, openTunePanel, setTuneRange, closeTunePanel, clickAccept, waitForBarHidden } from './live-e2e/ui.mjs';

test('Vue preview → Tune → Accept → Reload preserves style and behavior', { timeout: 240000 }, async (t) => {
  const browser = await chromium.launch({ headless: true, ...(process.env.IMPECCABLE_TEST_CHROME ? { executablePath: process.env.IMPECCABLE_TEST_CHROME } : {}) });
  const fixture = JSON.parse(readFileSync(join(FIXTURES_DIR, 'vite-vue-stateful', 'fixture.json')));
  let session;
  let cleanupPromise;
  const cleanup = () => cleanupPromise ||= (async () => {
    await browser.close();
    if (session) {
      // This PID belongs to the sandboxed fixture server returned by boot.
      // Closing it releases an outstanding agent poll before teardown awaits it.
      try { process.kill(session.live.pid, 'SIGTERM'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
      await session.teardown();
    }
  })();
  t.after(cleanup);
  try {
    session = await bootFixtureSession({ name: 'vite-vue-stateful', fixture, browser,
      wrapTarget: { file: 'src/App.vue', classes: 'card', tag: 'section' },
      agent: { autoRepairMountFailures: false, async generateVariants(event) {
        return { scopedCss: '', variants: Array.from({ length: event.count }, () => ({
          vueComponent: { css: '.card { width: 400px; min-height: 80px; padding: calc(var(--p-space, 8) * 1px); color: #111; background: #ffd6d6; }' },
          params: [{ id: 'space', kind: 'range', min: 4, max: 32, step: 1, default: 8, label: 'Space' }],
        })) };
      } }, log: message => t.diagnostic(message),
    });
    const { page, tmp } = session;
    await waitForHandshake(page);
    const source = readFileSync(join(tmp, 'src/App.vue'), 'utf8');
    await pickElement(page, 'section.card', { position: { x: 4, y: 4 } });
    await clickGo(page);
    await waitForCycling(page, 3);
    assert.equal(readFileSync(join(tmp, 'src/App.vue'), 'utf8'), source, 'generation must not rewrite source');
    await openTunePanel(page);
    await setTuneRange(page, 'Space', 23);
    await closeTunePanel(page);
    await page.waitForFunction(() => getComputedStyle(document.querySelector('section.card')).paddingLeft === '23px', null, { timeout: 15000 });
    const phase = await page.evaluate(() => Object.keys(localStorage).map(key => {
      try { return JSON.parse(localStorage.getItem(key)); } catch { return null; }
    }).find(value => value?.state === 'CYCLING')?.generationPhase);
    assert.ok(phase, 'generation phase must be persisted before reload');
    await page.reload();
    await waitForHandshake(page);
    await waitForCycling(page, 3);
    await page.waitForFunction(() => getComputedStyle(document.querySelector('section.card')).paddingLeft === '23px', null, { timeout: 15000 });
    await clickAccept(page);
    await waitForBarHidden(page);
    await page.waitForFunction(() => !document.querySelector('[data-impeccable-variants]'), null, { timeout: 15000 });
    await page.reload();
    await page.waitForSelector('section.card');
    assert.equal(await page.locator('section.card').evaluate(el => getComputedStyle(el).paddingLeft), '23px');
    assert.equal(await page.locator('.unrelated').evaluate(el => getComputedStyle(el).paddingLeft), '3px');
    await page.locator('section.card').click();
    await page.waitForFunction(() => document.querySelector('section.card').textContent.includes('Imports: 1'), null, { timeout: 15000 });
    const accepted = readFileSync(join(tmp, 'src/App.vue'), 'utf8');
    assert.equal(accepted.split('<template>')[0], source.split('<template>')[0]);
    assert.equal(accepted.split('<style>')[1], source.split('<style>')[1]);
    assert.doesNotMatch(accepted, /data-impeccable-|var\(--p-/);
    assert.deepEqual(readdirSync(join(tmp, 'node_modules/.impeccable-live')).filter(name => /^[a-f0-9]{8}$/.test(name)), []);
    execFileSync('npm', ['run', 'build'], { cwd: tmp, stdio: 'pipe' });
  } catch (error) {
    if (session) t.diagnostic(JSON.stringify(await session.page.evaluate(() => ({
      cards: [...document.querySelectorAll('section.card')].map(el => ({ html: el.outerHTML.slice(0, 500), padding: getComputedStyle(el).paddingLeft })),
      state: window.__IMPECCABLE_LIVE_CHROME_CORE__?.debugState?.(),
    })).catch(() => null)));
    throw error;
  } finally { await cleanup(); }
});
