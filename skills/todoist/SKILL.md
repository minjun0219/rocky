---
name: todoist
description: Use when working with the user's Todoist from a coding session — figuring out what to work on next ("오늘 뭐 하지", "다음 작업 뭐야"), registering or organizing tasks ("todoist에 등록해둬", capturing follow-up work discovered mid-task), or closing tasks after work ships (PR merged, release cut). Cross-references Todoist tasks with git state and the rocky worklog to propose next actions; applies fixed registration conventions (priority semantics, self-contained descriptions, duplicate check) and confirmation gates for writes.
---

# Todoist

Work with the user's Todoist as the task ledger for the current repository: read it to decide
what to do next, write to it under consistent conventions, and close tasks when work ships.

## Tool gate (run first)

rocky ships no Todoist access of its own — no tools, no tokens, no API calls. Use whatever
Todoist MCP tools are connected to the session:

- Find tools whose name contains `todoist` (case-insensitive). If they are deferred, load them
  via ToolSearch in one call.
- Tool name prefixes vary per host / connector — match by name suffix (`find-tasks`,
  `add-tasks`, `complete-tasks`, …), never assume a fixed full name.
- **No Todoist tools connected → stop.** Tell the user no Todoist MCP is available and how to
  connect one (e.g. the claude.ai Todoist connector, or `claude mcp add` with a Todoist MCP
  server). Never fake progress or invent task state.

## Repo ↔ project mapping

Map the current repository to exactly one Todoist project before reading or writing anything:

1. List projects and match the repo directory name / path against project names and
   descriptions. The convention `레포: <path>` in a project description is the strongest
   signal.
2. Exactly one match → use it. Zero or multiple → ask the user which project to use (or
   whether to create one).
3. When a mapping was confirmed non-deterministically, offer to record `레포: <path>` in the
   project description so future sessions match deterministically.

Never read or write tasks in unrelated projects unless the user explicitly asks.

## Workflow 1 — what to work on next

Trigger: the user asks what to do ("오늘 뭐 하지", "다음 작업 뭐야") or opens a session
looking for direction.

Gather three sources, then cross-reference:

1. **Todoist** — open tasks in the mapped project: priority, deadline, description.
2. **git** — unmerged local branches, uncommitted changes, recent commit flow.
3. **worklog** — `worklog_status` plus recent `worklog_read` entries (turns and digests) for
   what was in flight. These tools exist only where the full-surface rocky server is running —
   if absent, skip this source silently; absence is not an error.

Present 2–3 candidates with reasoning (priority, deadline, momentum from git / worklog), and
mark exactly one as recommended.

## Workflow 2 — registering and organizing tasks

Trigger: an explicit request ("todoist에 등록해둬") or follow-up work discovered while
working.

The confirmation gate is situational:

- **Explicitly requested registrations → execute immediately**, then report what was created.
- **Skill-initiated suggestions and edits to existing tasks → show a draft and wait for
  confirmation** before writing.

Before adding, search the mapped project for an existing task covering the same work — if a
duplicate exists, report it instead of adding.

Registration conventions:

| Field | Convention |
| --- | --- |
| content | Concise and actionable — details go in the description, not the title |
| description | Self-contained: paths, procedure, rationale, related files / branches. Another session must be able to start from it alone |
| priority | Fixed semantics — p1 blocker/urgent, p2 next up, p3 ready backlog, p4 idea/on-hold |
| due / deadline | Only when a real date exists — never invent one |

## Workflow 3 — closing tasks

Trigger: work ships (PR merged, release cut) or the user says it is done.

1. Find the task(s) matching the shipped work in the mapped project.
2. Propose completion — name the exact task(s) — and complete only after confirmation.
3. If the work spawned follow-ups, offer them as new task drafts (Workflow 2 gate applies).

## Guardrails

- Completing, editing, or rescheduling existing tasks always goes through confirmation.
  Deletion requires an explicit user request — never propose it proactively.
- Move dates with the reschedule-style tool, not by rewriting the due string — a due-string
  update destroys recurrence patterns.
- No bulk mutations: one project at a time, only the tasks that were confirmed.
- Stay inside the mapped project unless the user explicitly directs otherwise.
