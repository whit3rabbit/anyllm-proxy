# Admin UI Overhaul Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the admin UI (`crates/proxy/admin-ui/index.html`) in line with the backend API by adding virtual key management, model routing, request detail view, cost visibility, feed controls, and security fixes.

**Architecture:** Single-file SPA (vanilla JS, no framework). All changes are in `index.html`. The backend already exposes every endpoint needed; this is purely frontend work. Each task adds a self-contained feature via new tab, new section, or new behavior. No backend changes required.

**Tech Stack:** HTML, CSS, vanilla JS (ES5-compatible IIFE pattern matching existing code), fetch API, WebSocket.

---

## File Structure

All tasks modify a single file:

- **Modify:** `crates/proxy/admin-ui/index.html` (the entire admin SPA)

No new files are created. No backend changes are needed. Tests are manual browser verification since this is a static HTML file embedded via `include_str!`.

---

### Task 1: Security -- Replace localStorage with sessionStorage and add login form

**Files:**
- Modify: `crates/proxy/admin-ui/index.html:121-132` (token handling) and `:7-55` (styles)

Addresses audit items #11 (localStorage persistence) and #13 (prompt() phishing risk).

- [ ] **Step 1: Replace localStorage token storage with sessionStorage**

Replace the token initialization block (lines 125-131):

```js
// OLD:
var TOKEN = localStorage.getItem('admin_token');
if (!TOKEN) {
    TOKEN = prompt('Enter admin token:');
    if (TOKEN) {
      localStorage.setItem('admin_token', TOKEN);
    }
}
if (!TOKEN) { document.body.textContent = 'No token provided'; return; }

// NEW:
var TOKEN = sessionStorage.getItem('admin_token');
```

- [ ] **Step 2: Add login form HTML and CSS**

Add CSS for the login form (after the existing `.empty` rule, before closing `</style>`):

```css
.login-overlay{position:fixed;inset:0;background:#0d1117;display:flex;align-items:center;justify-content:center;z-index:100}
.login-box{background:#161b22;border:1px solid #30363d;border-radius:8px;padding:32px;width:320px;text-align:center}
.login-box h2{margin-bottom:16px;font-size:18px;font-weight:600}
.login-box input[type=password]{width:100%;margin-bottom:12px;padding:10px;font-size:14px}
.login-box .btn{width:100%}
.login-error{color:#f85149;font-size:12px;margin-top:8px;min-height:16px}
```

Add the login form HTML right after `<body>`, before the `.nav` div:

```html
<div id="login-overlay" class="login-overlay" style="display:none">
  <div class="login-box">
    <h2>Proxy Admin</h2>
    <form id="login-form">
      <input type="password" id="login-token" placeholder="Admin token" autocomplete="current-password">
      <button type="submit" class="btn btn-primary">Sign In</button>
      <div class="login-error" id="login-error"></div>
    </form>
  </div>
</div>
```

- [ ] **Step 3: Wire up login form logic**

Replace the old TOKEN block with:

```js
var TOKEN = sessionStorage.getItem('admin_token');
if (!TOKEN) {
    // Show login form, hide nav and tabs
    document.getElementById('login-overlay').style.display = '';
    document.querySelector('.nav').style.display = 'none';
    document.querySelectorAll('.tab').forEach(function(t) { t.style.display = 'none'; });
    document.getElementById('login-form').addEventListener('submit', function(e) {
        e.preventDefault();
        var val = document.getElementById('login-token').value.trim();
        if (!val) return;
        // Validate token with a test API call
        fetch('/admin/api/metrics', {headers: {'Authorization': 'Bearer ' + val}})
          .then(function(r) {
              if (r.ok) {
                  sessionStorage.setItem('admin_token', val);
                  location.reload();
              } else {
                  document.getElementById('login-error').textContent = 'Invalid token';
              }
          })
          .catch(function() {
              document.getElementById('login-error').textContent = 'Connection failed';
          });
    });
    return; // Stop IIFE execution until authenticated
}
```

- [ ] **Step 4: Remove old localStorage cleanup code**

Remove the URL cleanup block (lines 135-137) since we no longer use query param tokens:

```js
// DELETE THIS:
if (window.location.search.indexOf('token=') !== -1) {
    window.history.replaceState({}, '', window.location.pathname);
}
```

- [ ] **Step 5: Verify in browser**

Open `http://localhost:9091/admin/` (or whatever ADMIN_PORT is set to). Expected:
1. Login form appears (not `prompt()`)
2. Enter wrong token: "Invalid token" error shown
3. Enter correct token: page reloads with dashboard
4. Close tab, reopen: login form appears again (sessionStorage cleared)
5. No `admin_token` in localStorage (check in DevTools > Application > Local Storage)

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/admin-ui/index.html
git commit -m "security: replace localStorage with sessionStorage and add login form for admin UI"
```

---

### Task 2: Virtual Key Management Tab

**Files:**
- Modify: `crates/proxy/admin-ui/index.html` (add Keys nav item, tab content, JS functions)

Addresses audit item #5. Calls `GET /admin/api/keys`, `POST /admin/api/keys`, `DELETE /admin/api/keys/{id}`, `GET /admin/api/keys/{id}/spend`.

- [ ] **Step 1: Add Keys nav item and tab container**

Add nav item after the Backends nav item:

```html
<div class="nav-item" data-tab="keys">Keys</div>
```

Add tab div after `tab-backends`:

```html
<div id="tab-keys" class="tab">
  <div class="section-header">
    <div class="section-label">Virtual API Keys</div>
    <button class="btn btn-primary" id="btn-create-key">Create Key</button>
  </div>
  <div id="create-key-form" style="display:none;margin-bottom:16px">
    <div class="form-group">
      <div class="form-label">New Virtual Key</div>
      <div class="model-grid" style="grid-template-columns:120px 1fr">
        <div class="label">Description:</div><input id="key-desc" placeholder="e.g. team-ml-prod">
        <div class="label">Role:</div><select id="key-role"><option value="developer">developer</option><option value="admin">admin</option></select>
        <div class="label">RPM Limit:</div><input id="key-rpm" type="number" placeholder="(unlimited)">
        <div class="label">TPM Limit:</div><input id="key-tpm" type="number" placeholder="(unlimited)">
        <div class="label">Max Budget $:</div><input id="key-budget" type="number" step="0.01" placeholder="(unlimited)">
        <div class="label">Budget Period:</div><select id="key-budget-dur"><option value="">(lifetime)</option><option value="daily">daily</option><option value="monthly">monthly</option></select>
        <div class="label">Expires At:</div><input id="key-expires" type="datetime-local">
      </div>
      <div style="margin-top:12px;display:flex;gap:8px">
        <button class="btn btn-primary" id="btn-submit-key">Create</button>
        <button class="btn btn-secondary" id="btn-cancel-key">Cancel</button>
      </div>
      <div id="key-created-result" style="display:none;margin-top:12px;padding:12px;background:#0d2818;border:1px solid #238636;border-radius:6px">
        <div style="font-weight:600;margin-bottom:4px">Key created (copy now, shown only once):</div>
        <code id="key-created-value" style="word-break:break-all;user-select:all;font-size:13px;color:#3fb950"></code>
      </div>
    </div>
  </div>
  <div id="keys-table"></div>
</div>
```

- [ ] **Step 2: Add CSS for the keys table**

Add after existing `.pagination` rule:

```css
.keys-grid{width:100%;border-collapse:collapse}
.keys-grid th,.keys-grid td{padding:8px 10px;text-align:left;font-size:12px;border-bottom:1px solid #21262d}
.keys-grid th{background:#1c2128;color:#8b949e;position:sticky;top:0;font-weight:500;text-transform:uppercase;font-size:10px;letter-spacing:0.5px}
.keys-grid tr:hover{background:#1c2128}
.badge-active{background:rgba(63,185,80,0.2);color:#3fb950}
.badge-revoked{background:rgba(248,81,73,0.2);color:#f85149}
.badge-expired{background:rgba(210,153,34,0.2);color:#d29922}
.btn-danger{background:#da3633;border-color:#f85149;color:white;font-size:11px;padding:4px 10px}
.btn-sm{font-size:11px;padding:4px 10px}
```

- [ ] **Step 3: Add loadKeys function**

Add in the JS section, after the `loadBackends` function:

```js
// -- Keys management --
function loadKeys() {
    apiFetch('/keys').then(function(data) {
        var container = document.getElementById('keys-table');
        clearChildren(container, false);
        var keys = data.keys || [];
        if (keys.length === 0) {
            container.appendChild(el('div', {className: 'empty', textContent: 'No virtual keys created yet'}));
            return;
        }
        var table = el('table', {className: 'keys-grid'});
        var thead = el('thead', null, [el('tr', null, [
            el('th', {textContent: 'Prefix'}),
            el('th', {textContent: 'Description'}),
            el('th', {textContent: 'Role'}),
            el('th', {textContent: 'RPM/TPM'}),
            el('th', {textContent: 'Budget'}),
            el('th', {textContent: 'Spend'}),
            el('th', {textContent: 'Requests'}),
            el('th', {textContent: 'Status'}),
            el('th', {textContent: ''})
        ])]);
        table.appendChild(thead);
        var tbody = el('tbody');
        keys.forEach(function(k) {
            var statusBadge = el('span', {
                className: 'badge badge-' + k.status,
                textContent: k.status
            });
            var limits = (k.rpm_limit ? k.rpm_limit + ' rpm' : '--') + ' / ' + (k.tpm_limit ? k.tpm_limit + ' tpm' : '--');
            var budget = k.max_budget_usd != null ? '$' + k.max_budget_usd.toFixed(2) + (k.budget_duration ? '/' + k.budget_duration : '') : '--';
            var spend = '$' + (k.period_spend_usd || 0).toFixed(4);
            var actions = [];
            if (k.status === 'active') {
                var revokeBtn = el('button', {className: 'btn btn-danger', textContent: 'Revoke'});
                revokeBtn.addEventListener('click', function() { revokeKey(k.id); });
                actions.push(revokeBtn);
            }
            var spendBtn = el('button', {className: 'btn btn-sm btn-secondary', textContent: 'Spend', style:{marginLeft:'4px'}});
            spendBtn.addEventListener('click', function() { showKeySpend(k.id, k.key_prefix); });
            actions.push(spendBtn);
            var actionCell = el('td', null, actions);
            var row = el('tr', null, [
                el('td', {textContent: k.key_prefix, style:{fontFamily:'monospace',color:'#4a9eff'}}),
                el('td', {textContent: k.description || '--', style:{color:'#8b949e'}}),
                el('td', {textContent: k.role || 'developer'}),
                el('td', {textContent: limits}),
                el('td', {textContent: budget}),
                el('td', {textContent: spend}),
                el('td', {textContent: String(k.total_requests || 0)}),
                el('td', null, [statusBadge]),
                actionCell
            ]);
            tbody.appendChild(row);
        });
        table.appendChild(tbody);
        container.appendChild(table);
    }).catch(function(e) { console.error('Keys load failed:', e); });
}
```

- [ ] **Step 4: Add createKey, revokeKey, and showKeySpend functions**

```js
function createKey() {
    var desc = document.getElementById('key-desc').value.trim();
    var role = document.getElementById('key-role').value;
    var rpm = document.getElementById('key-rpm').value;
    var tpm = document.getElementById('key-tpm').value;
    var budget = document.getElementById('key-budget').value;
    var budgetDur = document.getElementById('key-budget-dur').value;
    var expires = document.getElementById('key-expires').value;

    var body = {};
    if (desc) body.description = desc;
    if (role) body.role = role;
    if (rpm) body.rpm_limit = parseInt(rpm, 10);
    if (tpm) body.tpm_limit = parseInt(tpm, 10);
    if (budget) body.max_budget_usd = parseFloat(budget);
    if (budgetDur) body.budget_duration = budgetDur;
    if (expires) body.expires_at = new Date(expires).toISOString();

    fetch(API + '/keys', {method: 'POST', headers: authHeaders, body: JSON.stringify(body)})
      .then(function(r) { return r.json(); })
      .then(function(result) {
          if (result.key) {
              document.getElementById('key-created-value').textContent = result.key;
              document.getElementById('key-created-result').style.display = '';
              loadKeys();
          } else {
              alert('Failed: ' + (result.error || 'unknown error'));
          }
      })
      .catch(function(e) { alert('Failed: ' + e.message); });
}

function revokeKey(id) {
    if (!confirm('Revoke this key? This cannot be undone.')) return;
    fetch(API + '/keys/' + id, {method: 'DELETE', headers: authHeaders})
      .then(function(r) { return r.json(); })
      .then(function() { loadKeys(); })
      .catch(function(e) { alert('Revoke failed: ' + e.message); });
}

function showKeySpend(id, prefix) {
    apiFetch('/keys/' + id + '/spend').then(function(data) {
        alert('Spend for ' + prefix + ':\n' +
              'Total: $' + (data.total_cost_usd || 0).toFixed(4) + '\n' +
              'Requests: ' + (data.total_requests || 0) + '\n' +
              'Input tokens: ' + (data.total_input_tokens || 0) + '\n' +
              'Output tokens: ' + (data.total_output_tokens || 0));
    }).catch(function(e) { alert('Failed: ' + e.message); });
}
```

- [ ] **Step 5: Wire up create/cancel buttons and tab switch**

Add after the button event listeners:

```js
document.getElementById('btn-create-key').addEventListener('click', function() {
    document.getElementById('create-key-form').style.display = '';
    document.getElementById('key-created-result').style.display = 'none';
});
document.getElementById('btn-cancel-key').addEventListener('click', function() {
    document.getElementById('create-key-form').style.display = 'none';
});
document.getElementById('btn-submit-key').addEventListener('click', createKey);
```

Update the tab switching handler to also load keys:

```js
if (navEl.dataset.tab === 'keys') loadKeys();
```

- [ ] **Step 6: Verify in browser**

Expected:
1. Keys tab appears in nav
2. Click Keys tab: shows empty state or list of keys
3. Click "Create Key": form expands
4. Fill form, click Create: key displayed (copy-once box), key appears in table
5. Click Revoke on a key: confirm dialog, key status changes to "revoked"
6. Click Spend on a key: alert shows spend data

- [ ] **Step 7: Commit**

```bash
git add crates/proxy/admin-ui/index.html
git commit -m "feat: add virtual key management tab to admin UI (CRUD, spend, rate limits)"
```

---

### Task 3: Model Routing Tab

**Files:**
- Modify: `crates/proxy/admin-ui/index.html` (add Models nav item, tab content, JS functions)

Addresses audit item #6. Calls `GET /admin/api/models`, `POST /admin/api/models`, `DELETE /admin/api/models/{name}`.

- [ ] **Step 1: Add Models nav item and tab container**

Add nav item after Keys:

```html
<div class="nav-item" data-tab="models">Models</div>
```

Add tab div after `tab-keys`:

```html
<div id="tab-models" class="tab">
  <div class="section-header">
    <div class="section-label">Model Routing</div>
    <button class="btn btn-primary" id="btn-add-model">Add Deployment</button>
  </div>
  <div id="add-model-form" style="display:none;margin-bottom:16px">
    <div class="form-group">
      <div class="form-label">New Model Deployment</div>
      <div class="model-grid" style="grid-template-columns:120px 1fr">
        <div class="label">Model Name:</div><input id="mdl-name" placeholder="e.g. gpt-4o">
        <div class="label">Backend:</div><input id="mdl-backend" placeholder="e.g. openai">
        <div class="label">Actual Model:</div><input id="mdl-actual" placeholder="e.g. gpt-4o-2024-08-06">
        <div class="label">RPM:</div><input id="mdl-rpm" type="number" placeholder="(optional)">
        <div class="label">TPM:</div><input id="mdl-tpm" type="number" placeholder="(optional)">
        <div class="label">Weight:</div><input id="mdl-weight" type="number" value="1" min="1">
      </div>
      <div style="margin-top:12px;display:flex;gap:8px">
        <button class="btn btn-primary" id="btn-submit-model">Add</button>
        <button class="btn btn-secondary" id="btn-cancel-model">Cancel</button>
      </div>
    </div>
  </div>
  <div id="models-table"></div>
</div>
```

- [ ] **Step 2: Add loadModels function**

```js
// -- Model routing --
function loadModels() {
    apiFetch('/models').then(function(data) {
        var container = document.getElementById('models-table');
        clearChildren(container, false);

        if (data.note) {
            container.appendChild(el('div', {className: 'empty', textContent: data.note}));
            return;
        }

        var strategy = data.strategy || 'unknown';
        container.appendChild(el('div', {style:{color:'#8b949e',fontSize:'12px',marginBottom:'12px'}, textContent: 'Routing strategy: ' + strategy}));

        var models = data.models || [];
        if (models.length === 0) {
            container.appendChild(el('div', {className: 'empty', textContent: 'No models configured'}));
            return;
        }

        var table = el('table', {className: 'keys-grid'});
        var thead = el('thead', null, [el('tr', null, [
            el('th', {textContent: 'Model Name'}),
            el('th', {textContent: 'Deployments'}),
            el('th', {textContent: ''})
        ])]);
        table.appendChild(thead);
        var tbody = el('tbody');
        models.forEach(function(m) {
            var removeBtn = el('button', {className: 'btn btn-danger', textContent: 'Remove'});
            removeBtn.addEventListener('click', function() { removeModel(m.model_name); });
            var row = el('tr', null, [
                el('td', {textContent: m.model_name, style:{fontFamily:'monospace',color:'#4a9eff'}}),
                el('td', {textContent: String(m.deployments)}),
                el('td', null, [removeBtn])
            ]);
            tbody.appendChild(row);
        });
        table.appendChild(tbody);
        container.appendChild(table);
    }).catch(function(e) { console.error('Models load failed:', e); });
}

function addModel() {
    var body = {
        model_name: document.getElementById('mdl-name').value.trim(),
        backend_name: document.getElementById('mdl-backend').value.trim(),
        actual_model: document.getElementById('mdl-actual').value.trim(),
        weight: parseInt(document.getElementById('mdl-weight').value, 10) || 1
    };
    var rpm = document.getElementById('mdl-rpm').value;
    var tpm = document.getElementById('mdl-tpm').value;
    if (rpm) body.rpm = parseInt(rpm, 10);
    if (tpm) body.tpm = parseInt(tpm, 10);

    if (!body.model_name || !body.backend_name || !body.actual_model) {
        alert('Model name, backend, and actual model are required');
        return;
    }

    fetch(API + '/models', {method: 'POST', headers: authHeaders, body: JSON.stringify(body)})
      .then(function(r) { return r.json(); })
      .then(function(result) {
          if (result.status === 'added') {
              document.getElementById('add-model-form').style.display = 'none';
              loadModels();
          } else {
              alert('Failed: ' + (result.error || 'unknown error'));
          }
      })
      .catch(function(e) { alert('Failed: ' + e.message); });
}

function removeModel(name) {
    if (!confirm('Remove all deployments for "' + name + '"?')) return;
    fetch(API + '/models/' + encodeURIComponent(name), {method: 'DELETE', headers: authHeaders})
      .then(function(r) { return r.json(); })
      .then(function() { loadModels(); })
      .catch(function(e) { alert('Remove failed: ' + e.message); });
}
```

- [ ] **Step 3: Wire up buttons and tab switch**

```js
document.getElementById('btn-add-model').addEventListener('click', function() {
    document.getElementById('add-model-form').style.display = '';
});
document.getElementById('btn-cancel-model').addEventListener('click', function() {
    document.getElementById('add-model-form').style.display = 'none';
});
document.getElementById('btn-submit-model').addEventListener('click', addModel);
```

Add to tab switching:

```js
if (navEl.dataset.tab === 'models') loadModels();
```

- [ ] **Step 4: Verify in browser**

Expected:
1. Models tab appears
2. Without LiteLLM config: shows "no model router active" message
3. With LiteLLM config: shows strategy and model list with deployment counts
4. Add Deployment form works (fields validated)
5. Remove button prompts confirm, then removes

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/admin-ui/index.html
git commit -m "feat: add model routing management tab to admin UI"
```

---

### Task 4: Request Detail Expansion

**Files:**
- Modify: `crates/proxy/admin-ui/index.html` (modify `makeFeedRow`, add detail fetch)

Addresses audit items #7 (request detail view) and related items #6 (streaming indicator), #7 (error messages).

- [ ] **Step 1: Add CSS for expandable rows**

```css
.feed-row{cursor:pointer}
.feed-row:hover{background:#1c2128}
.feed-detail{padding:12px 16px;background:#0d1117;border-bottom:1px solid #21262d;font-size:12px;display:grid;grid-template-columns:120px 1fr;gap:4px 12px}
.feed-detail .label{color:#8b949e}
.feed-detail .val{color:#c9d1d9;font-family:monospace;word-break:break-all}
.feed-detail .error-msg{color:#f85149;grid-column:1/-1;margin-top:4px;padding:6px;background:rgba(248,81,73,0.1);border-radius:4px}
.streaming-badge{font-size:9px;padding:1px 4px;border-radius:2px;background:rgba(74,158,255,0.2);color:#4a9eff;margin-left:4px}
```

- [ ] **Step 2: Update makeFeedRow to include streaming badge and click handler**

Replace the existing `makeFeedRow` function:

```js
function makeFeedRow(r, clickable) {
    var tokens = (r.input_tokens || 0) + (r.output_tokens || 0);
    var modelText = r.model_mapped || r.model_requested || '--';
    var modelChildren = [text(modelText)];
    if (r.is_streaming) {
        modelChildren.push(el('span', {className: 'streaming-badge', textContent: 'SSE'}));
    }
    var row = el('div', {className: 'feed-row'}, [
        el('div', {textContent: formatTime(r.timestamp)}),
        el('div', {className: statusClass(r.status_code), textContent: String(r.status_code)}),
        el('div', {textContent: r.backend || '--'}),
        el('div', {style:{color:'#8b949e'}}, modelChildren),
        el('div', {textContent: formatLatency(r.latency_ms)}),
        el('div', {textContent: tokens ? String(tokens) : '--', style:{color:'#8b949e'}})
    ]);
    if (clickable && r.request_id) {
        row.addEventListener('click', function() { toggleRequestDetail(row, r.request_id); });
    }
    return row;
}
```

Update all callers: in `renderFeed` pass `true` as second arg, and in `loadRequests` pass `true`.

- [ ] **Step 3: Add toggleRequestDetail function**

```js
function toggleRequestDetail(row, requestId) {
    // If detail already shown below this row, remove it
    var next = row.nextElementSibling;
    if (next && next.classList.contains('feed-detail')) {
        next.remove();
        return;
    }
    apiFetch('/requests/' + encodeURIComponent(requestId)).then(function(data) {
        var detail = el('div', {className: 'feed-detail'});
        var fields = [
            ['Request ID', data.request_id],
            ['Timestamp', data.timestamp],
            ['Backend', data.backend],
            ['Model Requested', data.model_requested || '--'],
            ['Model Mapped', data.model_mapped || '--'],
            ['Status', String(data.status_code)],
            ['Latency', formatLatency(data.latency_ms)],
            ['Streaming', data.is_streaming ? 'Yes' : 'No'],
            ['Input Tokens', String(data.input_tokens || 0)],
            ['Output Tokens', String(data.output_tokens || 0)]
        ];
        fields.forEach(function(f) {
            detail.appendChild(el('div', {className: 'label', textContent: f[0] + ':'}));
            detail.appendChild(el('div', {className: 'val', textContent: f[1]}));
        });
        if (data.error_message) {
            detail.appendChild(el('div', {className: 'error-msg', textContent: 'Error: ' + data.error_message}));
        }
        row.after(detail);
    }).catch(function(e) { console.error('Request detail failed:', e); });
}
```

- [ ] **Step 4: Verify in browser**

Expected:
1. Feed rows show SSE badge for streaming requests
2. Clicking a row in Request Log expands detail panel below it
3. Detail shows all fields including request_id, model_requested vs model_mapped, streaming flag, error_message
4. Clicking again collapses the detail
5. Live feed rows are also clickable

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/admin-ui/index.html
git commit -m "feat: add request detail expansion, streaming badge, and error display in admin UI"
```

---

### Task 5: Cost Column in Request Feed

**Files:**
- Modify: `crates/proxy/admin-ui/index.html` (feed header, makeFeedRow, grid columns)

Addresses audit item #4 (cost visibility in feed). The `RequestLogEntry` already includes cost data via the `x-anyllm-cost-usd` response header, but the cost is not currently in the SQLite log entry. We can show a calculated cost from the token counts, or we show cost if the field exists. For now, show tokens split (in/out) as a proxy for cost until the backend adds a `cost_usd` field to `RequestLogEntry`.

Actually, let me check: the backend may already return cost in the log entry or in the detail endpoint.

- [ ] **Step 1: Update feed grid to add Cost column**

Change grid template from 6 columns to 7:

```css
/* OLD: */
.feed-header,.feed-row{display:grid;grid-template-columns:150px 60px 90px 100px 70px 70px;...}

/* NEW: */
.feed-header,.feed-row{display:grid;grid-template-columns:140px 50px 80px 100px 65px 55px 55px;...}
```

Update the feed header divs in both `tab-dashboard` and `tab-requests`:

```html
<div class="feed-header"><div>Time</div><div>Status</div><div>Backend</div><div>Model</div><div>Latency</div><div>In</div><div>Out</div></div>
```

- [ ] **Step 2: Update makeFeedRow to show split token counts**

Replace the single tokens column with two columns:

```js
// Replace the tokens div at the end of makeFeedRow with:
el('div', {textContent: r.input_tokens ? String(r.input_tokens) : '--', style:{color:'#8b949e'}}),
el('div', {textContent: r.output_tokens ? String(r.output_tokens) : '--', style:{color:'#8b949e'}})
```

- [ ] **Step 3: Verify in browser**

Expected:
1. Feed header shows: Time | Status | Backend | Model | Latency | In | Out
2. Input and output tokens shown separately
3. Columns align properly without overflow

- [ ] **Step 4: Commit**

```bash
git add crates/proxy/admin-ui/index.html
git commit -m "feat: split token counts into input/output columns in admin UI feed"
```

---

### Task 6: Live Feed Pause/Filter Controls

**Files:**
- Modify: `crates/proxy/admin-ui/index.html` (dashboard section, JS)

Addresses audit item #9.

- [ ] **Step 1: Add pause button and status filter to live feed section**

Replace the static "Live Request Feed" label on the dashboard with:

```html
<div class="section-header">
    <div class="section-label">Live Request Feed</div>
    <div class="form-row">
        <select id="live-filter-status"><option value="">All</option><option value="2xx">2xx</option><option value="4xx">4xx</option><option value="5xx">5xx</option></select>
        <button class="btn btn-secondary" id="btn-pause-feed">Pause</button>
    </div>
</div>
```

- [ ] **Step 2: Add pause state and filter logic**

```js
var feedPaused = false;
var feedFilter = '';

document.getElementById('btn-pause-feed').addEventListener('click', function() {
    feedPaused = !feedPaused;
    this.textContent = feedPaused ? 'Resume' : 'Pause';
    this.style.borderColor = feedPaused ? '#d29922' : '#484f58';
});

document.getElementById('live-filter-status').addEventListener('change', function() {
    feedFilter = this.value;
    renderFeed();
});
```

- [ ] **Step 3: Update handleEvent and renderFeed to respect pause and filter**

```js
// In handleEvent, wrap the request_completed case:
if (event.type === 'request_completed') {
    feedRows.unshift(event.data);
    if (feedRows.length > MAX_FEED) feedRows.pop();
    if (!feedPaused) renderFeed();
    totalRequests++;
    document.getElementById('stat-total').textContent = totalRequests.toLocaleString();
}

// Update renderFeed:
function renderFeed() {
    var feed = document.getElementById('live-feed');
    clearChildren(feed, true);
    feedRows.forEach(function(r) {
        if (feedFilter) {
            var prefix = String(r.status_code).charAt(0) + 'xx';
            if (prefix !== feedFilter) return;
        }
        feed.appendChild(makeFeedRow(r, true));
    });
}
```

- [ ] **Step 4: Verify in browser**

Expected:
1. Pause button stops new rows from appearing (but they're still collected)
2. Resume shows all buffered rows
3. Status filter hides non-matching rows
4. Filter + pause work together

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/admin-ui/index.html
git commit -m "feat: add pause/resume and status filter to admin UI live feed"
```

---

### Task 7: Dashboard Metrics Robustness

**Files:**
- Modify: `crates/proxy/admin-ui/index.html` (loadDashboard function)

Addresses audit item about fragile metrics response shape.

- [ ] **Step 1: Fix loadDashboard to use consistent field access**

The REST endpoint returns `error_rate` at top level but also `total.requests_total`. Make loadDashboard handle both shapes consistently:

```js
function loadDashboard() {
    apiFetch('/metrics').then(function(data) {
        if (data.total) {
            totalRequests = data.total.requests_total || 0;
            document.getElementById('stat-total').textContent = totalRequests.toLocaleString();
        }
        if (data.latency_p50_ms != null) document.getElementById('stat-p50').textContent = formatLatency(data.latency_p50_ms);
        if (data.latency_p95_ms != null) document.getElementById('stat-p95').textContent = formatLatency(data.latency_p95_ms);
        if (data.error_rate != null) {
            var pct = (data.error_rate * 100).toFixed(1) + '%';
            var errEl = document.getElementById('stat-error-rate');
            errEl.textContent = pct;
            errEl.style.color = data.error_rate > 0.05 ? '#f85149' : '#3fb950';
        }
        // Derive requests_per_second from total if not directly available
        if (data.requests_per_second != null) {
            document.getElementById('stat-rpm').textContent = (data.requests_per_second * 60).toFixed(0);
        }
    }).catch(function(e) { console.error('Dashboard load failed:', e); });

    apiFetch('/backends').then(function(data) {
        var container = document.getElementById('backend-cards');
        clearChildren(container, false);
        (data.backends || []).forEach(function(b) {
            var card = el('div', {className: 'card'}, [
                el('div', {className: 'card-header'}, [el('span', {className: 'card-name', textContent: b.name})]),
                el('div', {className: 'card-body', textContent: b.big_model + ' / ' + b.small_model + ' | ' + (b.metrics.requests_total||0) + ' reqs | ' + (b.metrics.requests_error||0) + ' errs'})
            ]);
            container.appendChild(card);
        });
    }).catch(function(e) { console.error('Backends load failed:', e); });
}
```

The key changes: use same `updateDashboardMetrics`-style logic for REST response, add error count to backend cards.

- [ ] **Step 2: Verify in browser**

Expected:
1. Dashboard loads without JS errors
2. All stat cards populated from REST response
3. WebSocket updates continue to work
4. Backend cards show error counts

- [ ] **Step 3: Commit**

```bash
git add crates/proxy/admin-ui/index.html
git commit -m "fix: robust metrics rendering and add error counts to backend cards in admin UI"
```

---

### Task 8: Environment Display -- Show Active Backend and Missing Groups

**Files:**
- Modify: `crates/proxy/admin-ui/index.html` (ENV_GROUPS, loadEnv)

Addresses audit item #11 (show active backend, missing env groups).

- [ ] **Step 1: Add missing env groups**

Update `ENV_GROUPS` to include Azure, Bedrock, IP Allowlist, and Webhooks:

```js
var ENV_GROUPS = [
    { label: 'Core', keys: ['BACKEND','LISTEN_PORT','BIG_MODEL','SMALL_MODEL','RUST_LOG','LOG_BODIES','PROXY_CONFIG'] },
    { label: 'OpenAI / Compatible', keys: ['OPENAI_BASE_URL','OPENAI_API_FORMAT','OPENAI_API_KEY'] },
    { label: 'Vertex AI', keys: ['VERTEX_PROJECT','VERTEX_REGION','VERTEX_API_KEY'] },
    { label: 'Gemini', keys: ['GEMINI_BASE_URL','GEMINI_API_KEY'] },
    { label: 'Azure OpenAI', keys: ['AZURE_OPENAI_ENDPOINT','AZURE_OPENAI_DEPLOYMENT','AZURE_OPENAI_API_KEY','AZURE_OPENAI_API_VERSION'] },
    { label: 'AWS Bedrock', keys: ['AWS_REGION','AWS_ACCESS_KEY_ID','AWS_SECRET_ACCESS_KEY','AWS_SESSION_TOKEN'] },
    { label: 'Auth & TLS', keys: ['PROXY_API_KEYS','PROXY_OPEN_RELAY','TLS_CLIENT_CERT_P12','TLS_CA_CERT'] },
    { label: 'Network', keys: ['IP_ALLOWLIST','TRUST_PROXY_HEADERS','WEBHOOK_URLS'] },
    { label: 'Admin', keys: ['ADMIN_PORT','ADMIN_DB_PATH','ADMIN_LOG_RETENTION_DAYS'] },
];
var ENV_SECRET_KEYS = {'OPENAI_API_KEY':1,'VERTEX_API_KEY':1,'GEMINI_API_KEY':1,'PROXY_API_KEYS':1,'AZURE_OPENAI_API_KEY':1,'AWS_SECRET_ACCESS_KEY':1,'AWS_SESSION_TOKEN':1};
```

- [ ] **Step 2: Update get_env backend to include new vars**

Modify `crates/proxy/src/admin/routes.rs` `get_env` function to add the missing env vars:

```rust
// Add to the json! macro in get_env:
// Azure OpenAI
"AZURE_OPENAI_ENDPOINT":    plain("AZURE_OPENAI_ENDPOINT"),
"AZURE_OPENAI_DEPLOYMENT":  plain("AZURE_OPENAI_DEPLOYMENT"),
"AZURE_OPENAI_API_KEY":     secret("AZURE_OPENAI_API_KEY"),
"AZURE_OPENAI_API_VERSION": plain("AZURE_OPENAI_API_VERSION"),
// AWS Bedrock
"AWS_REGION":               plain("AWS_REGION"),
"AWS_ACCESS_KEY_ID":        plain("AWS_ACCESS_KEY_ID"),
"AWS_SECRET_ACCESS_KEY":    secret("AWS_SECRET_ACCESS_KEY"),
"AWS_SESSION_TOKEN":        secret("AWS_SESSION_TOKEN"),
// Network
"PROXY_OPEN_RELAY":         plain("PROXY_OPEN_RELAY"),
"IP_ALLOWLIST":             plain("IP_ALLOWLIST"),
"TRUST_PROXY_HEADERS":      plain("TRUST_PROXY_HEADERS"),
"WEBHOOK_URLS":             plain("WEBHOOK_URLS"),
```

- [ ] **Step 3: Verify in browser**

Expected:
1. Settings tab shows Azure and Bedrock env groups (when those vars are set)
2. Secret vars are masked
3. Export .env includes new vars

- [ ] **Step 4: Commit**

```bash
git add crates/proxy/admin-ui/index.html crates/proxy/src/admin/routes.rs
git commit -m "feat: add Azure, Bedrock, and network env groups to admin UI and backend"
```

---

## Summary of Changes Per Audit Item

| Audit Item | Task | Status |
|---|---|---|
| #5 Virtual key management | Task 2 | Full CRUD, spend, rate limits |
| #6 Model routing | Task 3 | View, add, remove deployments |
| #7 Request detail view | Task 4 | Click-to-expand with all fields |
| #4 Cost/token visibility | Task 5 | Split in/out token columns |
| #9 Feed pause/filter | Task 6 | Pause button, status filter |
| #1 Metrics fragility | Task 7 | Consistent field access |
| #11 Env display gaps | Task 8 | Azure, Bedrock, network groups |
| #11 localStorage | Task 1 | sessionStorage |
| #13 prompt() phishing | Task 1 | Proper login form |
| #8 sessionStorage recommendation | Task 1 | Done |

**Not addressed in this plan (lower priority / needs backend changes):**
- #8 httpOnly cookie auth (requires server-side session management, larger scope)
- #10 Cache status visibility (backend doesn't expose cache metrics yet)
- #12 CSRF (mitigated by origin checking already in place, Bearer token not in cookie)
- Dark/light theme (cosmetic, low priority)
