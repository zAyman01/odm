// @ts-check
// odm browser extension — background service worker

import { shouldIntercept } from './filter.js';

const DEFAULT_PORT = 6820;
const ext = typeof chrome !== 'undefined' ? chrome : browser;

/** @type {Map<number, {url: string, mime: string}[]>} tabId → detected media */
const detectedMedia = new Map();

/**
 * Reads user settings from storage.
 */
async function getSettings() {
  return new Promise((resolve) => {
    ext.storage.sync.get(
      ['token', 'port', 'enabled', 'minSize', 'extensionList', 'disabledSites'],
      (items) => {
        resolve({
          token: items?.token || '',
          port: items?.port || DEFAULT_PORT,
          enabled: items?.enabled !== false,
          minSize: items?.minSize || 0,
          extensionList: items?.extensionList || '',
          disabledSites: items?.disabledSites || '',
        });
      }
    );
  });
}

/**
 * Retrieves cookies for a URL to pass to odm for authenticated downloads.
 */
async function getCookiesForUrl(url) {
  return new Promise((resolve) => {
    if (!ext.cookies || !ext.cookies.getAll) {
      resolve([]);
      return;
    }
    ext.cookies.getAll({ url }, (cookies) => {
      resolve(cookies || []);
    });
  });
}

/**
 * Sends a download request to the local odm daemon.
 */
async function sendToDaemon(url, extras = {}) {
  const settings = await getSettings();
  if (!settings.token) {
    return false;
  }
  const base = `http://127.0.0.1:${settings.port || DEFAULT_PORT}`;
  try {
    const resp = await fetch(`${base}/api/v1/downloads`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${settings.token}`,
      },
      body: JSON.stringify({ url, ...extras }),
    });
    return resp.ok;
  } catch (_) {
    return false;
  }
}

// --- Download Capture ---
if (ext.downloads && ext.downloads.onCreated) {
  ext.downloads.onCreated.addListener(async (downloadItem) => {
    const settings = await getSettings();
    if (!settings.token) return; // Not configured yet, allow normal browser download

    const url = downloadItem.url;
    if (!url || !url.startsWith('http://') && !url.startsWith('https://')) return;

    const fileSize = downloadItem.fileSize || downloadItem.totalBytes || 0;
    let hostname = '';
    try {
      hostname = new URL(downloadItem.referrer || downloadItem.url).hostname;
    } catch (_) {}

    const filterOpts = {
      enabled: settings.enabled !== false,
      minSizeBytes: (settings.minSize || 0) * 1024,
      extensionList: settings.extensionList
        ? settings.extensionList.split(',').map((e) => e.trim()).filter(Boolean)
        : [],
      disabledSites: settings.disabledSites
        ? settings.disabledSites.split('\n').map((s) => s.trim()).filter(Boolean)
        : [],
    };

    if (!shouldIntercept(url, fileSize, hostname, filterOpts)) return;

    // Retrieve cookies and referrer
    const cookies = await getCookiesForUrl(url);
    const cookieStr = cookies.map((c) => `${c.name}=${c.value}`).join('; ');
    const extras = { options: {} };

    if (cookieStr) {
      extras.options.cookies = cookieStr;
    }
    if (downloadItem.referrer) {
      extras.options.referer = downloadItem.referrer;
    }

    try {
      const ok = await sendToDaemon(url, extras);
      if (ok) {
        // Cancel the browser download
        ext.downloads.cancel(downloadItem.id, () => {
          ext.downloads.erase({ id: downloadItem.id }, () => {});
        });
      }
      // If daemon returned error, let browser download proceed (fallback)
    } catch (_) {
      // Daemon unreachable — fallback: browser download proceeds normally
    }
  });
}

// --- Context Menu ---
ext.runtime.onInstalled.addListener(() => {
  if (ext.contextMenus) {
    ext.contextMenus.create({
      id: 'odm-download-link',
      title: 'Download with odm',
      contexts: ['link'],
    });
    ext.contextMenus.create({
      id: 'odm-download-all',
      title: 'Download all links',
      contexts: ['page'],
    });
  }
});

if (ext.contextMenus && ext.contextMenus.onClicked) {
  ext.contextMenus.onClicked.addListener(async (info, tab) => {
    if (info.menuItemId === 'odm-download-link' && info.linkUrl) {
      await sendToDaemon(info.linkUrl);
    } else if (info.menuItemId === 'odm-download-all' && tab?.id) {
      ext.tabs.sendMessage(tab.id, { action: 'collectLinks' }, async (links) => {
        if (ext.runtime.lastError || !Array.isArray(links)) return;
        for (const link of links) {
          await sendToDaemon(link);
        }
      });
    }
  });
}

// --- Media Detection ---
if (ext.webRequest && ext.webRequest.onBeforeRequest) {
  ext.webRequest.onBeforeRequest.addListener(
    (details) => {
      const url = details.url;
      if (!url) return;

      const isMedia =
        url.includes('.m3u8') ||
        url.includes('.mpd') ||
        details.type === 'media' ||
        (details.type === 'xmlhttprequest' && (url.includes('.m3u8') || url.includes('.mpd')));

      if (isMedia && details.tabId >= 0) {
        const tabMedia = detectedMedia.get(details.tabId) || [];
        if (!tabMedia.some((m) => m.url === url)) {
          tabMedia.push({ url, mime: details.type || '' });
          detectedMedia.set(details.tabId, tabMedia);

          if (ext.action) {
            ext.action.setBadgeText({
              text: String(tabMedia.length),
              tabId: details.tabId,
            });
            ext.action.setBadgeBackgroundColor({
              color: '#4a9eff',
              tabId: details.tabId,
            });
          }
        }
      }
    },
    { urls: ['<all_urls>'] }
  );
}

// Clear media on tab navigation
if (ext.tabs && ext.tabs.onUpdated) {
  ext.tabs.onUpdated.addListener((tabId, changeInfo) => {
    if (changeInfo.status === 'loading') {
      detectedMedia.delete(tabId);
      if (ext.action) {
        ext.action.setBadgeText({ text: '', tabId });
      }
    }
  });
}

if (ext.tabs && ext.tabs.onRemoved) {
  ext.tabs.onRemoved.addListener((tabId) => {
    detectedMedia.delete(tabId);
  });
}

// Message handler for popup
ext.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.action === 'getMedia' && msg.tabId != null) {
    sendResponse(detectedMedia.get(msg.tabId) || []);
  }
  return true;
});
