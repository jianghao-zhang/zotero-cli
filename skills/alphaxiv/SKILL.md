---
name: alphaxiv
description: Use alphaXiv as a public paper discovery and metrics source without browser/web-access/CDP. Trigger for alphaXiv feeds, search, recent paper triage, time-window filtering, metrics, markdown/PDF/overview retrieval, Zotero dry-run import planning, and alphaXiv-to-zcli inbox handoff. For broad multi-source intake or X paper discussion, route through zcli inbox.
---

# alphaXiv

Use this skill when alphaXiv itself is the discovery or metrics source. Public alphaXiv surfaces work through normal HTTP; do not use browser automation, web-access, CDP, or login flows unless the user explicitly asks for authenticated Recommended feed support.

## Safety

- Public mode is default. Do not require, print, log, or store auth headers/cookies unless the user explicitly asks for authenticated Recommended feed support.
- `sort=Recommended` is an auth boundary, not anti-bot. alphaXiv uses Clerk: the `Authorization: Bearer ...` token is short-lived, while the Clerk session cookie can refresh short tokens until the session expires. Never show cookie or token contents.
- Treat alphaXiv as discovery/metrics evidence. Use Zotero CLI for Zotero library reads/writes.
- Zotero imports and notes are dry-run-first: `zcli import ... --dry-run --format json` and `zcli write note ... --dry-run --format json`. Use `--execute` only with explicit current-turn user approval.

## Routing

- alphaXiv-specific feed/search/metrics/brief -> `zcli alphaxiv ...`.
- Broad “papers to read” across alphaXiv/Hugging Face/X -> `zcli inbox fetch ...`.
- One known paper’s X/community discussion -> `zcli inbox discussion PAPER --format json`.
- Zotero import/write -> use returned `zcli import ... --dry-run` or `zcli write ... --dry-run`; never invent a separate mutation path.

## Commands

Prefer the Rust-native `zcli alphaxiv` commands for repeatable work. Use `brief` for broad research triage, `discover` for JSON candidate pipelines, `search` for query-only lookup, and `feed` for alphaXiv ranking pages:

```bash
zcli alphaxiv feed --sort hot --interval "All time" --limit 100 --format json
zcli alphaxiv feed --sort likes --interval "30 Days" --limit 100 --format json
zcli alphaxiv feed --sort github --interval "30 Days" --min-github-stars 5 --rank-metrics --limit 30 --format json
zcli alphaxiv feed --sort hot --days 7 --date-field first-seen --rank-metrics --limit 30 --format json
zcli alphaxiv auth-refresh --format json
zcli alphaxiv auth-status --format json
zcli alphaxiv feed --sort recommended --interval "7 Days" --limit 30 --format json
zcli alphaxiv search "agentic harness" --since 2026-04-28 --date-field published --rank-metrics --limit 20 --format json
zcli alphaxiv search "agentic harness" --topic agents --rank-metrics --with-zotero-plan --limit 10 --format json
zcli alphaxiv discover "coding agent harness memory" --days 30 --date-field any --min-likes 1 --limit 10 --format json
zcli alphaxiv brief "coding agent harness memory" --days 30 --date-field any --limit 8 --format text
zcli alphaxiv paper 2604.25850 --format json
zcli alphaxiv paper "https://www.alphaxiv.org/abs/2604.25850" --format json
zcli alphaxiv overview 2604.25850 --lang en --format json
zcli alphaxiv markdown visual-primitives --kind abs --format json
zcli alphaxiv markdown visual-primitives --kind overview --format json
zcli alphaxiv pdf visual-primitives --download /tmp/visual-primitives.pdf --format json
zcli alphaxiv zotero-plan 2604.25850 --format json
```

The Rust path returns normalized JSON for feeds, search, discovery, paper metadata, PDF downloads, and zcli import plans. It accepts bare alphaXiv IDs, alphaXiv URLs, arXiv abs/PDF URLs, and alphaXiv fetcher PDF URLs.

`discover` and `brief` share one discovery pipeline: full search, fast search fallback, feed fallback, dedupe, time filtering, metric ranking, then Zotero dry-run import plans. Use `brief` when the user wants an agent-readable triage with read-now/skim/watch lanes and import commands.

For recency-sensitive requests, always set `--days N` or `--since YYYY-MM-DD`. Choose `--date-field first-seen` for newly appearing alphaXiv papers, `published` for publication date, `updated` for recently changed entries, and `any` for broad recent activity. Prefer `--since` for reproducible batches.

The older Python helper at `scripts/alphaxiv.py` remains a fallback while the Rust path is the preferred agent interface.

When the user wants a broader "papers to read" intake surface rather than alphaXiv-specific output, route through `zcli inbox fetch [QUERY] --source alphaxiv|huggingface|x --days N --date-field any --dry-run --format json`. It wraps source-specific discovery into `paper_candidate/v1` records; alphaXiv remains the strongest alphaXiv-specific adapter, while Hugging Face and X/Bird cover daily HF papers, HF search, and curated X paper accounts. Inbox candidates add local Zotero/queue context matching, source-specific time semantics, workflow commands, and optional quick GitHub code overview via `--code-overview`.

When the user wants X discussion around one known alphaXiv/arXiv paper, route through `zcli inbox discussion PAPER --format json` rather than alphaXiv comments alone. Pass the paper title, arXiv ID, or alphaXiv URL; add `--handle AUTHOR_OR_CURATOR` or `--tweet URL` when known. The output separates announcement posts from high-value questions, possible author answers, limitations, benchmark/comparison notes, and code/data/reproduction discussion.

For Recommended feed, prefer the local Clerk cookie file:

```text
~/.config/alphaxiv/clerk-cookie.txt
```

The helper can refresh a short Bearer token from that cookie by calling Clerk `/v1/client`. Use `auth-status` to check the session expiry. Treat the cookie file as a login credential: mode `600`, never print it, and refresh it from the browser only after explicit user authorization.

To update Recommended auth without DevTools hand-copying, use:

```bash
web-access-cdp on
zcli alphaxiv auth-refresh --format json
web-access-cdp off
```

`auth-refresh` is exposed through `zcli` but delegates to the bundled Node CDP bridge because Chrome DevTools cookie export is a browser-session workflow. It connects to the current Chrome CDP endpoint, finds or opens alphaXiv, reads only alphaXiv/Clerk cookies, writes `~/.config/alphaxiv/clerk-cookie.txt`, and validates Recommended feed. It prints cookie names, expiry metadata, and validation status only; never token or cookie values.

## Public Endpoints

Base headers:

```python
HEADERS = {
    "User-Agent": "Mozilla/5.0",
    "Accept": "application/json,*/*",
    "Origin": "https://www.alphaxiv.org",
    "Referer": "https://www.alphaxiv.org/",
}
```

Home SSR:

```text
GET https://www.alphaxiv.org/
```

The home page is SSR HTML with TanStack dehydration data. Default feed parameters are `sort=Hot`, `interval=All time`, `pageSize=20`.

Feed API:

```text
GET https://api.alphaxiv.org/papers/v3/feed?pageNum=0&pageSize=100&sort=Hot&interval=All+time&topics=%5B%5D
```

Valid `sort`: `Hot`, `Comments`, `Views`, `Likes`, `GitHub`, `Twitter`, `Recommended`.

Valid `interval`: `3 Days`, `7 Days`, `30 Days`, `90 Days`, `All time`.

Default `pageSize=100`. Fall back to `50`, then `20`; `200` and `500` have returned 504/500. Parse both response shapes:

```python
papers = data.get("papers") or data.get("trendingPapers") or []
```

Search:

```text
GET https://api.alphaxiv.org/v1/search/paper?q=agentic+harness
GET https://api.alphaxiv.org/search/v2/paper/fast?q=agentic+harness&includePrivate=false
```

Use full search first because it includes metrics and GitHub stars. Use fast search only as autocomplete or fallback.

Metadata:

```text
GET https://api.alphaxiv.org/papers/v3/{id}
GET https://api.alphaxiv.org/papers/v3/legacy/{id}
GET https://api.alphaxiv.org/papers/v3/{id}/preview
```

`/papers/v3/{id}` resolves compact metadata and `versionId`. `/legacy/{id}` is richer for metrics, authors, resources, and comments.

Overview needs `versionId`, not the universal paper ID:

```text
GET https://api.alphaxiv.org/papers/v3/{versionId}/overview/en
GET https://api.alphaxiv.org/papers/v3/{versionId}/overview/status
```

Markdown:

```text
GET https://www.alphaxiv.org/overview/{id}.md
GET https://www.alphaxiv.org/abs/{id}.md
```

`/resources/{id}.md` has returned 404; do not rely on it.

PDF:

```text
GET https://fetcher.alphaxiv.org/v2/pdf/{canonical_id}
```

Use a versioned canonical ID such as `2604.25850v3` or `visual-primitivesv1`. Versionless IDs return `paperID must contain a version`.

Other useful public endpoints:

```text
GET https://api.alphaxiv.org/papers/v3/legacy/{groupId}/comments
GET https://api.alphaxiv.org/papers/v3/{paperGroupId}/figures
GET https://api.alphaxiv.org/papers/v3/{id}/similar-papers
```

Use `groupId` for legacy comments when universal IDs produce schema errors.

## Normalized Paper Schema

Normalize discovery output to this shape before ranking, importing, or writing notes:

```json
{
  "source": "alphaxiv",
  "title": "...",
  "alphaxiv_id": "2604.25850",
  "canonical_id": "2604.25850v3",
  "version_id": "...",
  "group_id": "...",
  "url": "https://www.alphaxiv.org/abs/2604.25850",
  "overview_url": "https://www.alphaxiv.org/overview/2604.25850",
  "pdf_url": "https://fetcher.alphaxiv.org/v2/pdf/2604.25850v3",
  "authors": [],
  "topics": [],
  "summary": "...",
  "first_seen_at": "2026-05-05T16:55:02+00:00",
  "published_at": "2026-05-05T17:51:38+00:00",
  "updated_at": "2026-05-06T02:00:17+00:00",
  "metrics": {
    "public_total_votes": 74,
    "total_votes": 13,
    "x_likes": 0,
    "github_stars": 1,
    "visits_all": 796,
    "visits_7d": 796
  },
  "resources": {
    "github": "..."
  }
}
```

Metrics field mapping:

- `metrics.public_total_votes`: page-displayed likes.
- `metrics.total_votes`: internal votes.
- `metrics.x_likes`: Twitter/X likes.
- `github_stars`: GitHub stars.
- `metrics.visits_count.all`: total visits.
- `metrics.visits_count.last_7_days`: 7-day visits.

## Zotero CLI Workflow

`zcli alphaxiv zotero-plan ID --format json` delegates to zcli's shared import-plan logic. For arXiv-like alphaXiv IDs matching `^\d{4}\.\d{4,5}(v\d+)?$`, it strips the version for arXiv import:

```bash
zcli import arxiv 2604.25850 --dry-run --format json
```

For alphaXiv-only IDs such as `visual-primitives`, `deepseek-v4`, or `a-path-towards-autonomous-machine-intelligence`, prefer the alphaXiv PDF fetcher when a canonical ID is available:

```bash
zcli alphaxiv pdf visual-primitives --download /tmp/visual-primitives.pdf --format json
zcli import pdf /tmp/visual-primitives.pdf --dry-run --format json
```

Use URL import as a fallback:

```bash
zcli import url https://www.alphaxiv.org/abs/visual-primitives --dry-run --format json
```

For high-value imports, add an alphaXiv metrics note only after the item exists and only dry-run first:

```bash
zcli write note ITEMKEY --content "## alphaXiv metrics

- alphaXiv URL: https://www.alphaxiv.org/abs/2604.25850
- Overview: https://www.alphaxiv.org/overview/2604.25850
- Public likes: 74
- Internal votes: 13
- X likes: 0
- GitHub stars: 1
- Visits: 796
- Retrieved: YYYY-MM-DD" --dry-run --format json
```

Execute imports or note writes only when the user explicitly approves the mutation in the current turn.
