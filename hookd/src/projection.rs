// ╭──────────────────────────────────────────────────────────────────────────╮
// │   P R O J E C T I O N                                                    │
// │   meeko-hookd · memory-only projection of the Codync host (spec §5)      │
// │                                                                          │
// │   Nothing at rest. Caps: approvals ≤ 10, turns ≤ 50/agent FIFO.         │
// │   Overflow: drop oldest, bump dropCount (surfaced in snapshot).          │
// │   GPL-3.0-only                                                           │
// ╰──────────────────────────────────────────────────────────────────────────╯

use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

pub const MAX_APPROVALS: usize = 10;
pub const MAX_TURNS_PER_AGENT: usize = 50;
/// Rev gap larger than this ⇒ drop projection, full resync (spec §6.3).
pub const MAX_REV_GAP: i64 = 5000;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub status: String,
    pub last_ts: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    /// Pointer only, no message body (spec §5 — host-only data is never cached).
    pub id: String,
    pub agent_id: String,
    pub preview: String,
    pub unread: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub entry_id: String,
    pub agent_id: String,
    pub question: String,
    pub options: Vec<ApprovalOption>,
    pub ts: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub provider: String,
    pub used: f64,
    pub limit: f64,
    pub resets_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum LinkState {
    Online,
    Offline,
    Degraded,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub state: LinkState,
    pub rev: i64,
    pub host_version: String,
}

/// A change the server pushes to connected clients (spec §6.1 pushed set).
#[derive(Debug, Clone)]
#[allow(dead_code)] // full pushed vocabulary per spec §6.1; consumers land in Phase 3
pub enum Push {
    Snapshot,
    AgentUpsert(Agent),
    AgentRemove { id: String },
    TurnAdd(Turn),
    ApprovalAdd(Approval),
    ApprovalResolve { entry_id: String },
    Usage(Usage),
    Link(Link),
}

impl Push {
    /// NDJSON line for the wire. Pushes never carry token or entry bodies.
    pub fn to_line(&self) -> String {
        match self {
            Push::Snapshot => serde_json::json!({"type": "snapshot"}).to_string(),
            Push::AgentUpsert(a) => {
                serde_json::json!({"type": "agent.upsert", "agent": a}).to_string()
            }
            Push::AgentRemove { id } => {
                serde_json::json!({"type": "agent.remove", "id": id}).to_string()
            }
            Push::TurnAdd(t) => serde_json::json!({"type": "turn.add", "turn": t}).to_string(),
            Push::ApprovalAdd(a) => {
                serde_json::json!({"type": "approval.add", "approval": a}).to_string()
            }
            Push::ApprovalResolve { entry_id } => {
                serde_json::json!({"type": "approval.resolve", "entryId": entry_id}).to_string()
            }
            Push::Usage(u) => serde_json::json!({"type": "usage", "usage": u}).to_string(),
            Push::Link(l) => serde_json::json!({"type": "link", "link": l}).to_string(),
        }
    }
}

#[derive(Default)]
pub struct Projection {
    pub rev: i64,
    pub link: Link,
    agents: HashMap<String, Agent>,
    turns: HashMap<String, VecDeque<Turn>>,
    approvals: VecDeque<Approval>,
    usage: Vec<Usage>,
    pub drop_count: u64,
}

impl Default for Link {
    fn default() -> Self {
        Link {
            state: LinkState::Offline,
            rev: 0,
            host_version: String::new(),
        }
    }
}

impl Projection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot object for `hello` / connect-push (spec §5 entity map).
    pub fn snapshot(&self) -> Value {
        serde_json::json!({
            "type": "snapshot",
            "rev": self.rev,
            "link": self.link,
            "agents": self.agents.values().collect::<Vec<_>>(),
            "turns": self.turns,
            "approvals": self.approvals.iter().collect::<Vec<_>>(),
            "usage": self.usage,
            "dropCount": self.drop_count,
        })
    }

    pub fn approvals(&self) -> Vec<&Approval> {
        self.approvals.iter().collect()
    }

    pub fn agents(&self) -> Vec<&Agent> {
        self.agents.values().collect()
    }

    pub fn agent_exists(&self, id: &str) -> bool {
        self.agents.contains_key(id)
    }

    pub fn pending_approval(&self, entry_id: &str) -> Option<&Approval> {
        self.approvals.iter().find(|a| a.entry_id == entry_id)
    }

    /// Local gate for respondApproval's optionId: the entry must be pending,
    /// and a present option must exist in its projected options. Absent
    /// option (None) passes only for an existing entry — the host treats a
    /// missing option as its own default. Anything else is rejected before
    /// any host call (spec §12).
    pub fn option_is_pending(&self, entry_id: &str, option_id: Option<&str>) -> bool {
        let Some(approval) = self.pending_approval(entry_id) else {
            return false;
        };
        match option_id {
            None => true,
            Some(id) => approval.options.iter().any(|o| o.id == id),
        }
    }

    pub fn set_link(&mut self, state: LinkState) -> Push {
        self.link.state = state;
        Push::Link(self.link.clone())
    }

    /// Apply one host event. Returns pushes to broadcast, or None if the
    /// event was dropped (undecodable ⇒ silently dropped, spec §5).
    pub fn apply(&mut self, event: &Value) -> Vec<Push> {
        let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
        let mut pushes = Vec::new();
        match kind {
            "hello" => {
                self.rev = event.get("rev").and_then(Value::as_i64).unwrap_or(self.rev);
                self.link.rev = self.rev;
                self.link.host_version = event
                    .get("hostId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if self.link.state != LinkState::Online {
                    self.link.state = LinkState::Online;
                }
                pushes.push(Push::Link(self.link.clone()));
            }
            "bot" => {
                if let Some(bot) = event.get("bot") {
                    if let Some(agent) = parse_agent(bot) {
                        self.agents.insert(agent.id.clone(), agent.clone());
                        pushes.push(Push::AgentUpsert(agent));
                    }
                }
            }
            "entry" => {
                if let Some(entry) = event.get("entry") {
                    if let Some(turn) = parse_turn(entry) {
                        let q = self.turns.entry(turn.agent_id.clone()).or_default();
                        q.push_back(turn.clone());
                        while q.len() > MAX_TURNS_PER_AGENT {
                            q.pop_front();
                            self.drop_count += 1;
                        }
                        pushes.push(Push::TurnAdd(turn));
                    }
                }
            }
            "accessRequests" => {
                pushes.extend(self.apply_access_requests(event));
            }
            "usage" => {
                if let Some(usages) = event.get("usage").and_then(Value::as_array) {
                    self.usage = usages.iter().filter_map(parse_usage).collect();
                    for u in &self.usage {
                        pushes.push(Push::Usage(u.clone()));
                    }
                }
            }
            "screen" | "cloud" => { /* host-only surfaces, not in the v0 map */ }
            "resync" => { /* handled by the host loop, not the projection */ }
            _ => {
                // Undecodable by this build (host newer): drop silently.
            }
        }
        // Keep rev monotonic from any event carrying one.
        if let Some(r) = event.get("rev").and_then(Value::as_i64) {
            if r > self.rev {
                self.rev = r;
                self.link.rev = self.rev;
            }
        }
        pushes
    }

    /// Reconcile the pending-approval set from an accessRequests snapshot:
    /// entries present are (re)added, entries gone are resolved.
    fn apply_access_requests(&mut self, event: &Value) -> Vec<Push> {
        let mut pushes = Vec::new();
        let incoming: Vec<Approval> = event
            .get("requests")
            .and_then(Value::as_array)
            .map(|rs| rs.iter().filter_map(parse_approval).collect())
            .unwrap_or_default();

        // Resolve anything no longer pending upstream.
        let gone: Vec<String> = self
            .approvals
            .iter()
            .filter(|cur| !incoming.iter().any(|n| n.entry_id == cur.entry_id))
            .map(|a| a.entry_id.clone())
            .collect();
        for entry_id in gone {
            self.approvals.retain(|a| a.entry_id != entry_id);
            pushes.push(Push::ApprovalResolve { entry_id });
        }

        for a in incoming {
            if !self.approvals.iter().any(|n| n.entry_id == a.entry_id) {
                self.approvals.push_back(a.clone());
                while self.approvals.len() > MAX_APPROVALS {
                    self.approvals.pop_front();
                    self.drop_count += 1;
                }
                pushes.push(Push::ApprovalAdd(a));
            }
        }
        pushes
    }

    /// Full resync: drop everything (rev gap > cap or host resync, spec §6.3).
    pub fn reset(&mut self) {
        self.agents.clear();
        self.turns.clear();
        self.approvals.clear();
        self.usage.clear();
        self.rev = 0;
        self.link.rev = 0;
    }
}

fn str_at(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn parse_agent(bot: &Value) -> Option<Agent> {
    let id = str_at(bot, "id");
    if id.is_empty() {
        return None;
    }
    Some(Agent {
        name: str_at(bot, "name"),
        status: str_at(bot, "status"),
        last_ts: str_at(bot, "lastTs"),
        id,
    })
}

fn parse_turn(entry: &Value) -> Option<Turn> {
    let id = str_at(entry, "id");
    let agent_id = str_at(entry, "botId");
    if id.is_empty() || agent_id.is_empty() {
        return None;
    }
    // Preview: first line, truncated. The body is host-only (spec §5).
    let preview = str_at(entry, "preview");
    let preview: String = preview
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(120)
        .collect();
    Some(Turn {
        id,
        agent_id,
        preview,
        unread: entry
            .get("unread")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn parse_approval(req: &Value) -> Option<Approval> {
    let entry_id = str_at(req, "entryId");
    if entry_id.is_empty() {
        return None;
    }
    let options = req
        .get("options")
        .and_then(Value::as_array)
        .map(|os| {
            os.iter()
                .filter_map(|o| {
                    let id = str_at(o, "id");
                    if id.is_empty() {
                        None
                    } else {
                        Some(ApprovalOption {
                            label: str_at(o, "label"),
                            id,
                        })
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Approval {
        agent_id: str_at(req, "botId"),
        question: str_at(req, "question"),
        ts: str_at(req, "ts"),
        entry_id,
        options,
    })
}

fn parse_usage(u: &Value) -> Option<Usage> {
    Some(Usage {
        provider: str_at(u, "provider"),
        used: u.get("used").and_then(Value::as_f64).unwrap_or(0.0),
        limit: u.get("limit").and_then(Value::as_f64).unwrap_or(0.0),
        resets_at: str_at(u, "resetsAt"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn approvals_fifo_cap_drops_oldest_and_counts() {
        // accessRequests is a SNAPSHOT reconciliation: each event lists every
        // pending request. Model 12 simultaneously-pending approvals.
        let mut p = Projection::new();
        let all: Vec<Value> = (0..12)
            .map(|i| {
                serde_json::json!({
                    "entryId": format!("e{i}"), "botId": "b", "question": "q",
                    "options": [{"id": "yes", "label": "Yes"}], "ts": "t"
                })
            })
            .collect();
        p.apply(&serde_json::json!({"type": "accessRequests", "requests": all}));
        assert_eq!(p.approvals().len(), MAX_APPROVALS);
        // oldest (e0, e1) dropped, newest kept
        assert!(p.pending_approval("e0").is_none());
        assert!(p.pending_approval("e11").is_some());
        assert_eq!(p.drop_count, 2);
    }

    #[test]
    fn access_requests_snapshot_reconciles_not_accumulates() {
        // A later snapshot without an old entry resolves it (never accumulates).
        let mut p = Projection::new();
        p.apply(&serde_json::json!({"type": "accessRequests", "requests": [
            serde_json::json!({"entryId": "a", "botId": "b", "question": "q", "options": []}),
            serde_json::json!({"entryId": "b1", "botId": "b", "question": "q", "options": []}),
        ]}));
        assert_eq!(p.approvals().len(), 2);
        p.apply(&serde_json::json!({"type": "accessRequests", "requests": [
            serde_json::json!({"entryId": "b1", "botId": "b", "question": "q", "options": []}),
        ]}));
        assert!(p.pending_approval("a").is_none());
        assert_eq!(p.approvals().len(), 1);
    }

    #[test]
    fn turns_fifo_cap_per_agent() {
        let mut p = Projection::new();
        for i in 0..55 {
            p.apply(&json!({"type": "entry", "rev": i, "entry": {
                "id": format!("t{i}"), "botId": "b1", "preview": "hi"
            }}));
        }
        assert_eq!(p.drop_count, 5);
        assert_eq!(p.rev, 54);
    }

    #[test]
    fn turn_preview_is_truncated_no_body() {
        let mut p = Projection::new();
        p.apply(&json!({"type": "entry", "entry": {
            "id": "t1", "botId": "b", "preview": "line one\nsecret second line", "body": "FULL BODY"
        }}));
        let snap = p.snapshot();
        let turns = snap["turns"].as_object().unwrap();
        let t = &turns["b"][0];
        assert_eq!(t["preview"], "line one");
        // body must never be projected
        assert!(t.get("body").is_none());
    }

    #[test]
    fn undecodable_events_silently_dropped() {
        let mut p = Projection::new();
        assert!(p.apply(&json!({"type": "telepathy"})).is_empty());
        assert!(p.apply(&json!({"no": "type"})).is_empty());
    }

    #[test]
    fn option_gate_membership() {
        let mut p = Projection::new();
        p.apply(&json!({"type": "accessRequests", "requests": [{
            "entryId": "a", "botId": "b", "question": "q",
            "options": [{"id": "yes", "label": "Yes"}, {"id": "no", "label": "No"}]
        }]}));
        // absent option = allowed (host default)
        assert!(p.option_is_pending("a", None));
        // projected members pass
        assert!(p.option_is_pending("a", Some("yes")));
        assert!(p.option_is_pending("a", Some("no")));
        // unknown member rejected
        assert!(!p.option_is_pending("a", Some("maybe")));
        assert!(!p.option_is_pending("a", Some("")));
        // stale entry rejects everything, including None
        assert!(!p.option_is_pending("ghost", Some("yes")));
        assert!(!p.option_is_pending("ghost", None));
    }

    #[test]
    fn reset_clears_everything() {
        let mut p = Projection::new();
        p.apply(&json!({"type": "bot", "bot": {"id": "b", "name": "N"}}));
        p.reset();
        assert!(p.agents().is_empty());
        assert_eq!(p.rev, 0);
    }
}
