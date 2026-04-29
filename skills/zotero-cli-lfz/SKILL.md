---
name: zotero-cli
description: Use inside llm-for-zotero Claude Code mode when the agent needs Zotero-native access to papers, selected or pinned paper context, paper text, notes, annotations, collections, tags, reading history, or llm-for-zotero conversation recaps through zcli.
---

# Zotero Native Access via zcli

Use this skill inside llm-for-zotero Claude Code mode whenever the user asks about papers, Zotero library state, selected or pinned papers, paper text, notes, annotations, reading history, collections, tags, or previous llm-for-zotero conversations.

## Mental Model

Treat `zcli` as the Zotero-native capability layer for the agent.

Do not start from runtime folders, project files, `.claude` files, or generic filesystem exploration. Those are implementation details. Start from Zotero concepts: item identity, paper text, metadata, notes, annotations, collections, tags, reading history, and llm-for-zotero conversation history.

## Routing

- Paper identity from title, short title, citation key, DOI, arXiv, URL, filename, or vague reference -> `zcli resolve QUERY --format json`.
- Topic-like or fuzzy paper request -> `zcli find paper QUERY --format json`; use the best hit's `item.key`.
- Repeated or broad paper search -> `zcli index status --format json`, then `zcli index search QUERY --format json`.
- If indexed search is missing and the task needs passage/full-paper search, run `zcli index update --include-full-text --format json`; otherwise run `zcli index update --format json`.
- Passage search -> `zcli index chunks QUERY --format json`. Add `--item ITEMKEY` for one paper, `--collection NAME` for a collection scope, or `--tag TAG` for a tagged slice.
- Expand one passage -> use the hit's `expand_command` or `zcli index chunk CHUNK_ID --format json`.
- Compact paper surface -> `zcli paper ITEMKEY --format json`.
- Agent reading context -> `zcli context ITEMKEY --budget 40k --format json`.
- Full Markdown paper surface -> `zcli item markdown ITEMKEY --format text`.
- Annotations -> `zcli item annotations ITEMKEY --format json`.
- Notes -> `zcli item notes ITEMKEY --format json`.
- Reading recap -> `zcli recap reading --from DATE --to DATE --format json`.
- llm-for-zotero recap -> `zcli recap lfz --limit 8 --format json`.
- One previous turn -> follow `turn_command` or call `zcli lfz turn MESSAGE_REF --format json`.

## Reading Behavior

Resolve the Zotero item first. Prefer compact context before full Markdown. Use targeted passage search for exact claims, equations, figures, tables, metrics, datasets, method details, and limitations.

Treat `index search` as paper-candidate search and `index chunks` as passage-candidate search. Chunk hits may include `page` or `page_label`; those are best-effort page labels from Zotero annotations or PDF page separators. A missing page means the source text has no reliable page marker, not that the passage is invalid.

For multi-paper work, resolve all papers first, compare from Zotero metadata and paper surfaces, then deepen only where the user's question requires it.

When answering from passage hits, preserve the paper identity and page label when available.

## Conversation Recaps

Treat `zcli recap lfz` as an index, not as full conversation memory. Expand one turn only when needed through `turn_command` or `zcli lfz turn MESSAGE_REF --format json`.

Do not request trace payloads or runtime event internals. `zcli` exposes compact event counts only.

## Write Safety

Reads are safe by default.

For tags, notes, attachments, imports, metadata edits, collection changes, or trash operations, use `zcli write ... --dry-run --format json` first. Use `--execute` only when the user explicitly asks to perform the write in the current turn.

Never call the helper plugin HTTP endpoint directly. If execution is needed, check `zcli helper doctor --format json` first.

## Answering

Use Zotero metadata and paper text as the source of truth. Mention `zcli` only when command provenance matters; otherwise answer as normal Zotero-backed research context.
