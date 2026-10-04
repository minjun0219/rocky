/**
 * 웹 UI E2E — `bun run e2e`(= `playwright test`). 격리 데몬은 `e2e/global-setup.ts` 가 한 번 띄운다.
 *
 * 화면 셋: phone(390·터치·light) · cmux 옆 창(360·dark) · desktop(1280·light).
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

export default defineConfig({
  testDir: 'e2e',
  globalSetup: './e2e/global-setup.ts',
  globalTeardown: './e2e/global-teardown.ts',
  fullyParallel: true,
  forbidOnly: ci,
  retries: ci ? 1 : 0,
  reporter: ci ? [['list'], ['html', { open: 'never' }], ['github']] : [['list']],
  use: {
    channel: channel(),
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    actionTimeout: 6000,
  },
  projects: [
    {
      name: 'phone',
      use: {
        viewport: { width: 390, height: 844 },
        isMobile: true,
        hasTouch: true,
        deviceScaleFactor: 2,
        colorScheme: 'light',
      },
    },
    {
      name: 'cmux',
      use: { viewport: { width: 360, height: 860 }, deviceScaleFactor: 2, colorScheme: 'dark' },
    },
    {
      name: 'desktop',
      use: { viewport: { width: 1280, height: 860 }, deviceScaleFactor: 1, colorScheme: 'light' },
    },
  ],
});
