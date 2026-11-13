import { describe, it, beforeEach } from 'node:test';
import assert from 'node:assert/strict';

let instance = 0;

/** Builds a fake WebExtension API whose storage areas live in `store`. */
function makeChrome(store) {
  const listeners = {};
  const cancelled = [];
  const area = (name) => ({
    get(keys, cb) {
      const out = {};
      for (const key of Array.isArray(keys) ? keys : [keys]) {
        if (key in store[name]) out[key] = store[name][key];
      }
      if (cb) cb(out);
      return Promise.resolve(out);
    },
    set(items, cb) {
      Object.assign(store[name], items);
      if (cb) cb();
      return Promise.resolve();
    },
    remove(keys, cb) {
      for (const key of Array.isArray(keys) ? keys : [keys]) delete store[name][key];
      if (cb) cb();
      return Promise.resolve();
    },
  });
  const event = (name) => ({ addListener: (fn) => { listeners[name] = fn; } });
  const chrome = {
    storage: { sync: area('sync'), local: area('local'), session: area('session') },
    downloads: {
      onCreated: event('downloadCreated'),
      cancel: (id, cb) => { cancelled.push(id); if (cb) cb(); },
      erase: (_query, cb) => { if (cb) cb(); },
    },
    cookies: { getAll: (_query, cb) => cb([]) },
    runtime: { onInstalled: event('installed'), onMessage: event('message') },
    contextMenus: { create: () => {}, onClicked: event('menuClicked') },
    webRequest: { onBeforeRequest: event('beforeRequest') },
    tabs: { onUpdated: event('tabUpdated'), onRemoved: event('tabRemoved'), sendMessage: () => {} },
    action: { setBadgeText: () => {}, setBadgeBackgroundColor: () => {} },
  };
  return { chrome, listeners, cancelled };
}

/** Loads a fresh copy of background.js, as a restarted service worker would. */
async function loadBackground(store) {
  const fake = makeChrome(store);
  globalThis.chrome = fake.chrome;
  const module = await import(`../background.js?instance=${instance++}`);
  return { module, ...fake };
}

function getMedia(listeners, tabId) {
  return new Promise((resolve) => {
    listeners.message({ action: 'getMedia', tabId }, {}, resolve);
  });
}

describe('background', () => {
  let store;
  beforeEach(() => {
    store = { sync: {}, local: {}, session: {} };
  });

  it('reads the token from storage.local and migrates it out of storage.sync', async () => {
    store.sync = { token: 'abc', port: 7000 };
    const { module } = await loadBackground(store);

    const settings = await module.getSettings();

    assert.equal(settings.token, 'abc');
    assert.equal(settings.port, 7000);
    assert.equal(store.local.token, 'abc');
    assert.equal('token' in store.sync, false);
  });

  it('keeps the detected media list when the service worker restarts', async () => {
    const first = await loadBackground(store);
    await first.listeners.beforeRequest({
      url: 'https://cdn.example/video.m3u8',
      tabId: 5,
      type: 'xmlhttprequest',
    });

    const second = await loadBackground(store);
    const media = await getMedia(second.listeners, 5);

    assert.deepEqual(
      media.map((item) => item.url),
      ['https://cdn.example/video.m3u8']
    );
  });

  describe('browser download capture', () => {
    const item = {
      id: 1,
      url: 'https://example.com/file.zip',
      fileSize: 5_000_000,
      referrer: 'https://example.com/',
    };

    it('leaves the browser download alone when the daemon rejects it', async () => {
      store.local.token = 'abc';
      globalThis.fetch = async () => ({ ok: false, status: 500 });
      const { listeners, cancelled } = await loadBackground(store);

      await listeners.downloadCreated(item);

      assert.deepEqual(cancelled, []);
    });

    it('cancels the browser download once the daemon accepts it', async () => {
      store.local.token = 'abc';
      globalThis.fetch = async () => ({ ok: true, status: 200 });
      const { listeners, cancelled } = await loadBackground(store);

      await listeners.downloadCreated(item);

      assert.deepEqual(cancelled, [1]);
    });
  });
});
