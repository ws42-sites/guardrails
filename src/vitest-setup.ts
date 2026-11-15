import { beforeEach, expect } from 'vitest';

/** Add to vitest setupFiles: every test must make at least one assertion. */
beforeEach(() => {
  expect.hasAssertions();
});
