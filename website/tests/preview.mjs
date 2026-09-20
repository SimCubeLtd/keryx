import { preview } from 'astro';

// Use the API so Playwright owns the process, including in agent environments
// where the Astro CLI automatically starts its preview in the background.
await preview({ server: { host: '127.0.0.1', port: 4329 } });
