---
name: delegating-to-codex
description: Use when handing a self-contained coding, code-review, or advisory task to a headless OpenAI model (the codex CLI) — running `codex exec` or `codex review`, picking a sandbox / model tier, capturing its output with `-o`, and verifying that output before reporting. Covers the self-contained-prompt rule and the guardrails that keep the delegation safe.
---

# Delegating to Codex

Delegate a **self-contained** task to a headless OpenAI coding model via the `codex` CLI, then
supervise the result. Codex is a peer implementer/reviewer you dispatch — not a pass-through.

**Core principle — the prompt must be self-contained.** Codex runs in its own process and
**cannot see this conversation's context**: no chat history, no files you already read, no prior
decisions. Everything it needs — the goal, the repo path, constraints, invariants, done-criteria —
must be in the prompt itself. A prompt that says "finish what we discussed" fails.

**You stay the supervisor.** Never relay Codex's output as truth. Read its final message, then
confirm the real change (`git diff`) or claim yourself before reporting. Report only what you
verified; say so plainly when it failed or is incomplete.

## Prerequisites
- `codex` on PATH — `which codex && codex --version` (if missing, stop and tell the user to install it; do not fake the delegation).
- Logged in — Codex uses the user's existing auth (ChatGPT account or API key). Auth failures surface at call time.

## Invocation pattern (`codex exec`)

```bash
codex exec "<self-contained prompt>" \
  -C /abs/path/to/repo \        # cwd + write scope (writes stay inside -C)
  -s workspace-write \          # default; use read-only for analysis/review
  -o /abs/path/to/out.txt       # capture the final message for you to read back
# -m <model> optional — omit to use the account's configured default (always auth-valid)
```

| Flag | Meaning | Default choice |
| :-- | :-- | :-- |
| `-C, --cd` | working dir + write boundary | the repo (or an isolated worktree) |
| `-s, --sandbox` | `read-only` / `workspace-write` / `danger-full-access` | `workspace-write` for edits, `read-only` for review/advice |
| `-o, --output-last-message` | write final answer to a file | always set it, then Read the file |
| `-m, --model` | model tier | **omit** unless you deliberately want another tier |
| `--json` | stream JSONL events to stdout | when you want to watch progress |
| `--add-dir` | extra writable dir | only if the task genuinely spans it; keep minimal |

## Model selection
Match tier to difficulty: **frontier** for hard reasoning / correctness-critical work, **balanced**
for ordinary tasks, **low-cost** for simple/bulk work. When unsure, **omit `-m`** — Codex uses the
account's configured default, which is always valid for that auth.

**Auth caveat:** on a ChatGPT-account login, a plain latest model name may be rejected
(`model is not supported when using Codex with a ChatGPT account`). If you must pin a model, use a
name the account actually offers rather than guessing — or just omit `-m`.

## Recipes

**Implement** (writes in scope):
```bash
codex exec "In the repo at <path>, do X. Constraints: <invariants>. Do not touch <out-of-scope>.
Make gates pass: <cmd>. Leave changes uncommitted." -C <path> -s workspace-write -o out.txt
```

**Code-review the working tree** — use the `review` subcommand, not a hand-rolled prompt:
```bash
codex review --uncommitted        # staged + unstaged + untracked; also --base <branch> / --commit <sha>
```
Gotcha: **`codex review` has no `-m`** — pin a model with `-c model="<name>"` instead. (`-m` is an
`codex exec` flag; `codex exec review` also accepts `-m`.)

**Advisory / second opinion** — independent cross-check on an approach or design, no edits:
```bash
codex exec "Review this plan and argue where it's wrong: <plan>. Repo at <path> for reference only."
  -C <path> -s read-only -o advice.txt
```

## Guardrails
- **Never** `--dangerously-bypass-approvals-and-sandbox` or `-s danger-full-access`.
- Scope writes with `-C`; widen only via minimal `--add-dir` when truly needed.
- Don't put secrets (keys, tokens, PII) in the prompt unless the task genuinely requires them.
- Read → verify → report. Do not paste Codex's output as the result.

## After it runs
1. Check the exit code; on failure, quote the log and stop — don't merge or claim success.
2. `Read` the `-o` output file (the final message).
3. If it edited files: `git -C <path> status --porcelain` + `git -C <path> diff` — confirm the change is real and in scope.
4. Re-run the repo gates yourself (`bun run check` / `typecheck` / `bun test`) before trusting it.
5. Summarize in one or two lines: what changed, what you verified, and any failure/gap — honestly.

## Relationship to `/rocky:codex`
This skill is the reusable **delegation mechanics + guardrails**. The `/rocky:codex` slash command
is one application of it: dispatch an *implementer* into an isolated git worktree, then supervise
gates + MCP-tool-surface + diff-scope before an approved merge. Reach for the command for that full
worktree-and-merge workflow; reach for this skill for one-off `codex exec` / `codex review` /
advisory delegations that don't need the worktree ceremony.
