---
name: zotero-cli
description: Use whenever the user asks for Zotero, zcli, local paper-library access, paper discovery, paper triage, reading context, paper import plans, X/community discussion around a paper, notes, annotations, collections, tags, recent reading, llm-for-zotero recaps, or dry-run-first Zotero writes. Prefer this skill before generic web search for the user's Zotero/library workflows.
---

# Zotero CLI

Use `zcli` as the direct capability layer for Zotero-backed research work. Do not route through MCP, an adapter API, the helper HTTP endpoint, or generic filesystem exploration when `zcli` exposes the needed state.

## Operating Rules

- Prefer `--format json` for agent work.
- Core reads are local and safe. Imports, writes, and queue/tag handoffs are dry-run-first.
- Use `--execute` only when the user explicitly asked to perform the write/import in the current turn.
- Never call the Zotero helper plugin endpoint directly. For real writes, check `zcli helper doctor --format json`, then use `zcli write ... --execute` or `zcli import ... --execute`.
- If a command returns a Zotero item key after a write/import, verify important metadata with `zcli item get ITEMKEY --format json`.

## Route By Intent

Paper identity:

```bash
zcli resolve "title / citation key / DOI / arXiv / URL / filename" --format json
zcli find paper "agent memory" --format json
```

One-paper reading surface:

```bash
zcli paper ITEMKEY --format json
zcli context ITEMKEY --budget 40k --format json
zcli item markdown ITEMKEY --format text
zcli item annotations ITEMKEY --format json
zcli item notes ITEMKEY --format json
```

Local search:

```bash
zcli index status --format json
zcli index search "agentic rl survey" --format json
zcli index chunks "credit assignment" --item ITEMKEY --format json
zcli index chunk CHUNK_ID --format json
```

Use `index search` for paper candidates and `index chunks` for passage evidence. Page labels are best-effort; missing page labels mean the source text has no reliable marker.

## Discovery And Intake

Use `inbox` as the unified paper intake surface when the user wants “papers to read”, “recent papers”, “triage”, or cross-source discovery. It returns `paper_candidate/v1`.

```bash
zcli inbox status --format json
zcli inbox fetch "coding agent harness memory" --source alphaxiv --days 30 --date-field any --dry-run --format json
zcli inbox fetch --source huggingface --days 3 --date-field any --limit 20 --dry-run --format json
zcli inbox triage "agent memory" --source huggingface --days 30 --code-overview --dry-run --format json
zcli inbox fetch "agent paper arxiv" --source x --days 7 --dry-run --format json
```

Read these fields first:

- `triage`: read-now / skim / watch lane and reasons.
- `context_match`: whether it matches recent Zotero items or reading queue.
- `time`: source-specific first-seen / published / updated fields and semantics.
- `signals`: platform metrics, links, resources, and GitHub/project hints.
- `workflow`: next commands for import preview, queue, tagging, and reading context.
- `zotero_plan`: dry-run import command when an arXiv/importable identifier exists.

For alphaXiv-only work, `zcli alphaxiv brief QUERY --days N --date-field any --format json` is still the strongest alphaXiv-specific triage command. For broad multi-source work, prefer `zcli inbox ...`.

## X Discussion Around A Paper

When the user asks what the author, a paper account, or the X community said about one paper, use:

```bash
zcli inbox discussion "paper title or arXiv id" --format json
zcli inbox discussion 2604.04979 --tweet https://x.com/author/status/123 --reply-limit 80 --format json
zcli inbox discussion "paper title" --handle author_or_curator --days 30 --format json
```

Read `announcement_posts` first, then `discussion_items`. Prioritize labels:

- `question`
- `possible_author_answer`
- `limitation_or_failure`
- `benchmark_or_comparison`
- `implementation_or_data`

The command is read-only. Quote repost coverage is best-effort through X search. Empty results are still useful because output includes the generated `search.queries`.

Manage curated X paper accounts:

```bash
zcli inbox sources x list --format json
zcli inbox sources x add HANDLE --format json
zcli inbox sources x remove HANDLE --format json
```

## Import And Write Safety

Preview imports:

```bash
zcli import arxiv 2604.06240 --dry-run --format json
zcli import ids 10.1145/1234567.1234568 --dry-run --format json
zcli import pdf ./paper.pdf --dry-run --format json
zcli import url https://arxiv.org/abs/2604.06240 --dry-run --format json
```

Preview writes:

```bash
zcli write tags ITEMKEY --add review --dry-run --format json
zcli write note ITEMKEY --content "reading note" --dry-run --format json
zcli write attach ITEMKEY ./paper.pdf --mode link --dry-run --format json
zcli write collection ITEMKEY --collection COLLECTIONKEY --action add --dry-run --format json
```

For inbox candidates, follow `zotero_plan.dry_run_commands[0]` first. Do not import directly from alphaXiv/Hugging Face/X without going through the returned zcli import preview.

## Recaps And llm-for-zotero

Reading history:

```bash
zcli recap reading --from 2026-04-01 --to 2026-04-25 --format json
zcli recap reading --no-lfz --from 2026-04-01 --to 2026-04-25 --format json
```

llm-for-zotero:

```bash
zcli lfz doctor --format json
zcli recap lfz --from today --to today --limit 8 --format json
zcli recap lfz --item ITEMKEY --from today --to today --limit 8 --format json
zcli lfz turn MESSAGE_REF --format json
```

Treat `recap lfz` as a compact index. Expand one turn through `turn_command` or `zcli lfz turn MESSAGE_REF`; do not request trace payloads or runtime internals.

## Setup And Distribution

Check local state:

```bash
zcli doctor --format json
zcli config status --format text
zcli skill doctor --format json
```

Install or refresh the skill:

```bash
zcli skill install --target codex --format json
zcli skill install --target claude --format json
zcli skill install --target hermes --format json
zcli skill install --target lfz --format json
```

If `zcli` appears stale, compare `command -v zcli` with the repo build and rebuild the release binary when the global path points to `target/release/zcli`.
