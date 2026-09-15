import { chromium, devices } from 'playwright';
const BASE = process.env.BASE_URL ?? 'http://127.0.0.1:3100';
const USER = process.env.USER_NAME, PASSWORD = process.env.PASSWORD;
const browser = await chromium.launch();

async function shot(name, opts, url) {
  const ctx = await browser.newContext(opts);
  const page = await ctx.newPage();
  await page.goto(`${BASE}/anmelden`, { waitUntil: 'networkidle' });
  await page.fill('input[name="username"], input#username', USER);
  await page.fill('input[type="password"]', PASSWORD);
  await page.click('button[type="submit"]');
  await page.waitForLoadState('networkidle');
  await page.goto(`${BASE}${url}`, { waitUntil: 'networkidle' });
  await page.waitForTimeout(1200);
  const wide = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1);
  console.log(name, page.url(), 'overflow:', wide);
  await page.screenshot({ path: `/out/${name}.png`, fullPage: true });
  await ctx.close();
}

await shot('flow-desktop', { viewport: { width: 1280, height: 900 } }, '/auswertung?jahr=2026&ansicht=fluss');
await shot('flow-mobile', { ...devices['Pixel 7'], viewport: { width: 390, height: 844 } }, '/auswertung?jahr=2026&ansicht=fluss');
await shot('flow-month', { viewport: { width: 1280, height: 900 } }, '/auswertung?jahr=2026&ansicht=fluss&monat=6');
await browser.close();
