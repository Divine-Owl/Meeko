# Meeko — Baseline capture (Phase 0, BUDGET-02 input)

Captured 2026-10-15 on the target host (CachyOS, Hyprland, 8 GB class — `MemTotal` 7328 MB).
Method: `ps` RSS of live processes, no load other than normal desktop use.

## Meeko-scope baseline (what the budget gates measure)

| Process | RSS | Notes |
|---|---|---|
| `qs` (quickshell / impasto shell) | **≈ 68 MB** | the only in-budget process today |

**Baseline = ~68 MB.** Per spec §10: idle gate ≤1.25× baseline ≈ **85 MB** for the
quickshell process once Meeko modules land; hookd stays inside its own 64 MB cap;
sum never exceeds 500 MB. Very large headroom against the ceiling — the gates
should be comfortable if the design holds.

## Out-of-budget context (Codync stack, excluded by spec)

`codync-host` ≈ 12 MB + `codync` main ≈ 196 MB + renderer/helpers ≈ 170 MB
→ **≈ 378 MB total** — the stack Meeko must never double or embed.

## Host pressure note

At capture time: used 6136 / free 281 / available 1191 MB, buff/cache 1518 MB.
The box genuinely runs near its memory ceiling during normal use — this
validates the ≤500 MB hard cap, quiet mode, and zram-option design decisions
as requirements, not preferences.

Feeds QA BUDGET-02 (idle ceiling) as the measured baseline input.
