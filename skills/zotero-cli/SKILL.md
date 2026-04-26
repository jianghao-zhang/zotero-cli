# Zotero CLI

Use this skill when working with a user's Zotero library through `zcli`.

## Rules

- Call `zcli` directly. Do not call an MCP server, adapter API, or HTTP bridge for Zotero access.
- Prefer JSON output: pass `--format json` unless the user explicitly wants human-readable text.
- Core Zotero commands are local and read-only by default. Treat any import, mutation, or inbox execution path as dry-run-first.
- For Zotero writes, use `zcli write ... --dry-run` first. Use `--execute` only when the user explicitly asked to perform the change in the current turn. Never call the helper plugin endpoint directly.
- Use `zcli helper doctor --format json` to check the optional Zotero helper plugin before executing local Zotero-runtime writes.
- Treat helper execute results as compact by default. Fetch normal item details with `zcli item get ITEMKEY --format json` when more metadata is needed after a write.
- Do not assume `llm-for-zotero` exists. Use `zcli lfz doctor` before `zcli recap lfz`.
- When the user gives a title, DOI, arXiv ID, URL, or file path instead of a Zotero key, call `zcli resolve QUERY --format json` first.
- Prefer `zcli paper ITEMKEY --format json` for a one-paper work surface, and `zcli context ITEMKEY --budget 40k --format json` when preparing agent context.
- Use `zcli recap reading` for reading activity and metadata. It automatically includes compact llm-for-zotero context when the user enabled lfz in zcli config; pass `--no-lfz` when the user asks for pure reading metadata only. Use `zcli recap lfz` when the user specifically wants llm-for-zotero or Claude Code runtime conversation context.
- Use `zcli item markdown ITEMKEY --format json` when an agent needs a Markdown paper surface. If llm-for-zotero is configured, zcli prefers MinerU `full.md` caches keyed by PDF attachment item id; otherwise it falls back to metadata, notes, annotations, and extracted text.
- Treat `zcli recap lfz` excerpts as summaries unless `text_full_included` is true. Check `text_truncated`, `text_chars`, and `text_excerpt_chars`.
- Do not ask for Claude/runtime trace or event payloads. `zcli` exposes event counts only.
- To expand one specific llm-for-zotero question, use the recap row's `turn_command` or call `zcli lfz turn MESSAGE_REF --format json`. This returns the full question, matching answer, and agent final without trace payloads.

## Common Calls

```bash
zcli doctor --format json
zcli resolve "agent memory" --format json
zcli paper ITEMKEY --format json
zcli context ITEMKEY --budget 40k --format json
zcli search list "agent memory" --format json
zcli item get ITEMKEY --format json
zcli item extract ITEMKEY --format json
zcli item annotations ITEMKEY --format json
zcli item markdown ITEMKEY --format json
zcli item markdown ITEMKEY --format text
zcli collection list --format json
zcli tags list --format json
zcli recent --days 7 --format json
zcli recap reading --from 2026-04-01 --to 2026-04-25 --format json
zcli lfz doctor --format json
zcli lfz turns --item ITEMKEY --format json
zcli recap lfz --from today --to today --format json
zcli recap lfz --item ITEMKEY --from today --to today --format json
zcli lfz turn claude:123 --format json
zcli write tags ITEMKEY --add review --dry-run --format json
zcli write note ITEMKEY --content "reading note" --dry-run --format json
zcli write attach ITEMKEY ./paper.pdf --mode link --dry-run --format json
zcli write rename-attachment ATTACHMENTKEY --name paper.pdf --dry-run --format json
zcli write import-files ./paper.pdf --dry-run --format json
zcli helper doctor --format json
zcli skill doctor --format json
zcli inbox status --format json
```

## Output Use

For paper identity, prefer `key`, `title`, `authors`, `year`, `doi`, `arxiv`, and `url`.

For recap provenance, preserve the exact `provenance` value. `metadata_modified` is only a fallback touched-paper signal, not definite reading.

For llm-for-zotero recaps, follow `message_ref` / `turn_command` instead of asking for large `--full-text` output when only one turn is needed.
