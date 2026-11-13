// @ts-check
// odm browser extension — content script

chrome.runtime.onMessage.addListener((request, sender, sendResponse) => {
  if (request.action === 'collectLinks') {
    const anchors = Array.from(document.querySelectorAll('a[href]'));
    const urls = new Set();

    for (const a of anchors) {
      try {
        const href = a.getAttribute('href');
        if (!href) continue;
        const resolved = new URL(href, window.location.href);
        if (resolved.protocol === 'http:' || resolved.protocol === 'https:') {
          urls.add(resolved.href);
        }
      } catch (_) {
        // Ignore malformed URLs
      }
    }

    sendResponse(Array.from(urls));
  }
  return true;
});
