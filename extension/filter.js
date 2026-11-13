/**
 * Pure filter logic for the odm browser extension.
 * Determines whether a browser download should be intercepted and sent to odm.
 *
 * @param {string} url - The download URL.
 * @param {number} fileSize - Known file size in bytes (0 if unknown).
 * @param {string} hostname - The hostname of the page that triggered the download.
 * @param {object} opts - Filter options.
 * @param {boolean} opts.enabled - Whether interception is globally enabled.
 * @param {number} opts.minSizeBytes - Minimum file size to intercept (0 = no minimum).
 * @param {string[]} opts.extensionList - File extensions to intercept (empty = all).
 * @param {string[]} opts.disabledSites - Hostnames where interception is disabled.
 * @returns {boolean} Whether the download should be intercepted.
 */
export function shouldIntercept(url, fileSize, hostname, opts) {
  if (!opts.enabled) return false;

  // Check disabled sites (exact match only)
  if (opts.disabledSites.some(site => hostname === site)) return false;

  // Check minimum size (0 = unknown, always intercept; skip check for unknown)
  if (fileSize > 0 && opts.minSizeBytes > 0 && fileSize < opts.minSizeBytes) return false;

  // Check extension filter (empty list = all extensions)
  if (opts.extensionList.length > 0) {
    const urlPath = url.split('?')[0].split('#')[0];
    const lastSegment = urlPath.split('/').pop() || '';
    const dotIndex = lastSegment.lastIndexOf('.');
    if (dotIndex === -1) return false;
    const ext = lastSegment.substring(dotIndex + 1).toLowerCase();
    if (!opts.extensionList.some(e => e.toLowerCase() === ext)) return false;
  }

  return true;
}
