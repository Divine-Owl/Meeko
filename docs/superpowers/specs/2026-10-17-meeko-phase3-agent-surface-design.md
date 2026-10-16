# Meeko — Phase 3 Agent Surface Design

Date: 2026-10-17 · Status: draft · Task: MEEKO-005

## 1. Purpose

Surface Codync agents, turns, approvals, and provider usage as first-class UI inside the impasto shell's island + bar. Builds on the hookd projection (§5 of the Phase 1 shell design) and the socket verb gate (§6.1). Coucou is the UX reference (approval-card interaction model) only — its code is not ported.

## 2. Boundaries (approach A, locked in)

| Component | Role | State ownership |
|---|---|---|
| `MeekoService` (singleton, QML) | Socket lifecycle + SSE projection mirror + intent/pending | **Single writer** of all projection state |
| `AgentModule` (bar chip) | Thin view — agent count/status; click → `meeko panel` | Reader only |
| `AgentPanel` (island) | Interactive view — approval cards, turns, usage | Reader only |
| `ApprovalCard` (leaf control) | Respond/reject via `MeekoService.respondApproval`; renders pending/resolved/error | Reader + intent trigger |
| `escapeDisplay` (helper) | Single escape boundary for host-sourced strings | Stateless |

`MeekoService` exposes a Qt-property model mirroring hookd's entity map: `agents` (≤100 FIFO), `turns` (≤50/agent FIFO), `approvals` (≤10 FIFO), `usage` (≤20 FIFO), `linkState`, `rev`, `hostVersion`, `dropCount`. Views never mutate the model directly.

## 3. Data flow

```
hookd socket ←► MeekoService ←► AgentModule / AgentPanel / ApprovalCard
```

- Connect → `hello` → hookd pushes `snapshot` → service mirrors → views re-render
- SSE events: `agent.upsert`, `agent.remove`, `turn.add`, `approval.add`, `approval.resolve`, `usage`, `link`
- Overflow → `dropCount++`, surfaced; never silent shrink
- Preview ≈ 120-char truncated pointer; entry bodies never cached
- `undecodable` → resync; `link.degraded` on unknown shape
- Host down → `link.offline` (5 s debounce), projection frozen, shell == plain impasto
- Reconnect → backoff → re-`hello` → snapshot refresh

## 4. Approval respond path

1. Card clicks respond/reject → `MeekoService.respondApproval(entryId, optionId?)`
2. **Local pre-gate** (before socket write): entryId allowlisted, approval exists in mirror + pending, optionId allowlisted against approval's options; else refuse locally, **no send**
3. Send `respondApproval` (shell role only) → hookd re-gates (exists+pending, else `stale_entry`) → host `respondPermission` → `approval.resolve` push
4. Optimistic intent: service marks pending-dispatch on respond; cleared on `approval.resolve` or `ok:false`; card shows error on failure — never silent drop, never stuck spinner
5. Observer/wrong-role → `peer_not_shell` (envelope `ok:false`). Verdicts are **socket-only**, never `qs ipc` verbs.

## 5. Constraint compliance

| Constraint | Satisfied by |
|---|---|
| No verdict verbs on `qs ipc` | `respondApproval` socket-only; `meeko panel`/`meeko status` are the qs-ipc surface |
| No `Timer` polling | Views render from service state mutated only by SSE pushes (+ 5 s link debounce timer) |
| Display-escape exactly once | `escapeDisplay` at MeekoService boundary; delegates render verbatim |
| Single-writer rule | MeekoService owns all mutation; views are thin readers |
| Human-in-the-loop default | Local pre-gate + hookd server gate; no auto-approve |
| ≤500 MB budget | In-process Quickshell `Socket` — zero extra resident process |

## 6. Transport

Native Quickshell `Socket { path; connected: true }` — in-process, keeps `/proc/self/exe` = quickshell → hookd `Role::Shell`. socat explicitly rejected: it reclassifies the peer as `Observer` and makes every verdict return `peer_not_shell`.

## 7. Wire format (contract)

hookd owns the contract. Serde structs emit camelCase (`entryId`, `agentId`, `lastTs`, `hostVersion`, `resetsAt`). `LinkState` serde `#[serde(rename_all = "lowercase")]` → `"online"`/`"offline"`. Response envelopes `{id, ok, error}` handled via `handleResponse` with pending id→intent correlation. `contract.test.js` asserts camelCase shape.

## 8. QA seams

- Unit (pure): local pre-gate (stale/unknown id, optionId allowlist), escape helper, pending-intent state machine
- Contract: `respondApproval` happy + `stale_entry` + `peer_not_shell` + error envelope; panel states from projection
- E2E smoke: headless Hyprland — panel opens, first-frame content on open

## 9. Open / deferred

- Usage chip placement (bar vs panel), per-provider vs aggregate — v1 decision pending
- Panel content v1: approval cards only vs cards + turns + usage — pending
- Design doc self-review + user review still outstanding (implementation ahead of spec)
