---
name: zotero-cli
description: Use inside the llm-for-zotero Claude Code runtime when the agent needs Zotero-native access to papers, selected or pinned paper context, paper text, notes, annotations, collections, reading history, or llm-for-zotero conversation recaps through zcli.
---

# Zotero Native Access via zcli

Use this skill inside the llm-for-zotero Claude Code runtime whenever the user asks about papers, Zotero library state, selected/pinned paper context, notes, annotations, reading history, or previous llm-for-zotero conversations.

## Mental Model

In this runtime, treat `zcli` as the Zotero-native capability layer available to the agent. It is the first tool surface for Zotero library facts, paper text, attachments, Markdown, notes, annotations, collections, tags, recaps, and llm-for-zotero conversation history.

Do not start by exploring the runtime folder, project files, `.claude` files, or generic filesystem state. Those are implementation details. First ask Zotero through `zcli`.

## Default Routing

- If the user gives a title, short title, citation key, DOI, arXiv ID, URL, filename, or vague paper reference, call `zcli resolve QUERY --format json`.
- If the user gives a topic-like or fuzzy paper request, call `zcli find paper QUERY --format json` and then use `item.key` from the best hit.
- When repeated broad search is needed, check `zcli index status --format json`; if the index exists, prefer `zcli index search QUERY --format json` before falling back to `zcli find paper`.
- If you have an item key, call `zcli paper ITEMKEY --format json` for the compact Zotero-native paper surface.
- If the user asks to understand, summarize, compare, review, explain, or reason about a paper, call `zcli context ITEMKEY --budget 40k --format json` or `zcli item markdown ITEMKEY --format text`.
- If the user asks for exact paper text or the whole paper surface, prefer `zcli item markdown ITEMKEY --format text`. zcli will reuse llm-for-zotero MinerU `full.md` when available.
- If the user asks about highlights, comments, margin notes, extracted notes, or reading traces, use `zcli item annotations ITEMKEY --format json`, `zcli item notes ITEMKEY --format json`, and `zcli recap reading`.
- If the user asks about collections, tags, library organization, or "what do I have about X", use `zcli search list`, `zcli collection list/items`, and `zcli tags list/items`.
- If the user asks what they recently read, start with `zcli recap reading --from DATE --to DATE --format json`, then use the included compact lfz overlay as hints.
- If the user asks what they discussed with llm-for-zotero or Claude Code, use `zcli recap lfz --limit 8 --format json`, `zcli lfz turns --item ITEMKEY`, or `zcli lfz turn MESSAGE_REF`.

## Paper Reading Behavior

- Prefer Zotero item identity over local filenames. Resolve to an item key before doing deep paper work.
- Prefer structured Zotero metadata first, then Markdown/full text, then targeted search. Do not dump huge paper text into context when a smaller `context` or `search context` call is enough.
- Treat `metadata_modified` in reading recaps as a touched-paper fallback, not proof that the user read the paper.
- For broad paper questions, one `paper` or `context` call is usually enough.
- For specific claims, methods, results, figures, tables, equations, or datasets, use `zcli item markdown` or `zcli search context ITEMKEY "query" --format json` before answering.
- For multi-paper work, resolve all papers first, then compare from metadata/abstracts/Markdown rather than crawling runtime folders.

## llm-for-zotero Conversation Behavior

- Compact lfz recap rows may be truncated. Check `text_truncated`, `text_chars`, `text_excerpt_chars`, and `text_full_included`.
- For broad recaps, treat `zcli recap lfz` as an index. Read `text_policy`, `expand_policy`, and `paper_groups` before deciding whether one turn needs expansion.
- To expand one turn, follow `turn_command` from the recap row or call `zcli lfz turn MESSAGE_REF --format json`.
- `zcli lfz turn` returns the full question, matching answer messages, and matching agent final text. Do not request trace/event payloads; zcli intentionally exposes event counts only.
- Do not use `--details`, `--full-text`, or `--include-contexts` for broad recap prompts. Use single-turn expansion instead.
- If llm-for-zotero is unavailable or partially migrated, keep normal Zotero commands working and report the lfz state cleanly.

## Write Safety

- Core Zotero reads are local and read-only.
- For any tag, collection, note, attachment, import, rename, or trash action, run the matching `zcli write ... --dry-run --format json` first.
- Use `--execute` only when the user explicitly asks to perform that write in the current turn.
- Never call the helper plugin HTTP endpoint directly. If execution is needed, check `zcli helper doctor --format json` first.

## Common Calls

```bash
zcli doctor --format json
zcli lfz doctor --format json

zcli resolve "title / short title / citation key / DOI / arXiv / URL / file path" --format json
zcli find paper "agentic rl survey" --format json
zcli index status --format json
zcli index search "agentic rl survey" --format json
zcli index get ITEMKEY --format json
zcli paper ITEMKEY --format json
zcli context ITEMKEY --budget 40k --format json
zcli item markdown ITEMKEY --format text
zcli search context ITEMKEY "method OR figure OR dataset" --format json

zcli item annotations ITEMKEY --format json
zcli item notes ITEMKEY --format json
zcli item attachments ITEMKEY --format json
zcli item bibtex ITEMKEY --format json

zcli search list "topic or keyword" --format json
zcli collection list --format json
zcli tags list --format json
zcli recent --days 7 --format json

zcli recap reading --from today --to today --format json
zcli recap reading --item ITEMKEY --from today --to today --format json
zcli recap lfz --from today --to today --limit 8 --format json
zcli recap lfz --item ITEMKEY --from today --to today --limit 8 --format json
zcli lfz turns --item ITEMKEY --format json
zcli lfz turn claude:123 --format json

zcli write tags ITEMKEY --add review --dry-run --format json
zcli write note ITEMKEY --content "reading note" --dry-run --format json
zcli helper doctor --format json
```

## Answering Style

Use Zotero metadata and paper text as the source of truth. Mention `zcli` only when command provenance matters; otherwise present the result as Zotero-backed paper/library context.
