#!/usr/bin/env node
// tests/mock-codync/server.js
// Mock Codync host for Phase 2 contract tests (test-matrix §15).
//
// HTTP stub: GET /health, POST /api/<method>, GET /events (SSE).
// Control:   POST /__control  — push event, clear log, close streams.
// Query:    GET  /__calls     — return the method-call log.
//
// The call log is the "nothing proxied" assertion surface (GATE-01..05):
// every POST /api/<method> is recorded with method name + parsed body.
//
// Env: MOCK_PORT (default 0 = ephemeral), MOCK_LOG (optional log file).
//
// SPDX-License-Identifier: GPL-3.0-only

const http = require('http');
const fs = require('fs');

function createMockHost(opts = {}) {
    const port = opts.port || 0;
    const logFile = opts.logFile || null;
    const callLog = [];
    const sseClients = [];

    const server = http.createServer((req, res) => {
        const url = new URL(req.url, `http://localhost:${port}`);

        // ── GET /health ─────────────────────────────────────────────
        if (url.pathname === '/health') {
            res.writeHead(200, { 'Content-Type': 'application/json' });
            res.end(JSON.stringify({
                ok: true, hostId: 'mock', computerId: 'mock',
                version: '0.0.0', busy: false, environment: 'test',
            }));
            return;
        }

        // ── POST /api/<method> ──────────────────────────────────────
        if (url.pathname.startsWith('/api/')) {
            const method = url.pathname.slice(5);
            let body = '';
            req.on('data', (c) => (body += c));
            req.on('end', () => {
                let parsed = {};
                try { parsed = JSON.parse(body || '{}'); } catch {}
                callLog.push({ method, body: parsed, ts: Date.now() });
                if (logFile) {
                    try { fs.writeFileSync(logFile, JSON.stringify(callLog, null, 2)); } catch {}
                }
                res.writeHead(200, { 'Content-Type': 'application/json' });
                res.end(JSON.stringify({ ok: true, result: {} }));
            });
            return;
        }

        // ── GET /events (SSE) ───────────────────────────────────────
        if (url.pathname === '/events') {
            const since = parseInt(url.searchParams.get('since') || '0', 10);
            res.writeHead(200, {
                'Content-Type': 'text/event-stream',
                'Cache-Control': 'no-cache',
                'Connection': 'keep-alive',
            });
            // Send hello immediately so hookd's projection comes alive.
            res.write(`data: ${JSON.stringify({ type: 'hello', rev: 1, hostId: 'mock' })}\n\n`);
            sseClients.push({ res, since });
            req.on('close', () => {
                const i = sseClients.findIndex((c) => c.res === res);
                if (i >= 0) sseClients.splice(i, 1);
            });
            return;
        }

        // ── POST /__control ─────────────────────────────────────────
        if (url.pathname === '/__control') {
            let body = '';
            req.on('data', (c) => (body += c));
            req.on('end', () => {
                let cmd = {};
                try { cmd = JSON.parse(body || '{}'); } catch {}
                if (cmd.action === 'push') {
                    const data = `data: ${JSON.stringify(cmd.event)}\n\n`;
                    for (const c of sseClients) c.res.write(data);
                } else if (cmd.action === 'clearLog') {
                    callLog.length = 0;
                    if (logFile) {
                        try { fs.writeFileSync(logFile, '[]'); } catch {}
                    }
                } else if (cmd.action === 'closeAll') {
                    for (const c of sseClients) c.res.end();
                    sseClients.length = 0;
                }
                res.writeHead(200, { 'Content-Type': 'application/json' });
                res.end(JSON.stringify({ ok: true }));
            });
            return;
        }

        // ── GET /__calls ────────────────────────────────────────────
        if (url.pathname === '/__calls') {
            res.writeHead(200, { 'Content-Type': 'application/json' });
            res.end(JSON.stringify(callLog));
            return;
        }

        res.writeHead(404);
        res.end();
    });

    return {
        server,
        callLog,
        sseClients,
        listen: () => new Promise((r) => server.listen(port, '127.0.0.1', r)),
        close: () => new Promise((r) => server.close(r)),
        pushEvent: (event) => {
            const data = `data: ${JSON.stringify(event)}\n\n`;
            for (const c of sseClients) c.res.write(data);
        },
        clearLog: () => { callLog.length = 0; },
        getCalls: () => [...callLog],
        closeAll: () => {
            for (const c of sseClients) c.res.end();
            sseClients.length = 0;
        },
    };
}

module.exports = { createMockHost };

if (require.main === module) {
    const port = parseInt(process.env.MOCK_PORT || '19223', 10);
    const mock = createMockHost({ port });
    mock.listen().then(() => console.log(`mock-codync on :${port}`));
}
