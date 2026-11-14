import { execFileSync } from 'node:child_process';
import { readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Read-only drift report. Checking an upstream revision never updates a production lock.
const here = dirname(fileURLToPath(import.meta.url));
const lock = JSON.parse(await readFile(resolve(here, 'upstream.lock.json'), 'utf8'));
const args = process.argv.slice(2);
const option = key => args.includes(key) ? args[args.indexOf(key) + 1] : null;
if (!option('--source') || !option('--out')) throw new Error('Usage: --source <checkout> --out <report.json> [--compare <commit>] [--remote]');
const source = resolve(option('--source'));
const git = (...values) => execFileSync('git', ['-C', source, ...values], { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 }).trim();
const target = git('rev-parse', `${option('--compare') ?? 'HEAD'}^{commit}`);
const category = path => {
  if (path.startsWith('tests/')) return 'behavior-tests';
  if (path.startsWith('docs/')) return 'upstream-specifications';
  if (/^(?:nodes(?:\/|\.py)|__init__\.py|core_adapters\.py)/.test(path)
      || /^comfyui_pano_suite\/(?:nodes|studio_node|preview_node|seam_prep|core_adapters)\.py$/.test(path)) return 'nodes-and-ports';
  if (/routes|upload|resolver|asset|file_?3d/.test(path)) return 'asset-routes-and-persistence';
  if (/state|handoff|timeline|camera|pose|lens|render|depth|space/.test(path)) return 'scene-render-time-contract';
  if (path.startsWith('web_src/')) return 'editor-actions-and-ui';
  if (path.startsWith('web/')) return 'built-editor-resources';
  if (/workflow|example/i.test(path)) return 'workflows-and-models';
  if (/package|requirements|pyproject|LICENSE|lock/i.test(path)) return 'dependencies-and-license';
  return 'other-requires-review';
};
const diff = git('diff', '--name-status', '--no-renames', lock.commit, target, '--');
const changes = diff ? diff.split('\n').map(line => {
  const [status, ...parts] = line.split('\t');
  const path = parts.join('\t');
  return { status, path, category: category(path) };
}) : [];
const remote = args.includes('--remote')
  ? execFileSync('git', ['ls-remote', lock.repository, 'refs/heads/main'], { encoding: 'utf8', timeout: 30_000 }).trim().split(/\s+/)[0]
  : null;
if (args.includes('--remote') && !/^[0-9a-f]{40}$/.test(remote)) throw new Error('Cannot resolve upstream main; freshness is unknown');
const report = {
  checkedAt: new Date().toISOString(), lockedCommit: lock.commit, comparedCommit: target,
  remoteMain: remote, remoteChecked: args.includes('--remote'),
  sourceMatchesLock: target === lock.commit, remoteMatchesLock: remote ? remote === lock.commit : null,
  changes, requiresReview: changes.length > 0 || Boolean(remote && remote !== lock.commit),
  note: 'File classification is an inventory, not semantic approval or runtime acceptance. Unchanged version numbers do not imply unchanged behavior.',
};
await writeFile(resolve(option('--out')), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ report: resolve(option('--out')), changes: changes.length, remoteMatchesLock: report.remoteMatchesLock }));
if (report.requiresReview) process.exitCode = 2;
