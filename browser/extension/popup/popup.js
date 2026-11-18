// CUA popup: connection status, token (masked by default), port, shield mode, session info.
// Codex absorb wave（契约 §3）：安全凭据（secret.request）面板 + 高级安全开关面板。

const $ = (id) => document.getElementById(id);

let fullToken = '';
let revealed = false;

function renderToken() {
  $('token').textContent = fullToken
    ? (revealed ? fullToken : fullToken.slice(0, 6) + ' … ' + fullToken.slice(-4))
    : '…';
  $('reveal').textContent = revealed ? '隐藏' : '显示';
}

async function refresh() {
  const st = await chrome.runtime.sendMessage({ type: 'CUA_POPUP_STATE' });
  if (!st) return;
  const badge = $('status');
  badge.textContent =
    st.connected ? '已连接' : st.status === 'connecting' || st.status === 'waiting_auth' ? '连接中' : '未连接';
  badge.className = 'status ' + (st.connected ? 'on' : st.status === 'off' ? 'off' : 'mid');
  if (st.token !== fullToken) revealed = false;
  fullToken = st.token || '';
  renderToken();
  $('port').value = st.port;
  $('enabled').checked = !!st.enabled;
  $('humanMode').checked = !!st.human_mode;
  if (document.activeElement !== $('browserName')) $('browserName').value = st.browser_name || 'default';
  setShield(st.shield_mode, true);
  renderSecurity(st);
  renderSecRequests(st.sec_requests);
  $('session').textContent = st.session
    ? `会话 ${st.session.id} · 自建 ${st.session.created} · 接管 ${st.session.claimed}`
    : '会话：无（agent 调 session.start / tab.create 时自动建立）';
  $('audit').textContent = (st.audit_tail || [])
    .map((a) => `${new Date(a.t).toLocaleTimeString()} ${a.action} ${a.ok ? '✓' : '✗ ' + (a.error || '')}`)
    .join('\n');
}

async function saveNow() {
  const res = await chrome.runtime.sendMessage({
    type: 'CUA_POPUP_CONFIG',
    port: Number($('port').value),
    enabled: $('enabled').checked,
    shield_mode: $('shieldMode').value,
    human: $('humanMode').checked,
    browser_name: ($('browserName').value.trim() || 'default').toLowerCase(),
  });
  if (res && res.ok === false) alert(res.error || '保存失败');
  refresh();
}

// toggles apply immediately — the 2s state poll would otherwise revert them
$('enabled').addEventListener('change', saveNow);
$('humanMode').addEventListener('change', saveNow);
$('shieldMode').addEventListener('change', saveNow);

// Fluent custom dropdown（shield / guard 共用）: the native select stays hidden as
// the value store — all existing read/write logic keeps working
function setupDropdown(ddId, btnId, menuId, selectId, labelId, labels, onChange) {
  const dd = $(ddId);
  const set = (v, silent) => {
    $(selectId).value = v;
    $(labelId).textContent = labels[v] || v;
    $(menuId).querySelectorAll('.dd-opt').forEach((o) =>
      o.classList.toggle('sel', o.dataset.value === v));
    if (!silent && onChange) onChange(v);
  };
  $(btnId).addEventListener('click', (e) => {
    e.stopPropagation();
    dd.classList.toggle('open');
  });
  $(menuId).querySelectorAll('.dd-opt').forEach((o) =>
    o.addEventListener('click', () => {
      set(o.dataset.value);
      dd.classList.remove('open');
    }));
  return set;
}

const SHIELD_LABELS = { block: '边框 + 拦截点击（默认）', border: '仅高亮边框', off: '关闭' };
const setShield = setupDropdown('shieldDD', 'shieldBtn', 'shieldMenu', 'shieldMode', 'shieldLabel', SHIELD_LABELS, () => saveNow());

const GUARD_LABELS = { session: '仅会话标签页（默认）', always: '所有标签页', off: '关闭' };
const setGuard = setupDropdown('guardDD', 'guardBtn', 'guardMenu', 'guardMode', 'guardLabel', GUARD_LABELS, (v) => sendSecurityPatch({ guard_mode: v }));

document.addEventListener('click', (e) => {
  for (const id of ['shieldDD', 'guardDD']) {
    if (!$(id).contains(e.target)) $(id).classList.remove('open');
  }
});
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    for (const id of ['shieldDD', 'guardDD']) $(id).classList.remove('open');
  }
});
$('save').addEventListener('click', saveNow);

// ---------- 高级 · 安全开关（契约 §3 新 patch 键） ----------
// 未就绪（新键 undefined）一律按契约默认值渲染，不闪错：
// guard_mode "session" / cursor_enabled true / cdp_raw false / policy_strict false / cdp_origins {}
async function sendSecurityPatch(patch) {
  try {
    const res = await chrome.runtime.sendMessage({ type: 'CUA_POPUP_CONFIG', ...patch });
    if (res && res.ok === false) alert(res.error || '保存失败');
  } catch (e) {
    // SW 重启瞬间的发送失败忽略；2s 轮询会把显示纠正回存储真值
  }
  refresh();
}

const LIFETIME_LABELS = { once: '一次', session: '本会话', forever: '永久' };

function renderCdpOrigins(origins) {
  const listEl = $('cdpOriginList');
  listEl.textContent = '';
  const entries = origins && typeof origins === 'object' ? Object.entries(origins) : [];
  if (!entries.length) {
    const empty = document.createElement('div');
    empty.className = 'hint';
    empty.style.margin = '4px 0 0';
    empty.textContent = '暂无已授权站点';
    listEl.appendChild(empty);
    return;
  }
  for (const [origin, rec] of entries) {
    const row = document.createElement('div');
    row.className = 'origin-item';
    const code = document.createElement('code');
    code.className = 'origin';
    code.textContent = origin;
    row.appendChild(code);
    const life = rec && LIFETIME_LABELS[rec.lifetime] ? rec.lifetime : 'once';
    const badge = document.createElement('span');
    badge.className = 'lifetime lt-' + life;
    badge.textContent = LIFETIME_LABELS[life];
    row.appendChild(badge);
    const btn = document.createElement('button');
    btn.textContent = '撤销';
    btn.addEventListener('click', () => sendSecurityPatch({ cdp_revoke_origin: origin }));
    row.appendChild(btn);
    listEl.appendChild(row);
  }
}

function renderSecurity(st) {
  setGuard(st.guard_mode === 'always' || st.guard_mode === 'off' ? st.guard_mode : 'session', true);
  $('cursorEnabled').checked = st.cursor_enabled !== false;
  $('cdpRaw').checked = st.cdp_raw === true;
  $('policyStrict').checked = st.policy_strict === true;
  renderCdpOrigins(st.cdp_origins);
}

$('cursorEnabled').addEventListener('change', () =>
  sendSecurityPatch({ cursor_enabled: $('cursorEnabled').checked }));
$('cdpRaw').addEventListener('change', () =>
  sendSecurityPatch({ cdp_raw: $('cdpRaw').checked }));
$('policyStrict').addEventListener('change', () =>
  sendSecurityPatch({ policy_strict: $('policyStrict').checked }));

// ---------- 安全凭据（secret.request，Codex browserAuth 式） ----------
// 值只存在于 DOM 输入框（纯 DOM 态）：不写 storage、不进日志，popup 关闭即消失。
// secItems 跨 2s 轮询持有每个请求的 DOM 节点 —— 只增删/更新元数据，绝不重写输入框，
// 输入值与焦点因此天然保留；仅提交瞬间从 DOM 读出 values 送消息通道。
const secItems = new Map(); // request id -> DOM 节点
const SEC_TYPE_LABELS = { username: '用户名', password: '密码', otp: '验证码', text: '文本' };

function renderSecRequests(raw) {
  const reqs = Array.isArray(raw) ? raw : [];
  const now = Date.now();
  const seen = new Set();
  const listEl = $('secReqList');
  for (const r of reqs) {
    if (!r || typeof r.id !== 'string' || !r.id) continue;
    seen.add(r.id);
    let item = secItems.get(r.id);
    if (!item) {
      item = buildSecItem(r);
      secItems.set(r.id, item);
      listEl.appendChild(item);
    }
    // 过期项灰显不可填（轮询间隙刚过期也要立刻锁住）；取消按钮保留以便手动移除
    const expired = typeof r.expires_at === 'number' && r.expires_at < now;
    item.classList.toggle('expired', expired);
    item.querySelectorAll('input').forEach((el) => (el.disabled = expired));
    const submitBtn = item.querySelector('.sec-submit');
    if (submitBtn) submitBtn.disabled = expired;
  }
  for (const [id, el] of [...secItems]) {
    if (!seen.has(id)) {
      el.remove();
      secItems.delete(id);
    }
  }
  $('secReqEmpty').style.display = seen.size ? 'none' : '';
  $('secReqCount').textContent = '待处理 ' + seen.size;
  $('secReqCount').style.display = seen.size ? '' : 'none';
}

function buildSecItem(r) {
  const item = document.createElement('div');
  item.className = 'sec-item';

  const head = document.createElement('div');
  head.className = 'sec-head';
  const origin = document.createElement('span');
  origin.className = 'sec-origin';
  origin.textContent = r.origin || '未知站点';
  head.appendChild(origin);
  item.appendChild(head);

  if (r.title) {
    const title = document.createElement('div');
    title.className = 'sec-title';
    title.textContent = r.title;
    item.appendChild(title);
  }

  const fields = Array.isArray(r.fields) ? r.fields : [];
  for (const f of fields) {
    if (!f || typeof f.id !== 'string' || !f.id) continue;
    const type = SEC_TYPE_LABELS[f.type] ? f.type : 'text';
    const wrap = document.createElement('div');
    wrap.className = 'sec-field';

    const fieldHead = document.createElement('div');
    fieldHead.className = 'sec-field-head';
    const label = document.createElement('span');
    label.className = 'sec-label';
    label.textContent = f.label || f.id;
    fieldHead.appendChild(label);
    const badge = document.createElement('span');
    badge.className = 'badge bt-' + type;
    badge.textContent = SEC_TYPE_LABELS[type];
    fieldHead.appendChild(badge);
    if (type === 'otp' || type === 'text') {
      const secureMark = document.createElement('span');
      secureMark.className = 'secure-mark';
      secureMark.textContent = '仅限安全表单';
      fieldHead.appendChild(secureMark);
    }
    wrap.appendChild(fieldHead);

    const input = document.createElement('input');
    input.type = type === 'password' ? 'password' : 'text';
    input.dataset.fid = f.id;
    input.autocomplete = 'off';
    input.spellcheck = false;
    input.addEventListener('input', () => input.classList.remove('invalid'));
    wrap.appendChild(input);
    item.appendChild(wrap);
  }

  const errEl = document.createElement('div');
  errEl.className = 'sec-err';
  errEl.style.display = 'none';
  item.appendChild(errEl);

  const actions = document.createElement('div');
  actions.className = 'sec-actions';
  const submitBtn = document.createElement('button');
  submitBtn.className = 'primary sec-submit';
  submitBtn.textContent = '提交';
  if (!item.querySelector('input[data-fid]')) submitBtn.disabled = true;
  submitBtn.addEventListener('click', () => submitSecReq(r.id, item));
  const cancelBtn = document.createElement('button');
  cancelBtn.textContent = '取消';
  cancelBtn.addEventListener('click', () => cancelSecReq(r.id));
  actions.appendChild(submitBtn);
  actions.appendChild(cancelBtn);
  item.appendChild(actions);
  return item;
}

function showSecErr(errEl, msg) {
  errEl.textContent = msg;
  errEl.style.display = '';
  clearTimeout(errEl._t);
  errEl._t = setTimeout(() => {
    errEl.style.display = 'none';
  }, 6000);
}

async function submitSecReq(id, item) {
  const errEl = item.querySelector('.sec-err');
  const values = {};
  let missing = false;
  for (const inp of item.querySelectorAll('input[data-fid]')) {
    if (!inp.value) {
      inp.classList.add('invalid');
      missing = true;
    } else {
      inp.classList.remove('invalid');
      values[inp.dataset.fid] = inp.value; // 仅此刻从 DOM 读出，随消息进内存通道
    }
  }
  if (missing) {
    showSecErr(errEl, '请填写所有字段后再提交');
    return;
  }
  let res = null;
  try {
    res = await chrome.runtime.sendMessage({ type: 'CUA_POPUP_SECREQ_SUBMIT', id, values });
  } catch (e) {
    res = null;
  }
  if (res && res.ok !== false) {
    // 提交成功：立即清空本地表单态（DOM 即态）并刷新
    const el = secItems.get(id);
    if (el) {
      el.remove();
      secItems.delete(id);
    }
    refresh();
  } else {
    // {ok:false} 或无响应：行内错误，不清表单
    showSecErr(errEl, res && res.error ? res.error : '提交失败，请重试（表单已保留）');
  }
}

async function cancelSecReq(id) {
  try {
    await chrome.runtime.sendMessage({ type: 'CUA_POPUP_SECREQ_CANCEL', id });
  } catch (e) {
    // 忽略：本地仍移除，2s 轮询会同步真实状态
  }
  const el = secItems.get(id);
  if (el) {
    el.remove();
    secItems.delete(id);
  }
  refresh();
}

refresh();
setInterval(refresh, 2000);
