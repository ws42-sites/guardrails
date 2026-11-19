import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './e2e/cuj',
  fullyParallel: false,
  retries: 0,
  workers: 2,
  use: { baseURL: process.env.CUJ_BASE_URL, trace: 'retain-on-failure', serviceWorkers: 'block' },
  reporter: [['list'], ['json', { outputFile: process.env.PLAYWRIGHT_JSON_OUTPUT_NAME ?? '.guardrails/journeys-results.json' }]],
});
