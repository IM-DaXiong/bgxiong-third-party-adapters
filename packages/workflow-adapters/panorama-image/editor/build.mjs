import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFile, writeFile, mkdir, cp, readdir } from 'node:fs/promises';
import { dirname, resolve, relative } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

// Builds the unchanged, pinned upstream editor. No clone, dependency install, model download,
// ComfyUI restart or installation side effects. The caller supplies a prepared source checkout.
const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const option = name => args[args.indexOf(name) + 1];
if (!args.includes('--source') || !args.includes('--out')) throw new Error('Usage: node build.mjs --source <checkout> --out <new-directory>');
const source = resolve(option('--source'));
const out = resolve(option('--out'));
const lock = JSON.parse(await readFile(resolve(here, 'upstream.lock.json'), 'utf8'));
const git = (...values) => execFileSync('git', ['-C', source, ...values], { encoding: 'utf8' }).trim();
if (git('rev-parse', 'HEAD') !== lock.commit) throw new Error('Upstream commit differs from source lock');
if (git('diff', 'HEAD', '--name-only')) throw new Error('Tracked upstream source has modifications');
const hash = data => createHash('sha256').update(data).digest('hex');
for (const [file, expected] of [['package-lock.json', lock.packageLockSha256], ['LICENSE', lock.licenseSha256]]) {
  if (hash(await readFile(resolve(source, file))) !== expected) throw new Error(`Upstream digest differs: ${file}`);
}
const font = (await readFile(resolve(source, 'web/fonts/PlusJakartaSans-Latin.woff2'))).toString('base64');
const inlineCss = css => css.replace('./fonts/PlusJakartaSans-Latin.woff2', `data:font/woff2;base64,${font}`);
const css = inlineCss(await readFile(resolve(source, 'web/pano_studio.css'), 'utf8')) + '\n' + await readFile(resolve(here, 'embedded.css'), 'utf8');
const styleBindings = new Map();
for (const [module, file] of [['web_src/studio/stylesheet.js', 'pano_studio.css'], ['web_src/pano_styles.js', 'pano_editor.css']]) {
  const text = inlineCss(await readFile(resolve(source, 'web', file), 'utf8'));
  styleBindings.set(resolve(source, module).replaceAll('\\', '/'), {
    expression: `new URL("./${file}", import.meta.url).toString()`,
    value: JSON.stringify(`data:text/css;base64,${Buffer.from(text).toString('base64')}`),
  });
}
await mkdir(out); // Never replace an existing candidate or installed package.
const { build } = await import(pathToFileURL(resolve(source, 'node_modules/vite/dist/node/index.js')));
const { default: vue } = await import(pathToFileURL(resolve(source, 'node_modules/@vitejs/plugin-vue/dist/index.mjs')));
await build({
  configFile: false,
  root: source,
  plugins: [{ name: 'offline-editor-styles', enforce: 'pre', transform(code, id) {
    const normalized = id.replaceAll('\\', '/');
    if (normalized === resolve(source, 'web_src/studio/editor.js').replaceAll('\\', '/')) {
      const original = 'return { close, session, stage: () => ctx.stage, paint: () => paint };';
      if (code.split(original).length !== 2) throw new Error('Upstream controller export changed');
      return code.replace(original, 'return { close, session, stage: () => ctx.stage, paint: () => paint, controller: ctx.controller, view: () => ctx.view, refresh: () => { ctx.refreshVideo(); ctx.sync(); } };');
    }
    if (normalized === resolve(source, 'web_src/pano_editor_state.js').replaceAll('\\', '/')) {
      const original = 'export const HISTORY_LIMIT = 80;';
      if (code.split(original).length !== 2) throw new Error('Upstream history contract changed');
      return code.replace(original, 'export const HISTORY_LIMIT = 51;'); // initial state + 50 successful actions
    }
    if (normalized === resolve(source, 'web_src/pano_scene_state.js').replaceAll('\\', '/')) {
      const original = 'return version === 1 ? migrateV1(parsed) : normalizeV2(parsed);';
      if (code.split(original).length !== 2) throw new Error('Upstream state loader changed');
      return code.replace(original, 'const state = version === 1 ? migrateV1(parsed) : normalizeV2(parsed); if (Object.hasOwn(parsed, "bgxGeometry")) state.bgxGeometry = structuredClone(parsed.bgxGeometry); return state;');
    }
    const binding = styleBindings.get(normalized);
    if (!binding) return null;
    if (code.split(binding.expression).length !== 2) throw new Error(`Upstream stylesheet loader changed: ${id}`);
    return code.replace(binding.expression, binding.value);
  } }, vue()],
  define: { 'process.env.NODE_ENV': JSON.stringify('production') },
  resolve: { alias: { '@panorama-upstream': source } },
  build: {
    outDir: out,
    emptyOutDir: false,
    assetsInlineLimit: 0,
    lib: { entry: resolve(here, 'studio.js'), formats: ['es'], fileName: () => 'studio.js' },
    rollupOptions: { output: { inlineDynamicImports: true } },
  },
});
await cp(resolve(source, 'web/pano_studio.css'), resolve(out, 'studio.css'));
// Keep relative CSS resources and their licenses. These are part of the offline editor,
// not resources to retrieve from an upstream CDN at runtime.
await cp(resolve(source, 'web/fonts'), resolve(out, 'fonts'), { recursive: true });
await cp(resolve(source, 'web/pano_editor.css'), resolve(out, 'pano_editor.css'));
await cp(resolve(source, 'web/pano_studio.css'), resolve(out, 'pano_studio.css'));
await cp(resolve(source, 'LICENSE'), resolve(out, 'UPSTREAM-LICENSE.txt'));
await cp(resolve(here, 'upstream.lock.json'), resolve(out, 'upstream.lock.json'));
// A self-contained payload for an opaque-origin frame: no filesystem URL, CDN or
// executable import from the trusted host is needed when opening the editor.
await writeFile(resolve(out, 'editor-bundle.json'), JSON.stringify({ schemaVersion: 1, assetTransport:'blob-v1', script: await readFile(resolve(out, 'studio.js'), 'utf8'), stylesheet: css,
  documentCompatibility: { schemaVersion:1, fromVersions:['1.1.0','1.3.0','1.5.0','1.6.0','1.7.0','1.8.0','1.9.0','1.10.0','1.11.0','1.12.0','1.13.0','1.13.1','1.14.1','1.14.2','1.15.1','1.16.0','1.17.0','1.18.0','1.19.0','1.20.0'] },
  generationBindings: ['editor-geometry','h3-studio-video','studio-video','scene-to-panorama','studio-render','studio-cutout','studio-3d','studio-moge','editor-frame-capture','editor-frame-return','editor-seam-prepare','editor-seam-return'].map(operation => ({ operationId: `local.thirdparty.comfy-panorama-image/${operation}`, documentField: 'editorDocument', assetsField: 'references', includeConnectedDocuments: true })),
  serviceBindings: [{ id: 'file3d', purpose: 'mediaPreparation', operationId: 'local.thirdparty.comfy-panorama-image/editor-3d-preview', modelId: 'panorama-editor-media', assetsField: 'sources' },
    { id: 'geometry', purpose: 'mediaPreparation', operationId: 'local.thirdparty.comfy-panorama-image/editor-geometry', modelId: 'panorama-editor-geometry', assetsField: 'references', revisionField: 'sourceRevision' },
    { id: 'geometryModels', purpose: 'mediaPreparation', operationId: 'local.thirdparty.comfy-panorama-image/editor-geometry-models', modelId: 'panorama-editor-geometry', assetsField: 'sources' },
    { id: 'frameCapture', purpose: 'mediaPreparation', operationId: 'local.thirdparty.comfy-panorama-image/editor-frame-capture', modelId: 'panorama-editor-capture', assetsField: 'references', revisionField: 'sourceRevision' },
    ...[['frameReturn','frame-return'],['seamPrepare','seam-prepare'],['seamReturn','seam-return']].map(([id, mode]) => ({id, purpose:'mediaPreparation', operationId:`local.thirdparty.comfy-panorama-image/editor-${mode}`,modelId:`panorama-editor-${mode}`,assetsField:'references',revisionField:'sourceRevision'}))],
}));
const files = [];
async function inventory(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = resolve(dir, entry.name);
    if (entry.isDirectory()) await inventory(path);
    else { const bytes = await readFile(path); files.push({ path: relative(out, path).replaceAll('\\', '/'), bytes: bytes.length, sha256: hash(bytes) }); }
  }
}
await inventory(out);
files.sort((a, b) => a.path.localeCompare(b.path));
const bridgeSources = [];
for (const path of ['build.mjs', 'studio.js', 'actions.js', 'capture-toolbar.js', 'state-preservation.js', 'geometry-state.js', 'geometry-toolbar.js', 'connected-scenes.js', 'embedded.css']) bridgeSources.push({ path, sha256: hash(await readFile(resolve(here, path))) });
await writeFile(resolve(out, 'editor-build.json'), JSON.stringify({ schemaVersion: 1, upstream: lock,
  upstreamLockSha256: hash(await readFile(resolve(here, 'upstream.lock.json'))), bridgeSources, files }, null, 2) + '\n');
console.log(JSON.stringify({ output: out, upstream: lock.commit, files: files.length }));
