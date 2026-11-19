import { readFileSync } from 'node:fs';
import { expect, it } from 'vitest';

it('SITE-05 workflow passes a base only for pull requests and retains standalone tooling', () => {
  const workflow = readFileSync('template/site/workflow.yml', 'utf8');
  expect(workflow.match(/^\s+BASE:.*$/gm)).toEqual([
    "          BASE: ${{ github.event_name == 'pull_request' && format('origin/{0}', github.base_ref) || '' }}",
  ]);
  expect(workflow).toContain('          ARGS=""; [ -n "$BASE" ] && ARGS="--base $BASE"\n');
  expect(workflow).toContain('          npx -y @ws42/guardrails@${GUARDRAILS_VERSION} check --only docs,lint,arch,gaming,journeys $ARGS\n');
  expect(workflow.match(/--base/g)).toHaveLength(1);
  expect(workflow.match(/^\s*GUARDRAILS_VERSION:/gm)).toHaveLength(1);
  expect(workflow).not.toMatch(/playwright|cuj/i);
});
