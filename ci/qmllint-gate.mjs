#!/usr/bin/env node
// ci/qmllint-gate.mjs — qmllint as a gate, not a vibe.
//
// Two traps this exists to close:
//
//   1. `qmllint` on PATH is often Qt5's (/usr/bin/qmllint, qt5-declarative).
//      On any Quickshell file it exits 255 with zero output, which reads as
//      "lint is broken on this box" and lets the gate pass by doing nothing.
//      So the Qt6 linter is pinned by path first.
//   2. Linting the tree in one shot drowns in legacy warnings (unqualified
//      access, unused imports) that predate the Meeko phases. Failing on all
//      of them makes the gate unrunnable; ignoring them hides new ones. So
//      warnings are capped by a committed baseline and *errors always fail*,
//      whatever the baseline says. A qmllint "syntax" finding counts as an
//      error even though Qt reports it with warning severity.
//   3. A warning count is a property of the *environment*, not just the tree:
//      Qt 6.12 with the Quickshell module installed reports ~6.9k warnings for
//      this repo, Qt 6.4 without it reports ~65k (every `import Quickshell`
//      resolves to nothing). One number for both is a ratchet that only ever
//      fails, so the baseline is keyed by an environment signature —
//      <qt-version>-quickshell | -noquickshell [-text]. An environment with no
//      recorded baseline fails loudly rather than silently passing or
//      silently failing.
//
// Usage:
//   node ci/qmllint-gate.mjs                   gate (warnings ≤ baseline, 0 errors)
//   node ci/qmllint-gate.mjs --update-baseline record this environment's number
//   node ci/qmllint-gate.mjs path/to/File.qml lint only these files (still gated)
//   MEEKO_QMLLINT_MEASURE=1 …                  report the count, gate nothing
//                                              (how a new environment's baseline
//                                              is bootstrapped in CI)
//   MEEKO_QMLLINT_NOJSON=1 …                   force the Qt ≤6.4 text parser
//
// SPDX-License-Identifier: GPL-3.0-only

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';

const REPO_ROOT = path.resolve(import.meta.dirname, '..');
const BASELINE_FILE = path.join(REPO_ROOT, 'ci/qmllint-baseline.txt');
const CHUNK = 60; // files per qmllint invocation

// ── resolve the linter ─────────────────────────────────────────────
const CANDIDATES = [
    process.env.MEEKO_QMLLINT,
    '/usr/lib/qt6/bin/qmllint',   // Arch / Debian qt6-declarative
    '/usr/lib64/qt6/bin/qmllint', // Fedora / openSUSE
    '/opt/qt6/bin/qmllint',
    spawnSync('command', ['-v', 'qmllint'], { shell: true }).stdout?.toString().trim(),
].filter(Boolean);

const qmllint = CANDIDATES.find((p) => {
    try { return fs.statSync(p).isFile(); } catch { return false; }
});

if (!qmllint) {
    console.error('ci/qmllint-gate: no qmllint found — install qt6-declarative');
    process.exit(1);
}

const version = spawnSync(qmllint, ['--version'], { encoding: 'utf8' }).stdout?.trim() ?? '';
if (/\b5\./.test(version)) {
    // Qt5 linter: silent 255 on Quickshell files. Better a hard stop than a
    // green gate that linted nothing.
    console.error(`ci/qmllint-gate: ${qmllint} is ${version} — the Qt6 linter is required`);
    console.error('  set MEEKO_QMLLINT=/usr/lib/qt6/bin/qmllint or install qt6-declarative');
    process.exit(1);
}

// Qt 6.4 and older (Ubuntu 24.04's qt6-declarative-dev-tools) predate
// `--json`; fall back to parsing the text report so the gate behaves the
// same on a dev box and in CI.
const help = (spawnSync(qmllint, ['--help'], { encoding: 'utf8' }).stdout ?? '')
    + (spawnSync(qmllint, ['--help'], { encoding: 'utf8' }).stderr ?? '');
const HAS_JSON = help.includes('--json') && !process.env.MEEKO_QMLLINT_NOJSON;

// ── environment signature ──────────────────────────────────────────
// Warning counts move with the linter version *and* with whether the
// Quickshell module resolves; both have to be part of the baseline key or
// the ratchet is measuring a different world than it enforces.
const qtVersion = (version.match(/(\d+\.\d+\.\d+)/) ?? ['unknown'])[1];
const QS_MODULE_DIRS = [
    '/usr/lib/qt6/qml/Quickshell',
    '/usr/lib64/qt6/qml/Quickshell',
    '/usr/share/qt6/qml/Quickshell',
    '/opt/qt6/qml/Quickshell',
    ...String(process.env.QML_IMPORT_PATH ?? '').split(':').map((d) => path.join(d, 'Quickshell')),
];
const hasQuickshell = QS_MODULE_DIRS.some((d) => {
    try { return fs.statSync(d).isDirectory(); } catch { return false; }
});
const SIGNATURE = `${qtVersion}-${hasQuickshell ? 'quickshell' : 'noquickshell'}${HAS_JSON ? '' : '-text'}`;


// ── file list ──────────────────────────────────────────────────────
const args = process.argv.slice(2);
const updateBaseline = args.includes('--update-baseline');
const explicit = args.filter((a) => !a.startsWith('--'));

let files = explicit;
if (files.length === 0) {
    const ls = spawnSync('git', ['ls-files', '*.qml'], { cwd: REPO_ROOT, encoding: 'utf8' });
    files = ls.stdout.split('\n').filter(Boolean);
}
if (files.length === 0) {
    console.error('ci/qmllint-gate: no .qml files to lint');
    process.exit(1);
}

// ── lint ───────────────────────────────────────────────────────────
// Returns { errors, warnings, perFile } for one batch of files.
function lintBatch(batch) {
    const r = HAS_JSON
        ? spawnSync(qmllint, ['--ignore-settings', '--json', '-', ...batch], {
            cwd: REPO_ROOT, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024,
        })
        : spawnSync(qmllint, ['--ignore-settings', ...batch], {
            cwd: REPO_ROOT, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024,
        });
    const text = `${r.stdout ?? ''}${r.stderr ?? ''}`;
    const acc = new Map();

    const bump = (file, isError) => {
        const c = acc.get(file) ?? { errors: 0, warnings: 0 };
        if (isError) c.errors++; else c.warnings++;
        acc.set(file, c);
    };

    // A syntax failure is reported with severity *warning* (the ladder has no
    // hard-error bucket for it), so it has to be promoted by id — otherwise a
    // broken file hides under the warning baseline.
    const promote = (id, severity) => severity === 'error' || severity === 'Error' || id === 'syntax';

    if (HAS_JSON) {
        const out = r.stdout ?? '';
        const start = out.indexOf('{');
        if (start < 0) {
            console.error(`ci/qmllint-gate: qmllint produced no JSON (exit ${r.status})`);
            console.error(text.slice(0, 2000));
            process.exit(1);
        }
        let report;
        try {
            report = JSON.parse(out.slice(start));
        } catch (e) {
            console.error(`ci/qmllint-gate: unparseable JSON: ${e.message}`);
            process.exit(1);
        }
        for (const f of report.files ?? []) {
            for (const w of f.warnings ?? []) bump(f.filename, promote(w.id, w.type));
        }
    } else {
        // "Warning: file.qml:12:11: Expected token `}' [syntax]"
        const line = /^(\w+):\s*(.+?):(\d+):(\d+):\s*(.*?)\s*\[([A-Za-z-]+)\]\s*$/;
        for (const l of text.split('\n')) {
            const m = l.match(line);
            if (!m) continue;
            bump(m[2], promote(m[6], m[1]));
        }
    }
    return acc;
}

const perFile = new Map();
let errors = 0;
let warnings = 0;

for (let i = 0; i < files.length; i += CHUNK) {
    const acc = lintBatch(files.slice(i, i + CHUNK));
    for (const [f, c] of acc) {
        const seen = perFile.get(f) ?? { errors: 0, warnings: 0 };
        seen.errors += c.errors;
        seen.warnings += c.warnings;
        perFile.set(f, seen);
    }
}

for (const [, c] of perFile) {
    errors += c.errors;
    warnings += c.warnings;
}

// ── verdict ────────────────────────────────────────────────────────
const sorted = [...perFile.entries()].sort(
    (a, b) => b[1].errors - a[1].errors || b[1].warnings - a[1].warnings,
);

const measureOnly = process.env.MEEKO_QMLLINT_MEASURE === '1';

console.log(`qmllint: ${files.length} files · ${errors} errors · ${warnings} warnings (${qmllint} ${version.replace('qmllint ', '')})`);
console.log(`env: ${SIGNATURE} · baseline file: ci/qmllint-baseline.txt`);

// `<signature>=<warnings>` per line; `#` lines are free-form notes.
function readBaseline() {
    const map = new Map();
    try {
        for (const l of fs.readFileSync(BASELINE_FILE, 'utf8').split('\n')) {
            const m = l.match(/^([A-Za-z0-9._-]+)\s*=\s*(\d+)\s*$/);
            if (m) map.set(m[1], Number(m[2]));
        }
    } catch { /* absent until --update-baseline */ }
    return map;
}

const baselineMap = readBaseline();

if (updateBaseline) {
    const previous = baselineMap.get(SIGNATURE);
    baselineMap.set(SIGNATURE, warnings);
    const lines = [...baselineMap.entries()].sort().map(([k, v]) => `${k}=${v}`);
    lines.push('# measured per environment: warning counts are not portable between',
        '# qmllint versions or across Quickshell module availability.',
        '# Bootstrap a new environment with MEEKO_QMLLINT_MEASURE=1 in CI.',
        '# Errors (incl. qmllint "syntax") fail everywhere regardless.');
    fs.writeFileSync(BASELINE_FILE, lines.join('\n') + '\n');
    console.log(`baseline ${previous === undefined ? 'created' : 'updated'}: ${SIGNATURE}=${warnings}${previous !== undefined && previous !== warnings ? ` (was ${previous})` : ''}`);
}

if (measureOnly) {
    console.log(`measure: ${SIGNATURE}=${warnings} errors=${errors} (no gate applied)`);
    process.exit(0);
}

const baseline = baselineMap.get(SIGNATURE);
if (baseline === undefined) {
    console.error(`\nqmllint gate: no baseline for this environment (${SIGNATURE}).`);
    console.error('  warning counts differ by Qt version and by whether the Quickshell module');
    console.error('  resolves, so an unrecorded environment is not allowed to guess:');
    console.error('    measure it  → MEEKO_QMLLINT_MEASURE=1 node ci/qmllint-gate.mjs');
    console.error('    record it   → node ci/qmllint-gate.mjs --update-baseline');
    process.exit(1);
}
const mode = explicit.length ? 'files' : 'tree';

if (errors > 0) {
    console.error(`\nqmllint gate (${mode}): FAIL — ${errors} lint error(s)`);
    for (const [f, c] of sorted.filter(([, c]) => c.errors > 0).slice(0, 20)) {
        console.error(`  ${String(c.errors).padStart(4)} errors  ${f}`);
    }
    process.exit(1);
}

if (warnings > baseline) {
    console.error(`\nqmllint gate (${mode}): FAIL — ${warnings} warnings > baseline ${baseline}`);
    console.error('  newest offenders (fix, or re-baseline deliberately with --update-baseline):');
    for (const [f, c] of sorted.slice(0, 20)) {
        console.error(`  ${String(c.warnings).padStart(4)} warnings  ${f}`);
    }
    process.exit(1);
}

console.log(`qmllint gate (${mode}): PASS — 0 errors, ${warnings} warnings ≤ baseline ${baseline}`);
process.exit(0);
