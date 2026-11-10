// Serve a built web bundle on loopback, falling back to index.html.
// Usage: node scripts/serve-static.mjs <bundle-dir> <port>
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';

const [root, port] = process.argv.slice(2);
if (!root || !port) {
  console.error('usage: serve-static.mjs <bundle-dir> <port>');
  process.exit(2);
}
const base = path.resolve(root);
const types = {
  '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css',
  '.svg': 'image/svg+xml', '.png': 'image/png', '.json': 'application/json',
};
http.createServer((req, res) => {
  let file = path.join(base, decodeURIComponent(new URL(req.url, 'http://localhost').pathname));
  if (!file.startsWith(base) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
    file = path.join(base, 'index.html');
  }
  res.setHeader('Content-Type', types[path.extname(file)] || 'application/octet-stream');
  res.end(fs.readFileSync(file));
}).listen(Number(port), '127.0.0.1');
