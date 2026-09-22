// An isolated server fixture. Never inherit deployment configuration or paths.
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
const directory = await mkdtemp(join(tmpdir(), 'keryx-tags-browser-'));
const config = join(directory, 'config.toml');
await writeFile(config, '');
const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('KERYX_')));
const server = spawn('target/debug/keryx', ['--config', config, 'serve', '--port', '17823', '--host', '127.0.0.1', '--storage', 'disk', '--db', join(directory, 'test.db'), '--data-dir', directory], { env, stdio: 'inherit' });
for (const signal of ['SIGTERM', 'SIGINT']) process.on(signal, () => server.kill(signal));
server.on('exit', async code => { await rm(directory, { recursive: true, force: true }); process.exit(code || 0); });
