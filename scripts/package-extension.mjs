#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';

// Parse command line arguments
const args = process.argv.slice(2);
let outDir = 'dist';
let version = '0.1.0';

for (let i = 0; i < args.length; i++) {
  if (args[i] === '--out-dir' && args[i + 1]) {
    outDir = args[++i];
  } else if (args[i] === '--version' && args[i + 1]) {
    version = args[++i];
  }
}

// Attempt to read version from Cargo.toml if default
if (version === '0.1.0') {
  try {
    const cargoToml = fs.readFileSync('Cargo.toml', 'utf8');
    const match = cargoToml.match(/version\s*=\s*"([^"]+)"/);
    if (match) {
      version = match[1];
    }
  } catch {
    // fallback to default
  }
}

const rootDir = process.cwd();
const extDir = path.join(rootDir, 'extension');
const resolvedOutDir = path.resolve(outDir);

fs.mkdirSync(resolvedOutDir, { recursive: true });

// CRC32 table
const crcTable = new Uint32Array(256);
for (let n = 0; n < 256; n++) {
  let c = n;
  for (let k = 0; k < 8; k++) {
    c = (c & 1) ? (0xedb88320 ^ (c >>> 1)) : (c >>> 1);
  }
  crcTable[n] = c;
}

function computeCrc32(buf) {
  let crc = ~0;
  for (let i = 0; i < buf.length; i++) {
    crc = (crc >>> 8) ^ crcTable[(crc ^ buf[i]) & 0xff];
  }
  return (~crc) >>> 0;
}

// Minimal zip creator using Node zlib
function createZip(files, destZipPath) {
  const localHeaders = [];
  const centralHeaders = [];
  let offset = 0;

  const now = new Date();
  const dosTime = ((now.getHours() << 11) | (now.getMinutes() << 5) | (now.getSeconds() >> 1)) & 0xffff;
  const dosDate = (((now.getFullYear() - 1980) << 9) | ((now.getMonth() + 1) << 5) | now.getDate()) & 0xffff;

  for (const { relativePath, data } of files) {
    const normalizedPath = relativePath.replace(/\\/g, '/');
    const pathBuf = Buffer.from(normalizedPath, 'utf8');
    const crc = computeCrc32(data);
    const compressed = zlib.deflateRawSync(data);

    // Local file header
    const localHeader = Buffer.alloc(30 + pathBuf.length);
    localHeader.writeUInt32LE(0x04034b50, 0); // signature
    localHeader.writeUInt16LE(20, 4);         // version needed (2.0)
    localHeader.writeUInt16LE(0x0800, 6);     // flags (UTF-8)
    localHeader.writeUInt16LE(8, 8);          // compression (deflate)
    localHeader.writeUInt16LE(dosTime, 10);
    localHeader.writeUInt16LE(dosDate, 12);
    localHeader.writeUInt32LE(crc, 14);
    localHeader.writeUInt32LE(compressed.length, 18);
    localHeader.writeUInt32LE(data.length, 22);
    localHeader.writeUInt16LE(pathBuf.length, 26);
    localHeader.writeUInt16LE(0, 28);         // extra field length
    pathBuf.copy(localHeader, 30);

    localHeaders.push(localHeader, compressed);

    // Central directory file header
    const centralHeader = Buffer.alloc(46 + pathBuf.length);
    centralHeader.writeUInt32LE(0x02014b50, 0); // signature
    centralHeader.writeUInt16LE(20, 4);         // version made by
    centralHeader.writeUInt16LE(20, 6);         // version needed
    centralHeader.writeUInt16LE(0x0800, 8);     // flags (UTF-8)
    centralHeader.writeUInt16LE(8, 10);         // compression (deflate)
    centralHeader.writeUInt16LE(dosTime, 12);
    centralHeader.writeUInt16LE(dosDate, 14);
    centralHeader.writeUInt32LE(crc, 16);
    centralHeader.writeUInt32LE(compressed.length, 20);
    centralHeader.writeUInt32LE(data.length, 24);
    centralHeader.writeUInt16LE(pathBuf.length, 28);
    centralHeader.writeUInt16LE(0, 30);         // extra field length
    centralHeader.writeUInt16LE(0, 32);         // comment length
    centralHeader.writeUInt16LE(0, 34);         // disk number start
    centralHeader.writeUInt16LE(0, 36);         // internal attrs
    centralHeader.writeUInt32LE(0, 38);         // external attrs
    centralHeader.writeUInt32LE(offset, 42);    // relative offset
    pathBuf.copy(centralHeader, 46);

    centralHeaders.push(centralHeader);

    offset += localHeader.length + compressed.length;
  }

  const centralDirOffset = offset;
  let centralDirSize = 0;
  for (const h of centralHeaders) {
    centralDirSize += h.length;
  }

  // End of central directory record
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);       // signature
  eocd.writeUInt16LE(0, 4);                // disk number
  eocd.writeUInt16LE(0, 6);                // disk start
  eocd.writeUInt16LE(files.length, 8);     // total entries on disk
  eocd.writeUInt16LE(files.length, 10);    // total entries
  eocd.writeUInt32LE(centralDirSize, 12);  // central dir size
  eocd.writeUInt32LE(centralDirOffset, 16);// central dir offset
  eocd.writeUInt16LE(0, 20);               // comment length

  const finalZip = Buffer.concat([...localHeaders, ...centralHeaders, eocd]);
  fs.writeFileSync(destZipPath, finalZip);
}

// Stage and package for a target browser
function stageAndPackage(targetBrowser, manifestSourceFile) {
  const stageDir = path.join(resolvedOutDir, targetBrowser);
  if (fs.existsSync(stageDir)) {
    fs.rmSync(stageDir, { recursive: true, force: true });
  }
  fs.mkdirSync(stageDir, { recursive: true });

  // List of shared files
  const sharedFiles = [
    'background.js',
    'content.js',
    'filter.js',
    'popup.html',
    'popup.js',
    'popup.css',
    'options.html',
    'options.js',
    'options.css',
  ];

  for (const f of sharedFiles) {
    const src = path.join(extDir, f);
    if (fs.existsSync(src)) {
      fs.copyFileSync(src, path.join(stageDir, f));
    }
  }

  // Icons
  const iconsSrcDir = path.join(extDir, 'icons');
  const iconsDstDir = path.join(stageDir, 'icons');
  fs.mkdirSync(iconsDstDir, { recursive: true });
  for (const icon of fs.readdirSync(iconsSrcDir)) {
    fs.copyFileSync(path.join(iconsSrcDir, icon), path.join(iconsDstDir, icon));
  }

  // Copy target manifest as manifest.json
  const manifestContent = fs.readFileSync(path.join(extDir, manifestSourceFile), 'utf8');
  fs.writeFileSync(path.join(stageDir, 'manifest.json'), manifestContent, 'utf8');

  // Collect files to zip
  const filesToZip = [];
  function collectFiles(dir, relPrefix = '') {
    const entries = fs.readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
      const fullPath = path.join(dir, entry.name);
      const relPath = relPrefix ? `${relPrefix}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        collectFiles(fullPath, relPath);
      } else if (entry.isFile()) {
        filesToZip.push({
          relativePath: relPath,
          data: fs.readFileSync(fullPath),
        });
      }
    }
  }

  collectFiles(stageDir);

  const zipName = `odm-extension-${targetBrowser}-v${version}.zip`;
  const destZip = path.join(resolvedOutDir, zipName);
  createZip(filesToZip, destZip);
  console.log(`Packaged ${zipName} (${filesToZip.length} files) -> ${destZip}`);
}

stageAndPackage('chrome', 'manifest.chrome.json');
stageAndPackage('firefox', 'manifest.firefox.json');
console.log('Extension packaging completed successfully.');
