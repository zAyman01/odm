// State
let authToken = localStorage.getItem('odm_token') || '';
let downloads = [];
let socket = null;
let reconnectTimeout = null;
let reconnectDelay = 1000;

// Formatting utilities
function formatBytes(bytes) {
  if (bytes == null || isNaN(bytes) || bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

function formatSpeed(bps) {
  if (!bps || bps === 0) return '0 B/s';
  return formatBytes(bps) + '/s';
}

function formatEta(seconds) {
  if (!seconds || !isFinite(seconds) || seconds < 0) return '--';
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  if (m < 60) return `${m}m ${s}s`;
  const h = Math.floor(m / 60);
  const remM = m % 60;
  return `${h}h ${remM}m`;
}

// API Helpers
async function apiRequest(path, options = {}) {
  const headers = {
    'Content-Type': 'application/json',
    ...(authToken ? { 'Authorization': `Bearer ${authToken}` } : {}),
    ...options.headers,
  };
  const resp = await fetch(path, { ...options, headers });
  if (!resp.ok) {
    let errMessage = `HTTP ${resp.status}`;
    try {
      const errData = await resp.json();
      if (errData.error?.message) errMessage = errData.error.message;
    } catch (_) {}
    throw new Error(errMessage);
  }
  return resp;
}

// UI Updates
function showAuth() {
  document.getElementById('authSection').style.display = 'flex';
  document.getElementById('downloadsSection').style.display = 'none';
  document.getElementById('headerActions').style.display = 'none';
  if (socket) {
    socket.close();
    socket = null;
  }
}

function showMain() {
  document.getElementById('authSection').style.display = 'none';
  document.getElementById('downloadsSection').style.display = 'block';
  document.getElementById('headerActions').style.display = 'flex';
  loadDownloads();
  connectWebSocket();
}

async function loadVersion() {
  try {
    const resp = await fetch('/api/v1/health');
    if (resp.ok) {
      const data = await resp.json();
      if (data.version) {
        document.getElementById('appVersion').textContent = `v${data.version}`;
      }
    }
  } catch (_) {}
}

async function loadDownloads() {
  try {
    const resp = await apiRequest('/api/v1/downloads');
    const data = await resp.json();
    downloads = data.downloads || [];
    renderDownloads();
  } catch (err) {
    console.error('Failed to load downloads:', err);
  }
}

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function actionButton(label, action, id, danger) {
  const btn = el('button', danger ? 'btn btn-danger btn-sm' : 'btn btn-outline btn-sm', label);
  btn.dataset.action = action;
  btn.dataset.id = String(id);
  return btn;
}

function labelledCell(label, className) {
  const td = el('td', className);
  td.dataset.label = label;
  return td;
}

// The numbers a row shows, worked out from a download item.
function rowMetrics(item) {
  const total = item.total_bytes;
  const downloaded = item.downloaded_bytes || 0;
  const pct = item.progress_pct || (total > 0 ? (downloaded / total) * 100 : 0);
  const speed = item.speed_bps || 0;
  const etaSecs = (speed > 0 && total && total > downloaded) ? (total - downloaded) / speed : 0;
  const etaText = speed > 0 ? formatEta(etaSecs) : '';
  return { total, pct, speed, etaText };
}

function renderRow(item) {
  const { total, pct, speed, etaText } = rowMetrics(item);

  let stateClass = 'state-queued';
  let barClass = '';
  const st = (item.state || '').toLowerCase();
  if (st === 'downloading') { stateClass = 'state-downloading'; }
  else if (st === 'completed') { stateClass = 'state-completed'; barClass = 'completed'; }
  else if (st === 'paused') { stateClass = 'state-paused'; barClass = 'paused'; }
  else if (st === 'failed') { stateClass = 'state-failed'; barClass = 'failed'; }
  else if (st === 'probing') { stateClass = 'state-probing'; }
  else if (st === 'verifying') { stateClass = 'state-verifying'; barClass = 'paused'; }
  else if (st === 'seeding') { stateClass = 'state-seeding'; barClass = 'completed'; }
  else if (st === 'cancelled') { stateClass = 'state-cancelled'; }

  const tr = el('tr');
  tr.dataset.id = String(item.id);
  tr.dataset.state = st;

  const name = labelledCell('Filename', 'filename-cell');
  name.textContent = item.filename || item.url;
  name.title = item.filename || item.url;
  tr.appendChild(name);

  tr.appendChild(Object.assign(labelledCell('Size'), { textContent: total ? formatBytes(total) : '--' }));

  const progress = labelledCell('Progress');
  const container = el('div', 'progress-bar-container');
  const bar = el('div', ('progress-bar ' + barClass).trim());
  bar.style.width = Math.min(100, Math.max(0, pct)) + '%';
  container.appendChild(bar);
  const meta = el('div', 'progress-meta');
  meta.appendChild(el('span', '', pct.toFixed(1) + '%'));
  meta.appendChild(el('span', '', etaText));
  progress.appendChild(container);
  progress.appendChild(meta);
  tr.appendChild(progress);

  tr.appendChild(Object.assign(labelledCell('Speed'), { textContent: speed > 0 ? formatSpeed(speed) : '--' }));

  const stateCell = labelledCell('State');
  stateCell.appendChild(el('span', 'state-badge ' + stateClass, st));
  tr.appendChild(stateCell);

  const actionsCell = labelledCell('Actions');
  actionsCell.style.textAlign = 'right';
  const actions = el('div', 'action-buttons');
  if (st === 'downloading' || st === 'probing' || st === 'queued') {
    actions.appendChild(actionButton('Pause', 'pause', item.id, false));
  } else if (st === 'paused' || st === 'failed') {
    actions.appendChild(actionButton('Resume', 'resume', item.id, false));
  }
  if (st !== 'completed' && st !== 'cancelled') {
    actions.appendChild(actionButton('Cancel', 'cancel', item.id, false));
  }
  actions.appendChild(actionButton('Remove', 'remove', item.id, true));
  actionsCell.appendChild(actions);
  tr.appendChild(actionsCell);

  return tr;
}

function findRow(id) {
  return document.querySelector('#downloadsTableBody tr[data-id="' + String(id) + '"]');
}

// Changes only the cells that move while a download runs.
function updateRow(tr, item) {
  const { total, pct, speed, etaText } = rowMetrics(item);
  const cell = (label) => tr.querySelector('td[data-label="' + label + '"]');

  const name = cell('Filename');
  const title = item.filename || item.url;
  if (name.textContent !== title) {
    name.textContent = title;
    name.title = title;
  }
  cell('Size').textContent = total ? formatBytes(total) : '--';
  cell('Speed').textContent = speed > 0 ? formatSpeed(speed) : '--';
  tr.querySelector('.progress-bar').style.width = Math.min(100, Math.max(0, pct)) + '%';
  const meta = tr.querySelectorAll('.progress-meta span');
  meta[0].textContent = pct.toFixed(1) + '%';
  meta[1].textContent = etaText;
}

// Brings the table in line with `downloads`. A row whose state did not change
// is updated where it stands; a new row, or one whose buttons change, is built again.
function renderDownloads() {
  const tbody = document.getElementById('downloadsTableBody');
  if (!downloads || downloads.length === 0) {
    const tr = el('tr');
    const td = el('td', 'empty-state', 'No downloads yet');
    td.colSpan = 6;
    tr.appendChild(td);
    tbody.replaceChildren(tr);
    return;
  }
  const rows = downloads.map((item) => {
    const existing = findRow(item.id);
    const st = (item.state || '').toLowerCase();
    if (existing && existing.dataset.state === st) {
      updateRow(existing, item);
      return existing;
    }
    return renderRow(item);
  });
  tbody.replaceChildren(...rows);
}

// Row buttons use data attributes: inline handlers are blocked by the CSP.
document.getElementById('downloadsTableBody').addEventListener('click', (event) => {
  const button = event.target.closest('button[data-action]');
  if (!button) return;
  const id = Number(button.dataset.id);
  const handlers = {
    pause: window.pauseJob,
    resume: window.resumeJob,
    cancel: window.cancelJob,
    remove: window.removeJob,
  };
  const handler = handlers[button.dataset.action];
  if (handler) handler(id);
});

// Job Controls
window.pauseJob = async function(id) {
  try {
    await apiRequest(`/api/v1/downloads/${id}/pause`, { method: 'POST' });
  } catch (err) {
    alert('Pause failed: ' + err.message);
  }
};

window.resumeJob = async function(id) {
  try {
    await apiRequest(`/api/v1/downloads/${id}/resume`, { method: 'POST' });
  } catch (err) {
    alert('Resume failed: ' + err.message);
  }
};

window.cancelJob = async function(id) {
  try {
    await apiRequest(`/api/v1/downloads/${id}/cancel`, { method: 'POST' });
  } catch (err) {
    alert('Cancel failed: ' + err.message);
  }
};

window.removeJob = async function(id) {
  if (!confirm('Remove this download?')) return;
  try {
    await apiRequest(`/api/v1/downloads/${id}`, { method: 'DELETE' });
  } catch (err) {
    alert('Remove failed: ' + err.message);
  }
};

// WebSocket Handling
function connectWebSocket() {
  if (!authToken) return;
  if (socket) {
    socket.close();
  }

  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const wsUrl = `${protocol}//${window.location.host}/api/v1/events`;
  socket = new WebSocket(wsUrl);

  socket.onopen = () => {
    // Authenticate as first message
    socket.send(JSON.stringify({ token: authToken }));
  };

  socket.onmessage = (event) => {
    try {
      const msg = JSON.parse(event.data);
      if (msg.authenticated !== undefined) {
        if (msg.authenticated) {
          document.getElementById('wsStatus').classList.add('connected');
          document.getElementById('wsStatus').title = 'Connected';
          reconnectDelay = 1000;
        } else {
          showAuth();
        }
        return;
      }

      handleWsEvent(msg);
    } catch (err) {
      console.error('WebSocket parse error:', err);
    }
  };

  socket.onclose = () => {
    document.getElementById('wsStatus').classList.remove('connected');
    document.getElementById('wsStatus').title = 'Disconnected (reconnecting...)';
    if (authToken) {
      clearTimeout(reconnectTimeout);
      reconnectTimeout = setTimeout(() => {
        reconnectDelay = Math.min(reconnectDelay * 2, 30000);
        connectWebSocket();
      }, reconnectDelay);
    }
  };

  socket.onerror = (err) => {
    console.error('WebSocket error:', err);
  };
}

function handleWsEvent(event) {
  const type = event.type;
  const data = event.data;

  if (type === 'job_added') {
    const idx = downloads.findIndex(d => d.id === data.id);
    if (idx >= 0) {
      downloads[idx] = data;
    } else {
      downloads.unshift(data);
    }
    renderDownloads();
  } else if (type === 'job_state_changed') {
    const item = downloads.find(d => d.id === data.id);
    if (item) {
      item.state = data.state;
      renderDownloads();
    }
  } else if (type === 'job_progress') {
    const item = downloads.find(d => d.id === data.id);
    if (item) {
      item.downloaded_bytes = data.downloaded_bytes;
      item.total_bytes = data.total_bytes;
      item.speed_bps = data.speed_bps;
      if (data.total_bytes && data.total_bytes > 0) {
        item.progress_pct = (data.downloaded_bytes / data.total_bytes) * 100;
      }
      const row = findRow(item.id);
      if (row) {
        updateRow(row, item);
      } else {
        renderDownloads();
      }
    }
  } else if (type === 'job_removed') {
    downloads = downloads.filter(d => d.id !== data);
    renderDownloads();
  }
}

// Event Listeners
document.getElementById('authForm').addEventListener('submit', async (e) => {
  e.preventDefault();
  const input = document.getElementById('tokenInput');
  const token = input.value.trim();
  const errEl = document.getElementById('authError');
  errEl.style.display = 'none';

  try {
    authToken = token;
    // Verify token with downloads route
    await apiRequest('/api/v1/downloads');
    localStorage.setItem('odm_token', token);
    showMain();
  } catch (err) {
    authToken = '';
    errEl.textContent = err.message || 'Authentication failed';
    errEl.style.display = 'block';
  }
});

document.getElementById('disconnectBtn').addEventListener('click', () => {
  authToken = '';
  localStorage.removeItem('odm_token');
  showAuth();
});

// Add Download Modal Listeners
const addModal = document.getElementById('addModal');
document.getElementById('openAddModalBtn').addEventListener('click', () => {
  document.getElementById('downloadUrlInput').value = '';
  document.getElementById('addError').style.display = 'none';
  addModal.classList.add('active');
  document.getElementById('downloadUrlInput').focus();
});

function closeAddModal() {
  addModal.classList.remove('active');
}

document.getElementById('closeAddModalBtn').addEventListener('click', closeAddModal);
document.getElementById('cancelAddBtn').addEventListener('click', closeAddModal);

document.getElementById('addDownloadForm').addEventListener('submit', async (e) => {
  e.preventDefault();
  const urlInput = document.getElementById('downloadUrlInput');
  const url = urlInput.value.trim();
  const errEl = document.getElementById('addError');
  errEl.style.display = 'none';

  try {
    await apiRequest('/api/v1/downloads', {
      method: 'POST',
      body: JSON.stringify({ url }),
    });
    closeAddModal();
    loadDownloads();
  } catch (err) {
    errEl.textContent = err.message || 'Failed to add download';
    errEl.style.display = 'block';
  }
});

// Initial boot
loadVersion();
if (authToken) {
  showMain();
} else {
  showAuth();
}
