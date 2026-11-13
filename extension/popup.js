// @ts-check
// odm browser extension — popup script

const DEFAULT_PORT = 6820;

const api = typeof chrome !== 'undefined' ? chrome : browser;

async function getSettings() {
  return new Promise((resolve) => {
    api.storage.sync.get(['port', 'token'], (items) => {
      resolve({
        port: items?.port || DEFAULT_PORT,
        token: items?.token || '',
      });
    });
  });
}

async function checkHealth(port) {
  try {
    const res = await fetch(`http://127.0.0.1:${port}/api/v1/health`);
    if (res.ok) {
      const data = await res.json();
      return { ok: true, version: data.version };
    }
  } catch (_) {}
  return { ok: false, version: null };
}

async function getActiveTab() {
  return new Promise((resolve) => {
    api.tabs.query({ active: true, currentWindow: true }, (tabs) => {
      resolve(tabs && tabs.length > 0 ? tabs[0] : null);
    });
  });
}

async function queryTabMedia(tabId) {
  return new Promise((resolve) => {
    api.runtime.sendMessage({ action: 'getMedia', tabId }, (response) => {
      resolve(response || []);
    });
  });
}

async function sendToDaemon(url, port, token) {
  const resp = await fetch(`http://127.0.0.1:${port}/api/v1/downloads`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { 'Authorization': `Bearer ${token}` } : {}),
    },
    body: JSON.stringify({ url }),
  });
  return resp.ok;
}

document.addEventListener('DOMContentLoaded', async () => {
  const statusDot = document.getElementById('statusDot');
  const statusText = document.getElementById('statusText');
  const daemonVersion = document.getElementById('daemonVersion');
  const mediaList = document.getElementById('mediaList');
  const sendAllBtn = document.getElementById('sendAllBtn');
  const openOptionsBtn = document.getElementById('openOptionsBtn');

  openOptionsBtn.addEventListener('click', () => {
    if (api.runtime.openOptionsPage) {
      api.runtime.openOptionsPage();
    } else {
      window.open(api.runtime.getURL('options.html'));
    }
  });

  const settings = await getSettings();
  const health = await checkHealth(settings.port);

  if (health.ok) {
    statusDot.classList.add('connected');
    statusText.textContent = 'Connected';
    if (health.version) {
      daemonVersion.textContent = `v${health.version}`;
    }
  } else {
    statusDot.classList.remove('connected');
    statusText.textContent = 'Disconnected';
    daemonVersion.textContent = `Port: ${settings.port}`;
  }

  const activeTab = await getActiveTab();
  if (!activeTab || activeTab.id == null) return;

  const mediaItems = await queryTabMedia(activeTab.id);

  if (mediaItems.length === 0) {
    mediaList.innerHTML = `<div class="empty-msg">No media detected on this tab</div>`;
    sendAllBtn.style.display = 'none';
    return;
  }

  sendAllBtn.style.display = 'block';
  sendAllBtn.addEventListener('click', async () => {
    for (const item of mediaItems) {
      await sendToDaemon(item.url, settings.port, settings.token);
    }
    sendAllBtn.textContent = 'Sent!';
    setTimeout(() => { sendAllBtn.textContent = 'Send All'; }, 1500);
  });

  mediaList.innerHTML = mediaItems.map((item, idx) => `
    <div class="media-item">
      <div class="media-url" title="${item.url}">${item.url}</div>
      <div class="media-actions">
        <button class="btn btn-primary btn-sm" id="sendMediaBtn-${idx}">Send to odm</button>
      </div>
    </div>
  `).join('');

  mediaItems.forEach((item, idx) => {
    const btn = document.getElementById(`sendMediaBtn-${idx}`);
    if (btn) {
      btn.addEventListener('click', async () => {
        btn.textContent = 'Sending...';
        btn.setAttribute('disabled', 'true');
        const ok = await sendToDaemon(item.url, settings.port, settings.token);
        if (ok) {
          btn.textContent = 'Sent!';
        } else {
          btn.textContent = 'Failed';
          setTimeout(() => {
            btn.textContent = 'Send to odm';
            btn.removeAttribute('disabled');
          }, 2000);
        }
      });
    }
  });
});
