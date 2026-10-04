/**
 * 웹 UI E2E — `bun run e2e`(= `playwright test`). 격리 데몬은 `e2e/global-setup.ts` 가 한 번 띄운다.
 *
 * 화면 넷: phone(390·터치·light) · cmux 옆 창(360·dark) · desktop(1280·light) · cmux-webkit.
 * cmux 의 브라우저 창은 WKWebView(Safari 엔진)라, 같은 cmux 화면을 Playwright WebKit 으로도 돈다 —
 * Chromium 에서만 맞는 CSS·동작을 잡으려고(`bunx playwright install webkit`, CI 는 `--with-deps`).
 * 브라우저: Playwright 가 받아 둔 Chromium(`bunx playwright install chromium`)이 이 버전에 맞으면 그것,
 * 아니면 설치된 Google Chrome(`channel: 'chrome'`). CI 는 러너에 깔린 Chrome 을 써서 내려받지 않는다.
 * `E2E_CHANNEL` 로 강제할 수 있다(`chromium` 이면 받아 둔 것).
 */
import { existsSync } from 'node:fs';
import { chromium, defineConfig } from '@playwright/test';

const ci = !!process.env.CI;

function channel(): string | undefined {
  const forced = process.env.E2E_CHANNEL;
  if (forced) {
    return forced === 'chromium' ? undefined : forced;
  }
  if (ci) {
    return 'chrome';
  }
  return existsSync(chromium.executablePath()) ? undefined : 'chrome';
}

/** Chromium 프로젝트에만 거는 channel — WebKit 프로젝트에 걸면 "Unsupported webkit channel" 로 죽는다. */
const chromeChannel = channel();

export default defineConfig({
  testDir: 'e2e',
  globalSetup: './e2e/global-setup.ts',
  globalTeardown: './e2e/global-teardown.ts',
  fullyParallel: true,
  forbidOnly: ci,
  retries: ci ? 1 : 0,
  reporter: ci ? [['list'], ['html', { open: 'never' }], ['github']] : [['list']],
  use: {
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    actionTimeout: 6000,
  },
  projects: [
    {
      name: 'phone',
      use: {
        channel: chromeChannel,
        viewport: { width: 390, height: 844 },
        isMobile: true,
        hasTouch: true,
        deviceScaleFactor: 2,
        colorScheme: 'light',
      },
    },
    {
      name: 'cmux',
      use: {
        channel: chromeChannel,
        viewport: { width: 360, height: 860 },
        deviceScaleFactor: 2,
        colorScheme: 'dark',
      },
    },
    {
      name: 'cmux-webkit',
      // channel(Chrome)은 Chromium 전용이라 이 프로젝트엔 두지 않는다(아래 Chromium 프로젝트에만).
      use: {
        browserName: 'webkit',
        viewport: { width: 360, height: 860 },
        deviceScaleFactor: 2,
        colorScheme: 'dark',
      },
    },
    {
      name: 'desktop',
      use: {
        channel: chromeChannel,
        viewport: { width: 1280, height: 860 },
        deviceScaleFactor: 1,
        colorScheme: 'light',
      },
    },
  ],
});
