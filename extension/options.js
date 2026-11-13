// @ts-check
// odm browser extension — options script

const DEFAULT_PORT = 6820;
const DEFAULT_EXTENSIONS =
  'zip, exe, msi, dmg, pkg, deb, rpm, AppImage, iso, rar, 7z, tar, gz, bz2, xz, pdf, doc, docx, xls, xlsx, mp4, mkv, avi, mp3, flac, wav';

const api = typeof chrome !== 'undefined' ? chrome : browser;

document.addEventListener('DOMContentLoaded', () => {
  const enabledInput = /** @type {HTMLInputElement} */ (document.getElementById('enabledInput'));
  const tokenInput = /** @type {HTMLInputElement} */ (document.getElementById('tokenInput'));
  const portInput = /** @type {HTMLInputElement} */ (document.getElementById('portInput'));
  const minSizeInput = /** @type {HTMLInputElement} */ (document.getElementById('minSizeInput'));
  const extensionListInput = /** @type {HTMLTextAreaElement} */ (document.getElementById('extensionListInput'));
  const disabledSitesInput = /** @type {HTMLTextAreaElement} */ (document.getElementById('disabledSitesInput'));
  const toggleTokenBtn = document.getElementById('toggleTokenBtn');
  const testConnBtn = document.getElementById('testConnBtn');
  const optionsForm = document.getElementById('optionsForm');
  const statusMsg = document.getElementById('statusMessage');

  // Load existing settings
  // The token is kept in storage.local so it is not synced to the browser account.
  api.storage.local.get(['token'], (local) => {
    if (local?.token) tokenInput.value = local.token;
  });
  api.storage.sync.get(
    ['token', 'port', 'enabled', 'minSize', 'extensionList', 'disabledSites'],
    (items) => {
      enabledInput.checked = items?.enabled !== false;
      // Older versions saved the token in storage.sync; show it until it is saved again.
      if (items?.token && !tokenInput.value) tokenInput.value = items.token;
      portInput.value = String(items?.port || DEFAULT_PORT);
      minSizeInput.value = String(items?.minSize ?? 0);
      extensionListInput.value = items?.extensionList !== undefined ? items.extensionList : DEFAULT_EXTENSIONS;
      disabledSitesInput.value = items?.disabledSites || '';
    }
  );

  // Toggle show/hide token
  toggleTokenBtn.addEventListener('click', () => {
    if (tokenInput.type === 'password') {
      tokenInput.type = 'text';
      toggleTokenBtn.textContent = 'Hide';
    } else {
      tokenInput.type = 'password';
      toggleTokenBtn.textContent = 'Show';
    }
  });

  function showStatus(text, isSuccess) {
    statusMsg.textContent = text;
    statusMsg.className = `status-msg ${isSuccess ? 'success' : 'error'}`;
    setTimeout(() => {
      statusMsg.className = 'status-msg';
    }, 4000);
  }

  // Test Connection
  testConnBtn.addEventListener('click', async () => {
    const port = portInput.value.trim() || DEFAULT_PORT;
    const token = tokenInput.value.trim();

    testConnBtn.setAttribute('disabled', 'true');
    testConnBtn.textContent = 'Testing...';

    try {
      const healthRes = await fetch(`http://127.0.0.1:${port}/api/v1/health`);
      if (!healthRes.ok) {
        throw new Error(`Daemon unreachable (status ${healthRes.status})`);
      }
      const healthData = await healthRes.json();

      if (token) {
        const authRes = await fetch(`http://127.0.0.1:${port}/api/v1/downloads`, {
          headers: { 'Authorization': `Bearer ${token}` },
        });
        if (authRes.status === 401) {
          throw new Error('Connection succeeded, but bearer token is invalid');
        }
      }

      showStatus(`Connected to odm v${healthData.version || '0.1.0'} successfully!`, true);
    } catch (err) {
      showStatus(`Connection failed: ${err.message}`, false);
    } finally {
      testConnBtn.removeAttribute('disabled');
      testConnBtn.textContent = 'Test Connection';
    }
  });

  // Save Settings
  optionsForm.addEventListener('submit', (e) => {
    e.preventDefault();

    const settings = {
      enabled: enabledInput.checked,
      port: parseInt(portInput.value.trim(), 10) || DEFAULT_PORT,
      minSize: parseInt(minSizeInput.value.trim(), 10) || 0,
      extensionList: extensionListInput.value.trim(),
      disabledSites: disabledSitesInput.value.trim(),
    };

    api.storage.local.set({ token: tokenInput.value.trim() }, () => {
      api.storage.sync.remove(['token'], () => {
        api.storage.sync.set(settings, () => {
          showStatus('Settings saved successfully!', true);
        });
      });
    });
  });
});
