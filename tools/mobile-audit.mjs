/**
 * Does the app still fit a phone?
 *
 * Three checks, all of them things that have actually gone wrong here:
 *
 * 1. Nothing sticks out sideways. A single element wider than the viewport makes
 *    the whole DOCUMENT wider, and Chrome on Android answers that by scaling the
 *    entire page down — navigation bar included — and by making vertical scrolling
 *    fight a horizontal one. It looks like three unrelated bugs and it is one.
 * 2. The bottom bar is on screen with its full height, at the top of the page AND
 *    after scrolling. It is `position: sticky`, so unlike the URL-bar behaviour it
 *    depends on, this part IS reproducible headlessly: a sticky element that is
 *    not pinned to the scrollport shows up here immediately.
 *
 * Run (needs nothing installed but Docker):
 *
 *   docker run --rm --network host \
 *     -e BASE_URL=http://127.0.0.1:3100 -e USER_NAME=... -e PASSWORD=... \
 *     -v "$PWD/tools:/w" -w /w mcr.microsoft.com/playwright:v1.56.0-noble \
 *     node mobile-audit.mjs
 *
 * Exits non-zero and names the offending elements, so it can gate a release.
 */
import { chromium, devices } from 'playwright';

const BASE = process.env.BASE_URL ?? 'http://127.0.0.1:3100';
const USER = process.env.USER_NAME;
const PASSWORD = process.env.PASSWORD;

const ROUTES = [
  '/dashboard', '/buchungen', '/monate', '/auswertung', '/steuer',
  '/vorlagen', '/kitchenowl', '/kitchenowl?ansicht=analysis', '/kategorien', '/import',
  '/einstellungen', '/schnell',
];

// A small phone. Anything that fits here fits everything else.
const VIEWPORT = { width: 390, height: 844 };

const browser = await chromium.launch();
const context = await browser.newContext({
  ...devices['Pixel 7'],
  viewport: VIEWPORT,
  ignoreHTTPSErrors: true,
});
const page = await context.newPage();

const problems = [];

await page.goto(`${BASE}/anmelden`, { waitUntil: 'networkidle' });
if (USER && PASSWORD) {
  await page.fill('input[name="username"], input#username', USER);
  await page.fill('input[type="password"]', PASSWORD);
  await page.click('button[type="submit"]');
  await page.waitForLoadState('networkidle');
  await page.waitForTimeout(800);
  console.log(`signed in as ${USER} — landed on ${page.url()}`);
}

// The session cookie carries `Secure` whenever APP_PUBLIC_URL is https, and a
// browser then refuses to store it over plain http. Auditing http://127.0.0.1
// therefore signs in "successfully" and stays logged out; use the public URL.

for (const route of ROUTES) {
  await page.goto(`${BASE}${route}`, { waitUntil: 'networkidle' });
  // Charts and tables render after their query resolves; a check that runs first
  // passes for the wrong reason.
  await page.waitForTimeout(900);

  const report = await page.evaluate((width) => {
    const doc = document.documentElement;
    // Every app screen renders the shell. Without this the check passes for the
    // wrong reason the moment the session is not established — a redirect to the
    // login page is narrow, tidy and completely uninformative.
    const authed = Boolean(document.querySelector('.mobile-bar'));
    const offenders = [];
    // Only the widest few matter: a child sticking out is usually its parent's
    // fault, and listing a hundred nested nodes hides the one that counts.
    for (const el of document.querySelectorAll('body *')) {
      const r = el.getBoundingClientRect();
      if (r.width === 0 || r.height === 0) continue;
      if (r.right > width + 1 || r.left < -1) {
        offenders.push({
          selector:
            el.tagName.toLowerCase() +
            (el.id ? `#${el.id}` : '') +
            (typeof el.className === 'string' && el.className
              ? `.${el.className.trim().split(/\s+/).join('.')}`
              : ''),
          left: Math.round(r.left),
          right: Math.round(r.right),
          scrollWidth: el.scrollWidth,
        });
      }
    }
    // The bottom bar must be laid out inside the screen, and the DOCUMENT must
    // not scroll — the app scrolls inside its shell. Both together are what keep
    // the browser's URL bar out of the layout: it cannot hide if nothing scrolls
    // it, so there is no viewport transition for a bar to be caught in.
    const bar = document.querySelector('.mobile-bar');
    const barRect = bar ? bar.getBoundingClientRect() : null;
    return {
      authed,
      docScrollWidth: doc.scrollWidth,
      bodyScrollWidth: document.body.scrollWidth,
      docScrollHeight: (document.scrollingElement ?? doc).scrollHeight,
      innerHeight: window.innerHeight,
      bar: barRect
        ? { top: Math.round(barRect.top), bottom: Math.round(barRect.bottom), height: Math.round(barRect.height) }
        : null,
      offenders: offenders.slice(0, 8),
    };
  }, VIEWPORT.width);

  if (!report.authed && route !== '/schnell') {
    console.error(`! ${route} — not signed in; the check would pass vacuously. Set USER_NAME/PASSWORD.`);
    process.exitCode = 2;
    continue;
  }
  const faults = [];
  if (report.docScrollWidth > VIEWPORT.width + 1) {
    faults.push(`document is ${report.docScrollWidth}px wide, viewport is ${VIEWPORT.width}px`);
    for (const o of report.offenders) faults.push(`  sticks out: ${o.selector} [${o.left}…${o.right}]`);
  }
  if (route !== '/schnell') {
    if (!report.bar || report.bar.height === 0) {
      faults.push('no bottom bar rendered');
    } else if (report.bar.bottom > report.innerHeight + 1 || report.bar.top < 0) {
      faults.push(
        `bottom bar is off-screen (top ${report.bar.top}, bottom ${report.bar.bottom}, ` +
          `viewport ${report.innerHeight})`,
      );
    }

    // Scrolled, which is when a bar that is merely at the end of the document
    // rather than stuck to the scrollport disappears upward.
    if (report.docScrollHeight > report.innerHeight + 200) {
      await page.evaluate(() => window.scrollTo(0, 400));
      await page.waitForTimeout(250);
      const after = await page.evaluate(() => {
        const el = document.querySelector('.mobile-bar');
        if (!el) return null;
        const r = el.getBoundingClientRect();
        return { top: Math.round(r.top), bottom: Math.round(r.bottom), y: Math.round(window.scrollY) };
      });
      if (!after || after.bottom > VIEWPORT.height + 1 || after.top < 0) {
        faults.push(
          `bottom bar left the screen after scrolling to ${after?.y}px ` +
            `(top ${after?.top}, bottom ${after?.bottom})`,
        );
      }
    }
  }

  if (faults.length > 0) {
    problems.push({ route, faults });
    console.log(`✗ ${route}`);
    for (const f of faults) console.log(`    ${f}`);
  } else {
    console.log(`✓ ${route}`);
  }
}

await browser.close();

if (problems.length > 0) {
  console.error(`\n${problems.length} route(s) wider than the phone viewport.`);
  process.exit(1);
}
console.log('\nNothing sticks out sideways.');
