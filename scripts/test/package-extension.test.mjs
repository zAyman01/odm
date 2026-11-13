import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { execSync } from 'node:child_process';

test('package-extension creates valid Chrome and Firefox zip bundles', () => {
  const outDir = path.resolve('target/test-extension-dist');
  if (fs.existsSync(outDir)) {
    fs.rmSync(outDir, { recursive: true, force: true });
  }

  execSync(`node scripts/package-extension.mjs --out-dir "${outDir}"`, {
    stdio: 'pipe',
    encoding: 'utf8',
  });

  const chromeZip = path.join(outDir, 'odm-extension-chrome-v0.1.0.zip');
  const firefoxZip = path.join(outDir, 'odm-extension-firefox-v0.1.0.zip');

  assert.ok(fs.existsSync(chromeZip), 'Chrome extension zip must exist');
  assert.ok(fs.existsSync(firefoxZip), 'Firefox extension zip must exist');

  assert.ok(fs.statSync(chromeZip).size > 1000, 'Chrome extension zip must not be empty');
  assert.ok(fs.statSync(firefoxZip).size > 1000, 'Firefox extension zip must not be empty');

  // Verify staging directories content
  const chromeStage = path.join(outDir, 'chrome');
  const firefoxStage = path.join(outDir, 'firefox');

  assert.ok(fs.existsSync(path.join(chromeStage, 'manifest.json')), 'Chrome stage must have manifest.json');
  assert.ok(fs.existsSync(path.join(firefoxStage, 'manifest.json')), 'Firefox stage must have manifest.json');

  const chromeManifest = JSON.parse(fs.readFileSync(path.join(chromeStage, 'manifest.json'), 'utf8'));
  assert.equal(chromeManifest.name, 'odm — Open Download Manager');
  assert.ok(chromeManifest.background.service_worker, 'Chrome manifest must have service_worker');

  const firefoxManifest = JSON.parse(fs.readFileSync(path.join(firefoxStage, 'manifest.json'), 'utf8'));
  assert.equal(firefoxManifest.name, 'odm — Open Download Manager');
  assert.ok(firefoxManifest.background.scripts, 'Firefox manifest must have background.scripts');

  // Ensure test files are not packaged
  assert.ok(!fs.existsSync(path.join(chromeStage, 'test')), 'Chrome stage must not contain test folder');
  assert.ok(!fs.existsSync(path.join(firefoxStage, 'test')), 'Firefox stage must not contain test folder');

  // Ensure un-renamed browser manifests are not packaged in the output
  assert.ok(!fs.existsSync(path.join(chromeStage, 'manifest.chrome.json')), 'Chrome stage must not contain manifest.chrome.json');
  assert.ok(!fs.existsSync(path.join(chromeStage, 'manifest.firefox.json')), 'Chrome stage must not contain manifest.firefox.json');
  assert.ok(!fs.existsSync(path.join(firefoxStage, 'manifest.chrome.json')), 'Firefox stage must not contain manifest.chrome.json');
  assert.ok(!fs.existsSync(path.join(firefoxStage, 'manifest.firefox.json')), 'Firefox stage must not contain manifest.firefox.json');
});
