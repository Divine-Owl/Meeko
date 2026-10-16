// ╭──────────────────────────────────────────────────────────────────────────╮
// │                                                                          │
// │   M E E K O   S E R V I C E                                              │
// │   hookd socket client · projection mirror · escape boundary             │
// │                                                                          │
// │   Single writer of QML-side agent-surface state. Owns the socket        │
// │   lifecycle, mirrors hookd's NDJSON push stream into Qt properties,     │
// │   and provides the respondApproval verdict path. No Timer polling —     │
// │   every re-render is driven by the SSE mirror.                          │
// │                                                                          │
// ╰──────────────────────────────────────────────────────────────────────────╯

pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

import "../theme"

// MeekoService is the single QML-side state owner for the agent surface. It
// connects to hookd's 0600 unix socket, mirrors the projection (agents, turns,
// approvals, usage, link), and provides the verdict path. Views (AgentModule,
// AgentPanel, ApprovalCard) are thin readers — they never mutate state.
//
// Connection: native Quickshell Socket (in-process, zero extra resident
// process). `connected: true` begins connecting; clearing it disconnects.
// Reconnect uses a Timer — connection management, not data polling.
Singleton {
    id: root

    // ── LINK STATE ───────────────────────────────────────────────────────────

    property string linkState: "offline"
    property int rev: 0
    property string hostVersion: ""

    // ── PROJECTION MIRROR ───────────────────────────────────────────────────
    // Bounded lists mirroring hookd's §5 entity map. Caps enforced here too
    // (defense-in-depth: hookd is the primary enforcer).

    property var agents: []
    property var approvals: []
    property var usage: []
    property int dropCount: 0

    // turns: map of agentId → array of Turn (for panel display)
    property var turns: ({})

    // ── ESCAPE BOUNDARY ─────────────────────────────────────────────────────
    // Single boundary helper: every host-sourced string escaped exactly once
    // before it enters QML state. CR&S constraint — no double-escaping.

    function escapeDisplay(text: string): string {
        return (text ?? "")
            .replace(/&/g, "&amp;")
            .replace(/</g, "&lt;")
            .replace(/>/g, "&gt;")
            .replace(/"/g, "&quot;")
    }

    // ── SOCKET CONNECTION ───────────────────────────────────────────────────
    // Native Quickshell Socket — in-process, no extra resident process.
    // `connected: true` begins connecting; setting it false disconnects.

    readonly property string socketPath: Quickshell.env("MEEKO_HOOKD_SOCKET") || `${Quickshell.env("XDG_RUNTIME_DIR")}/meeko-hookd.sock`

    property int nextId: 1
    property var pending: ({})

    readonly property Socket socket: Socket {
        connected: true
        path: root.socketPath

        parser: SplitParser {
            splitMarker: "\n"
            onRead: line => root.handleLine(line)
        }
    }

    // Watch connection state — reconnect on disconnect.
    readonly property Connections connectionWatch: Connections {
        target: root.socket

        function onConnectedChanged(): void {
            if (root.socket.connected) {
                root.reconnect.stop()
                root.linkState = "online"
            } else {
                root.linkState = "offline"
                root.reconnect.restart()
            }
        }
    }

    // Reconnect timer — connection management only, never data polling.
    property Timer reconnect: Timer {
        interval: 2000
        repeat: false
        onTriggered: {
            if (root.linkState !== "online") {
                // Toggle connected to force a reconnect
                socket.connected = false
                socket.connected = true
            }
        }
    }

    // ── INCOMING MESSAGES ───────────────────────────────────────────────────
    // Every line from the socket is parsed and applied to the mirror.
    // Two shapes: pushes (have `type`) and responses (have `id` + `ok`).

    function handleLine(line: string): void {
        const trimmed = (line ?? "").trim()
        if (!trimmed)
            return
        try {
            const msg = JSON.parse(trimmed)
            if (msg.type)
                root.applyMessage(msg)
            else if (msg.id !== undefined)
                root.handleResponse(msg)
        } catch (e) {
            // undecodable line: drop silently (spec §5)
        }
    }

    function applyMessage(msg: var): void {
        if (!msg || !msg.type)
            return
        switch (msg.type) {
        case "snapshot":
            root.applySnapshot(msg)
            break
        case "agent.upsert":
            root.applyAgentUpsert(msg.agent)
            break
        case "agent.remove":
            root.applyAgentRemove(msg.id)
            break
        case "turn.add":
            root.applyTurnAdd(msg.turn)
            break
        case "approval.add":
            root.applyApprovalAdd(msg.approval)
            break
        case "approval.resolve":
            root.applyApprovalResolve(msg.entryId)
            break
        case "usage":
            root.applyUsage(msg.usage)
            break
        case "link":
            root.applyLink(msg.link)
            break
        }
    }

    // hookd responses: {id, ok, result|error}. No type field — correlate by id.
    function handleResponse(msg: var): void {
        const id = msg.id
        if (id === undefined || id === null)
            return
        const intent = root.pending[id]
        if (!intent)
            return
        delete root.pending[id]
        if (!msg.ok) {
            const error = msg.error ?? "unknown"
            if (intent.type === "respondApproval")
                root.applyApprovalResolve(intent.entryId)
            console.warn(`MeekoService: ${intent.type} failed: ${error}`)
        }
    }

    function applySnapshot(msg: var): void {
        root.rev = msg.rev ?? 0
        root.dropCount = msg.dropCount ?? 0
        if (msg.link)
            root.applyLink(msg.link)
        root.agents = msg.agents ?? []
        root.approvals = msg.approvals ?? []
        root.usage = msg.usage ?? []
        const t = {}
        const turnsObj = msg.turns ?? {}
        for (const agentId in turnsObj) {
            t[agentId] = turnsObj[agentId]
        }
        root.turns = t
    }

    function applyAgentUpsert(agent: var): void {
        if (!agent || !agent.id)
            return
        const next = root.agents.filter(a => a.id !== agent.id)
        next.push(agent)
        while (next.length > 100) {
            next.shift()
            root.dropCount++
        }
        root.agents = next
    }

    function applyAgentRemove(id: string): void {
        root.agents = root.agents.filter(a => a.id !== id)
    }

    function applyTurnAdd(turn: var): void {
        if (!turn || !turn.id || !turn.agentId)
            return
        const existing = root.turns[turn.agentId] ?? []
        const next = existing.concat([turn])
        while (next.length > 50) {
            next.shift()
            root.dropCount++
        }
        const updated = Object.assign({}, root.turns)
        updated[turn.agentId] = next
        root.turns = updated
    }

    function applyApprovalAdd(approval: var): void {
        if (!approval || !approval.entryId)
            return
        const next = root.approvals.filter(a => a.entryId !== approval.entryId)
        next.push(approval)
        while (next.length > 10) {
            next.shift()
            root.dropCount++
        }
        root.approvals = next
    }

    function applyApprovalResolve(entryId: string): void {
        root.approvals = root.approvals.filter(a => a.entryId !== entryId)
    }

    function applyUsage(u: var): void {
        if (!u)
            return
        const next = root.usage.filter(x => x.provider !== u.provider)
        next.push(u)
        while (next.length > 20) {
            next.shift()
            root.dropCount++
        }
        root.usage = next
    }

    function applyLink(link: var): void {
        if (!link)
            return
        root.linkState = link.state ?? "offline"
        root.rev = link.rev ?? 0
        root.hostVersion = link.hostVersion ?? ""
    }

    // ── OUTGOING VERBS ──────────────────────────────────────────────────────
    // Verdicts go through the socket, never qs ipc (CR&S constraint 1).

    function send(verb: string, params: var): void {
        if (root.linkState !== "online")
            return
        const id = root.nextId++
        root.pending[id] = { type: verb, entryId: params?.entryId }
        const line = JSON.stringify({ id, verb, params: params ?? {} })
        socket.write(`${line}\n`)
        socket.flush()
    }

    function respondApproval(entryId: string, optionId: string): void {
        root.send("respondApproval", {
            entryId: entryId,
            optionId: optionId && optionId.length > 0 ? optionId : null
        })
    }

    function ping(): void {
        root.send("ping", {})
    }
}
