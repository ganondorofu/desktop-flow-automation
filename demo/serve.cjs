// Minimal static file server for the demo site — no dependencies.
// Usage: node demo/serve.cjs  (serves demo/site on http://localhost:8787)
const http = require("http");
const fs = require("fs");
const path = require("path");

const ROOT = path.join(__dirname, "site");
const PORT = 8787;

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
};

http
  .createServer((req, res) => {
    let urlPath = decodeURIComponent(req.url.split("?")[0]);
    if (urlPath === "/") urlPath = "/price.html";
    const filePath = path.join(ROOT, urlPath);
    if (!filePath.startsWith(ROOT)) {
      res.writeHead(403);
      res.end();
      return;
    }
    fs.readFile(filePath, (err, data) => {
      if (err) {
        res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
        res.end("Not found: " + urlPath);
        return;
      }
      const ext = path.extname(filePath);
      res.writeHead(200, { "Content-Type": MIME[ext] || "application/octet-stream" });
      res.end(data);
    });
  })
  .listen(PORT, () => {
    console.log(`Demo site running at http://localhost:${PORT}/price.html`);
  });
