---
name: zotero-cli
description: Use whenever the user asks Codex to use Zotero, zcli, zotero-cli, or the local Zotero library; find, search, read, summarize, compare, cite, or inspect papers; locate papers by topic, title, short title, citation key, DOI, arXiv, URL, or filename; import arXiv/DOI/PDF/URL papers; search paper passages or page-level evidence; read notes, annotations, collections, tags, recent reading, reading recaps, or llm-for-zotero conversations; or perform dry-run-first Zotero writes through zcli.
---

# Zotero CLI

Use this skill when working with a user's Zotero library through `zcli` from Codex or another external-agent runtime.

## Rules

- Call `zcli` directly. Do not call an MCP server, adapter API, or HTTP bridge for Zotero access.
- Prefer JSON output: pass `--format json` unless the user explicitly wants human-readable text.
- Core Zotero commands are local and read-only by default. Treat any import, mutation, or inbox execution path as dry-run-first.
- For Zotero writes or imports, use `zcli write ... --dry-run` or `zcli import ... --dry-run` first. Use `--execute` only when the user explicitly asked to perform the change in the current turn. Never call the helper plugin endpoint directly.
- Use `zcli helper doctor --format json` to check the optional Zotero helper plugin before executing local Zotero-runtime writes.
- Treat helper execute results as compact by default. Fetch normal item details with `zcli item get ITEMKEY --format json` when more metadata is needed after a write.
- For paper imports, prefer `zcli import arxiv ...` for arXiv IDs, `zcli import ids ...` for DOI/ISBN/PMID/ADS identifiers, `zcli import pdf ...` for local or remote PDFs, and `zcli import url ...` for mixed paper URLs. Dry-run output is the duplicate/import plan; execute output should be followed by `zcli item get KEY --format json` for any item the answer depends on.
- arXiv imports use Zotero's native translator first. If Zotero returns no item, the helper can fall back to arXiv Atom metadata and attach the PDF, still through Zotero runtime APIs.
- Do not assume `llm-for-zotero` exists. Use `zcli lfz doctor` before `zcli recap lfz`.
- When the user gives a title, short title, citation key, DOI, arXiv ID, URL, or file path instead of a Zotero key, call `zcli resolve QUERY --format json` first.
- When the user gives a topic-like or fuzzy paper request, call `zcli find paper QUERY --format json` and then use `item.key` from the best hit.
- When repeated broad search is needed, check `zcli index status --format json`; if the index exists, prefer `zcli index search QUERY --format json` for paper candidates before falling back to `zcli find paper`.
- For fuzzy passage search, use `zcli index chunks QUERY --format json`. Add `--item ITEMKEY` for one paper, `--collection NAME` for a folder-like scope, or `--tag TAG` for a tagged slice of the library. If full-paper passages are missing, ask the user to run `zcli index update --include-full-text --format json`.
- Treat `index chunks` results as passage candidates. Use `page`/`page_label` when present, but respect `page_policy`: missing pages mean the source text has no reliable page marker.
- To expand one passage, use the hit's `expand_command` or call `zcli index chunk CHUNK_ID --format json`.
- Prefer `zcli paper ITEMKEY --format json` for a one-paper work surface, and `zcli context ITEMKEY --budget 40k --format json` when preparing agent context.
- For "what did I read recently" or broad date-range recaps, use `zcli recap reading --from DATE --to DATE --format json` first. Treat llm-for-zotero as a bounded overlay, not the primary source.
- `zcli recap reading` automatically includes compact llm-for-zotero hints when the user enabled lfz in zcli config; pass `--no-lfz` when the user asks for pure reading metadata only.
- Use `zcli recap lfz --limit 8 --format json` only when the user specifically asks what they discussed with llm-for-zotero or Claude Code. Add `--item ITEMKEY` whenever the prompt names one paper.
- Use `zcli item markdown ITEMKEY --format json` when an agent needs a Markdown paper surface. If llm-for-zotero is configured, zcli prefers MinerU `full.md` caches keyed by PDF attachment item id; otherwise it falls back to metadata, notes, annotations, and extracted text.
- Treat `zcli recap lfz` as a compact index unless `text_policy` says full text was requested. Check `text_policy`, `expand_policy`, `paper_groups`, `text_truncated`, `text_chars`, and `text_excerpt_chars`.
- Do not use `--details`, `--full-text`, or `--include-contexts` for broad recap prompts. Use those only after the user asks for a specific full turn or context payload.
- Do not ask for Claude/runtime trace or event payloads. `zcli` exposes event counts only.
- To expand one specific llm-for-zotero question, use the recap row's `turn_command` or call `zcli lfz turn MESSAGE_REF --format json`. This returns the full question, matching answer, and agent final without trace payloads.

## Common Calls

```bash
zcli doctor --format json
zcli resolve "agent memory" --format json
zcli find paper "agentic rl survey" --format json
zcli index status --format json
zcli index update --format json
zcli index search "agentic rl survey" --format json
zcli index chunks "credit assignment" --item ITEMKEY --format json
zcli index chunks "context compression" --collection "Agent Papers" --format json
zcli index chunk ITEMKEY:annotation:2 --format json
zcli index get ITEMKEY --format json
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
zcli recap lfz --from today --to today --limit 8 --format json
zcli recap lfz --item ITEMKEY --from today --to today --limit 8 --format json
zcli lfz turn claude:123 --format json
zcli import arxiv 2604.06240 --dry-run --format json
zcli import ids 10.1145/1234567.1234568 --dry-run --format json
zcli import pdf ./paper.pdf --dry-run --format json
zcli import url https://arxiv.org/abs/2604.06240 --dry-run --format json
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

For paper identity, prefer `key`, `citation_key`, `title`, `short_title`, `authors`, `year`, `doi`, `arxiv`, and `url`.

For import results, preserve `status`, `source`, and the returned Zotero `key`. Treat `source: "zotero_translator"` and `source: "arxiv_api_fallback"` as successful Zotero-native imports, but verify important metadata with `zcli item get`.

For recap provenance, preserve the exact `provenance` value. `metadata_modified` is only a fallback touched-paper signal, not definite reading.

For llm-for-zotero recaps, read `paper_groups` first for the topic map. Follow `message_ref` / `turn_command` instead of asking for large `--full-text` output when only one turn is needed.
