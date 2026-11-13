import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { shouldIntercept } from '../filter.js';

describe('shouldIntercept', () => {
  const defaults = {
    enabled: true,
    minSizeBytes: 1024,
    extensionList: ['zip', 'exe', 'pdf', 'mp4', 'iso'],
    disabledSites: [],
  };

  it('intercepts a matching extension above min size', () => {
    assert.strictEqual(
      shouldIntercept('https://example.com/file.zip', 50000, 'example.com', defaults),
      true
    );
  });

  it('ignores a file below minimum size', () => {
    assert.strictEqual(
      shouldIntercept('https://example.com/tiny.zip', 500, 'example.com', defaults),
      false
    );
  });

  it('ignores a non-matching extension', () => {
    assert.strictEqual(
      shouldIntercept('https://example.com/page.html', 50000, 'example.com', defaults),
      false
    );
  });

  it('ignores when site is disabled', () => {
    const opts = { ...defaults, disabledSites: ['example.com'] };
    assert.strictEqual(
      shouldIntercept('https://example.com/file.zip', 50000, 'example.com', opts),
      false
    );
  });

  it('ignores when interception is globally disabled', () => {
    const opts = { ...defaults, enabled: false };
    assert.strictEqual(
      shouldIntercept('https://example.com/file.zip', 50000, 'example.com', opts),
      false
    );
  });

  it('intercepts when size is unknown (0) and extension matches', () => {
    assert.strictEqual(
      shouldIntercept('https://example.com/file.exe', 0, 'example.com', defaults),
      true
    );
  });

  it('extracts extension case-insensitively', () => {
    assert.strictEqual(
      shouldIntercept('https://example.com/FILE.ZIP', 50000, 'example.com', defaults),
      true
    );
  });

  it('handles URLs with query strings', () => {
    assert.strictEqual(
      shouldIntercept('https://example.com/file.pdf?token=abc', 50000, 'example.com', defaults),
      true
    );
  });

  it('intercepts with empty extension list (all extensions)', () => {
    const opts = { ...defaults, extensionList: [] };
    assert.strictEqual(
      shouldIntercept('https://example.com/anything.xyz', 50000, 'example.com', opts),
      true
    );
  });

  it('handles subdomain matching for disabled sites', () => {
    const opts = { ...defaults, disabledSites: ['example.com'] };
    assert.strictEqual(
      shouldIntercept('https://sub.example.com/file.zip', 50000, 'sub.example.com', opts),
      true
    );
  });
});
