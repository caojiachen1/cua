// secret.* — the ONLY place a credential value may be touched, and it never
// touches a plaintext the model supplied: the value is fetched from the local
// daemon over the already-authenticated extension socket at the moment of
// typing (_scratch/AGENT-CONTRACT.md §5). Values never reach tool results,
// audit entries, daemon logs or error messages (§4.6).

import {
  targetTab,
  resolveRef,
  attrsOf,
  guardSensitiveField,
  PASSWORD_FIELD,
  cdp,
  evalJs,
  key,
  withPass,
  shield,
  audit,
  redact,
  err,
} from './actions.js';
import { requestDaemon } from './ws-client.js';
import { isEnabled as humanOn, humanType } from './human.js';
import { secReq } from './secreq.js';

const SECRET_NAME_RE = /^[A-Za-z0-9_.-]{1,64}$/;

// mirror of originMatches() in daemon/cli/secret.mjs — lib/* cannot import the
// Node module (contract §1); test-secrets asserts both copies judge alike.
function originMatches(host, boundHost) {
  const h = String(host || '').toLowerCase();
  const b = String(boundHost || '').toLowerCase();
  return !!h && !!b && (h === b || h.endsWith('.' + b));
}

function tabHost(url) {
  try {
    return new URL(String(url || '')).hostname.toLowerCase();
  } catch {
    return '';
  }
}

async function ask(method, params) {
  let r;
  try {
    r = await requestDaemon(method, params);
  } catch (e) {
    if (e && e.code) throw e; // SECRET_UNKNOWN / SECRET_FETCH_TIMEOUT keep their guidance
    throw err('DAEMON_NOT_CONNECTED', `could not ask the local daemon (${method}): ${String(e?.message || e)} — check the UBB popup status / \`ubb daemon status\``);
  }
  if (!r || typeof r !== 'object') throw err('DAEMON_ERROR', `the daemon sent no answer for ${method} — restart it with \`ubb daemon stop\` then retry`);
  return r;
}

/** @param {string} name → the plaintext, held only in this local frame. */
async function fetchSecret(name) {
  const r = await ask('secret.resolve', { name });
  const value = r.value;
  if (typeof value !== 'string' || !value.length) {
    throw err('SECRET_UNKNOWN', `the daemon returned no value for secret "${name}" — list names with \`ubb secret list\``);
  }
  return value;
}

function checkedName(name) {
  const n = typeof name === 'string' ? name : '';
  if (!SECRET_NAME_RE.test(n)) {
    throw err('BAD_PARAMS', `secret name "${String(name).slice(0, 64)}" is invalid (letters, digits, _ . - up to 64 chars; e.g. github_pw)`);
  }
  return n;
}

export function register(dispatcher) {
  const R = (name, meta, fn) => dispatcher.register(name, meta, fn);

  R('secret.list', { category: 'secrets', description: 'Names of the secrets in the local daemon vault, plus vault path/mtime. Values are never returned — reference a name from secret.fill or a {{secret:NAME}} placeholder.' }, async () => {
    const r = await ask('secret.list', {});
    return redact({
      names: Array.isArray(r.names) ? r.names : [],
      vault_path: r.vault_path,
      vault_updated_at: r.vault_updated_at,
      note: 'values are never listed; use the name in secret.fill',
    });
  });

  R('secret.set', { category: 'secrets', sensitive: true, risk: 'transmit', description: 'Store/overwrite a secret by name in the local vault. confirm:true required. WARNING: the value you pass here becomes part of this conversation transcript and crosses into the daemon — the safer route is to ask the user to run `ubb secret add <name>` in their terminal, which reads the value from stdin and never enters the conversation. Result and audit carry the name and byte count only.' }, async (p = {}) => {
    if (p.confirm !== true) {
      throw err('NEEDS_CONFIRM', 'secret.set writes a credential to disk and the value you pass lands in this transcript — prefer asking the user to run `ubb secret add <name>` themselves; retry with confirm:true only if they agree to hand it over here');
    }
    const name = checkedName(p.name);
    const value = typeof p.value === 'string' ? p.value : '';
    if (!value) throw err('BAD_PARAMS', 'secret.set needs {name, value} — and remember the value lands in the transcript; `ubb secret add` is better');
    const r = await ask('secret.set', { name, value });
    audit.log({ action: 'secret.set', name, bytes: r.bytes ?? value.length });
    return redact({ stored: r.name ?? name, bytes: r.bytes ?? value.length, note: 'value not returned; verify by using it in secret.fill' });
  });

  R('secret.delete', { category: 'secrets', description: 'Remove a secret by name from the local vault. Name only, never a value.' }, async (p = {}) => {
    const name = checkedName(p.name);
    const r = await ask('secret.delete', { name });
    audit.log({ action: 'secret.delete', name, existed: !!r.existed });
    return redact({ deleted: r.name ?? name, existed: !!r.existed });
  });

  R(
    'secret.fill',
    {
      category: 'secrets',
      description:
        'Fill a PASSWORD field from the vault by NAME: {"ref":"@e7","secret":"github_pw"}. The value is fetched from the local daemon at typing time, so it never appears in your arguments, results or logs. ' +
        'ORIGIN BINDING: a vault entry may be bound to hosts (ubb secret bind <name> <host>); origin mode "strict" (ubb secret mode strict) refuses any fill whose page host does not match the binding — unbound secrets are refused entirely under strict. The default mode "off" keeps unbound secrets typeable anywhere, but every fill is audited with {name, host, field_role:"password"} so harvesting attempts show up in `ubb audit tail`. ' +
        'The {{secret:NAME}} placeholder substitutes only in fill / fill_form values and replay vars — NOT in type (it has no target field to classify, so the placeholder stays literal there). ' +
        'Optional "enforce":"off"|"strict" overrides the configured mode for one call. ' +
        'OTP / one-time-code / CVV / card-number / private-key fields are refused permanently, with no bypass parameter. Fields whose NAME contains "secret" (e.g. client_secret) are also refused here — deliberate over-conservatism, not a bug; type those yourself.',
      timeoutMs: 25000,
    },
    async (p = {}) => {
      if (p.value != null || p.text != null) {
        throw err('SENSITIVE_FIELD', 'secret.fill takes the secret NAME only — a value here would sit in the transcript, the audit ring and the daemon log; drop it and pass {"ref":"@eN","secret":"<name>"}');
      }
      if (p.enforce != null && p.enforce !== 'off' && p.enforce !== 'strict') {
        throw err('BAD_PARAMS', `enforce must be "off" or "strict" (got ${JSON.stringify(String(p.enforce).slice(0, 24))})`);
      }
      const name = checkedName(p.secret);
      if (typeof p.ref !== 'string' || !p.ref.startsWith('@')) {
        throw err('BAD_PARAMS', 'secret.fill needs a snapshot ref like "@e7" — run snapshot first, then {"ref":"@e7","secret":"<name>"} (tab_id is honoured, pass it explicitly)');
      }
      const tabId = targetTab(p);
      const { nodeId, box, meta } = await resolveRef(tabId, p.ref);
      const attrs = await attrsOf(tabId, nodeId);
      const ac = String(attrs.autocomplete || '').toLowerCase();
      // §4.5 first, and it stays first: OTP/CVV/card/私钥/name-contains-secret are
      // refused even though this call is allowed to fill password fields.
      await guardSensitiveField(tabId, nodeId, meta, { allowPassword: true });
      const isPassword =
        attrs.type === 'password' ||
        ac === 'current-password' ||
        ac === 'new-password' ||
        PASSWORD_FIELD.test([attrs.type, attrs.autocomplete, attrs.name, attrs.id, attrs.placeholder, attrs['aria-label'], meta?.name].join(' '));
      if (!isPassword) {
        throw err('NOT_PASSWORD_FIELD', `${p.ref} is not a password field — secret.fill is for password inputs; for a normal field use fill with the "{{secret:${name}}}" placeholder`);
      }
      // __ubb_secret_binding is stamped by the daemon (agent copies of it are
      // stripped at the forward boundary), so this is the authoritative mode.
      const binding = p.__ubb_secret_binding && typeof p.__ubb_secret_binding === 'object' ? p.__ubb_secret_binding : {};
      const mode = binding.mode === 'strict' ? 'strict' : 'off';
      const origins = Array.isArray(binding.origins) ? binding.origins : [];
      const host = tabHost((await chrome.tabs.get(tabId)).url);
      if (mode === 'strict' && !origins.some((o) => originMatches(host, o))) {
        audit.log({
          action: 'secret.fill', tab_id: tabId, vault_entry: name, origin: host || null,
          field_role: 'password', mode, decision: 'denied', reason: origins.length ? 'origin_mismatch' : 'unbound',
        });
        throw err(
          'SECRET_ORIGIN_DENIED',
          `secret "${name}" ${origins.length ? `is bound to ${origins.join(', ')} but this tab is host ${JSON.stringify(host || '(unknown)')}` : 'has no origin binding and mode is strict'} — open the bound site, or bind it: \`ubb secret bind ${name} <host>\` (a human, not the agent, runs that). OTP/CVV remain refused everywhere.`,
        );
      }
      // visible notice through the existing shield channel before anything is typed
      await shield.help(tabId, {
        reqId: `secret-fill-${Date.now()}`,
        title: `ubb is auto-typing the saved password "${name}"${host ? ` into ${host}` : ''} — hit Interrupt if you did not ask for this login`,
        timeoutMs: 4000,
      });
      // The value is fetched BEFORE the door opens on purpose: a reverse RPC to
      // the daemon can take up to 10s and an open pass is the window in which the
      // USER's own input reaches the page — it must stay as short as the typing
      // itself. The plaintext only ever names the send the gate below refuses.
      const value = await fetchSecret(name); // held in this local frame only
      await withPass(tabId, async (gate) => {
        // same deterministic REPLACE as fill(): mark the node from the DOM
        // domain, select its current text from page JS, then insert over it
        await cdp.send(tabId, 'DOM.setAttributeValue', { nodeId, name: 'data-ubb-tmp', value: '1' });
        await evalJs(tabId, `(function(){var el=document.querySelector('[data-ubb-tmp]');el.removeAttribute('data-ubb-tmp');el.focus();try{el.setSelectionRange(0,(el.value||'').length)}catch(e){}})()`);
        // raw Input.insertText bypasses actions.mouse()/key(), so it carries its
        // own proof of THIS point — same strength as fill {ref} (lib/tools.js).
        // A refused check throws before the credential has a single character on
        // the wire, and this path never audits or returns the value.
        await gate.check();
        if (await humanOn()) await humanType(tabId, value);
        else await cdp.send(tabId, 'Input.insertText', { text: value });
        if (p.press_enter) {
          await key(tabId, 'keyDown', 'Enter');
          await key(tabId, 'keyUp', 'Enter');
        }
      }, { frameId: meta?.frameId ?? 0, at: [box.cx, box.cy], keys: true });
      // key names here deliberately avoid redact()'s sensitive substrings —
      // audit entries pass through redact() on the way out, and a key like
      // "secret" would blank the very field the stats need (integration §活一.3)
      // No `frame` key on purpose: a frame-scoped ref never reaches this line
      // (resolveRef returns nodeId null for it and the attrs/password check
      // refuses), so `origin` here is always the TAB host — the frame:true
      // marker belongs exclusively to the tools.js placeholder branch, the
      // one path that really does judge a frame's own origin.
      audit.log({
        action: 'secret.fill', tab_id: tabId, vault_entry: name, target: meta?.name, origin: host || null,
        field_role: 'password', mode, decision: 'filled', bytes: value.length,
      });
      const out = redact({ filled: p.ref, target: meta?.name, secret: name });
      // redact() blanks any key named "secret"; this one holds the name, which
      // SECRET_NAME_RE already proved cannot be a free-form value.
      out.secret = name;
      return out;
    },
  );

  R(
    'secret.request',
    {
      category: 'secrets',
      timeoutMs: 180000,
      description:
        '请用户在 UBB 弹窗里亲手填表（Codex 吸收 §2.3）：fields:[{id?, ref:"@eN", label, type:"username"|"password"|"otp"|"text"}]（id 可省，按序号补 f1..fN；≤8 个；label ≤40 字、只做展示文本）。' +
        '弹窗挂起等用户提交（timeout_ms 默认 120000，上限 180000），filled 后逐字段把值打进页面。' +
        '这是唯一允许 OTP/验证码 值落进页面的通道 —— 值由用户亲手输入、只在内存流转：不进对话、不进审计、不进存储、不进 daemon；' +
        'cvv/cvc/卡号/私钥/名字含 secret 的字段在任何通道仍然硬拒。tab 必须是会话 tab；同 tab 已有等待中的请求报 SECRET_REQ_BUSY。' +
        '返回 {state:"filled"|"cancelled", filled:[{id,label,type}], origin}（超时以 SECRET_REQ_TIMEOUT 报错）。',
    },
    async (p = {}) => {
      // fields 规范化 + 反注入轻启发式（label 只做展示文本）
      const raw = Array.isArray(p.fields) ? p.fields : [];
      if (!raw.length) throw err('BAD_PARAMS', 'fields required: [{id?, ref:"@eN", label, type}]');
      const fields = raw.map((f, i) => {
        if (!f || typeof f !== 'object') throw err('BAD_PARAMS', `fields[${i}] 必须是对象`);
        if (typeof f.ref !== 'string' || !f.ref.startsWith('@')) {
          throw err('BAD_PARAMS', `fields[${i}].ref 必须是 snapshot ref（"@eN"）— 先 snapshot 再声明字段`);
        }
        if (/https?:\/\/|\n|```|<\//i.test(String(f.label || ''))) {
          throw err('BAD_PARAMS', `fields[${i}].label 不得包含 URL/换行/代码标记（label 只是展示文本，不是指令位）`);
        }
        return {
          id: typeof f.id === 'string' && f.id ? f.id : `f${i + 1}`,
          ref: f.ref,
          label: f.label,
          type: f.type,
        };
      });
      secReq.validateFields(fields); // ≤8、type 枚举封闭（声明不出 card/cvv）、label ≤40、id 唯一
      const tabId = targetTab(p); // 会话 tab 才能挂请求（TAB_NOT_OWNED / NO_SESSION 在这里抛）
      const tab = await chrome.tabs.get(tabId).catch(() => null);
      let origin = '';
      try {
        origin = new URL(tab?.url || '').origin;
      } catch { /* 非常规页面 → origin 空串，弹窗仍显示 */ }
      // 元数据（无值）进弹窗轮询与审计；值只经 popup→background→内存
      const meta = fields.map((f) => ({ id: f.id, label: f.label, type: f.type }));
      const ms = Math.min(Math.max(Number(p.timeout_ms) || 120000, 5000), 180000);
      const req = secReq.create({ tab_id: tabId, origin, title: p.title, fields: meta, timeout_ms: ms });
      let outcome;
      try {
        outcome = await req.promise;
      } catch (e) {
        audit.log({ action: 'secret.request', tab_id: tabId, origin: origin || null, fields: meta, decision: 'timeout' });
        throw e; // SECRET_REQ_TIMEOUT 原样上抛（secreq.js 的文案已含指引）
      }
      if (outcome.state !== 'filled') {
        audit.log({ action: 'secret.request', tab_id: tabId, origin: origin || null, fields: meta, decision: 'cancelled' });
        return redact({ state: 'cancelled', filled: [], origin });
      }
      // 逐字段填充：定位/守卫/可见性/落键路径与 secret.fill 完全同款；用户没填
      // 的字段跳过（不造空值、不重复请求）
      const values = outcome.values || {};
      const filled = [];
      try {
        for (const f of fields) {
          const value = values[f.id];
          if (typeof value !== 'string' || !value.length) continue;
          const { nodeId, box, meta: nmeta } = await resolveRef(tabId, f.ref);
          if (nodeId == null) {
            throw err(
              'FRAME_UNSUPPORTED',
              `${f.ref} 在子 frame 里 — secret.request 的填充走主文档节点路径；对 frame 内字段先在主文档重新 snapshot，仍不可达就用 request_help 交回用户`,
            );
          }
          // §4.5 + §0：cvv/card/私钥/secret 名字永远拒绝；otp/one-time/验证码
          // 名字仅本通道放行 —— secureForm:true 是唯一入口，别处传了也没用
          await guardSensitiveField(tabId, nodeId, nmeta, { allowPassword: true, secureForm: true });
          // 可见性提示 + 落键，与 secret.fill 同一形状（前置聚焦替换旧文本）
          await shield.help(tabId, {
            reqId: `secret-req-${Date.now()}`,
            title: `ubb 正在把你刚在弹窗填写的字段写入页面${origin ? `（${origin}）` : ''} — 不是你发起的就点 Interrupt`,
            timeoutMs: 4000,
          });
          await withPass(tabId, async (gate) => {
            await cdp.send(tabId, 'DOM.setAttributeValue', { nodeId, name: 'data-ubb-tmp', value: '1' });
            await evalJs(tabId, `(function(){var el=document.querySelector('[data-ubb-tmp]');el.removeAttribute('data-ubb-tmp');el.focus();try{el.setSelectionRange(0,(el.value||'').length)}catch(e){}})()`);
            await gate.check();
            if (await humanOn()) await humanType(tabId, value);
            else await cdp.send(tabId, 'Input.insertText', { text: value });
          }, { frameId: nmeta?.frameId ?? 0, at: [box.cx, box.cy], keys: true });
          filled.push({ id: f.id, label: f.label, type: f.type });
        }
      } catch (e) {
        audit.log({ action: 'secret.request', tab_id: tabId, origin: origin || null, fields: meta, decision: 'failed' });
        throw e;
      }
      audit.log({
        action: 'secret.request', tab_id: tabId, origin: origin || null, fields: meta,
        decision: 'filled', filled: filled.map((f) => f.id),
      });
      return redact({ state: 'filled', filled, origin });
    },
  );
}
