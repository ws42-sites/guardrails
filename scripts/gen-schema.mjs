// Regenerates schema/guardrails.schema.json from the zod schema (run after build).
import { writeFileSync, mkdirSync } from 'node:fs';
import { zodToJsonSchema } from 'zod-to-json-schema';
import { configSchema } from '../dist/config.js';

mkdirSync('schema', { recursive: true });
const schema = zodToJsonSchema(configSchema, { name: 'GuardrailsConfig', $refStrategy: 'none' });
writeFileSync('schema/guardrails.schema.json', `${JSON.stringify(schema, null, 2)}\n`);
console.log('wrote schema/guardrails.schema.json');
