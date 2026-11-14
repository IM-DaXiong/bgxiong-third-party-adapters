// UI and MCP invoke the same declared action; submitting and collecting are separate.
// Closing the editor never loses the durable task or automatically resubmits it.
export function captureToolbar(editor, lastTask) {
  const bar = document.createElement('div'); bar.className = 'bgx-capture-toolbar';
  const number = (label, value, min, max, step) => {
    const wrap = document.createElement('label'); wrap.textContent = label;
    const input = document.createElement('input'); input.type = 'number'; input.value = value;
    input.min = min; input.max = max; input.step = step; input.setAttribute('aria-label', label);
    wrap.append(input); bar.append(wrap); return input;
  };
  const mp = number('百万像素', 2, 0.1, 16, 0.1), multiple = number('尺寸倍数', 8, 8, 128, 4);
  const capture = document.createElement('button'); capture.textContent = '取景与遮罩';
  const collect = document.createElement('button'); collect.textContent = '收取取景';
  const state = document.createElement('span'); state.setAttribute('role', 'status');
  bar.append(capture, collect, state); document.body.append(bar);
  document.body.classList.add('bgx-capture-enabled');
  let busy = false;
  async function run(action, args) {
    if (busy) return;
    busy = true; capture.disabled = collect.disabled = true; state.textContent = '处理中';
    try {
      const before = await editor.describe();
      const after = await editor.act({ action, arguments: args, expectedStateDigest: before.stateDigest });
      state.textContent = after.service.status === 'pending' ? '等待渲染' : '已收取';
    } catch (error) { state.textContent = String(error.message ?? error); }
    finally { busy = false; capture.disabled = false; collect.disabled = !lastTask(); }
  }
  collect.disabled = !lastTask();
  capture.onclick = () => run('capture.frame', { megapixels: Number(mp.value), multiple: Number(multiple.value) });
  collect.onclick = () => run('capture.collect', { taskId: lastTask() });
  return { refresh(result) {
    collect.disabled = busy || !lastTask();
    state.textContent = result.status === 'pending' ? '等待渲染' : '已收取';
  }, remove() { bar.remove(); document.body.classList.remove('bgx-capture-enabled'); } };
}
