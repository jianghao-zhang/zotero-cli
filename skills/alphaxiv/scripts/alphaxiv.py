#!/usr/bin/env python3
"""alphaXiv public discovery helper for agents.

Uses normal HTTP only. Public mode is default; auth headers are used only for
Recommended feed when explicitly supplied through environment variables/files.
"""

from __future__ import annotations

import argparse
import base64
import datetime as _dt
import json
import os
import pathlib
import re
import shlex
import socket
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from typing import Any


API_BASE = "https://api.alphaxiv.org"
WEB_BASE = "https://www.alphaxiv.org"
PDF_BASE = "https://fetcher.alphaxiv.org/v2/pdf"
CLERK_BASE = "https://clerk.alphaxiv.org"
CLERK_API_VERSION = "2025-04-10"
CLERK_JS_VERSION = "5.125.10"

HEADERS = {
    "User-Agent": "Mozilla/5.0",
    "Accept": "application/json,*/*",
    "Origin": WEB_BASE,
    "Referer": f"{WEB_BASE}/",
}

ARXIV_ID_RE = re.compile(r"^(?P<id>\d{4}\.\d{4,5})(?:v\d+)?$")
VERSIONED_ID_RE = re.compile(r"^.+v\d+$")
UUID_RE = re.compile(
    r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$"
)


class AlphaXivError(RuntimeError):
    def __init__(self, message: str, status: int | None = None):
        super().__init__(message)
        self.status = status


def print_json(data: Any) -> None:
    print(json.dumps(data, ensure_ascii=False, indent=2, sort_keys=False))


def endpoint(path_or_url: str) -> str:
    if path_or_url.startswith("http://") or path_or_url.startswith("https://"):
        return path_or_url
    return f"{API_BASE}{path_or_url}"


def config_dir() -> pathlib.Path:
    return pathlib.Path.home() / ".config" / "alphaxiv"


def display_path(path: pathlib.Path) -> str:
    try:
        return f"~/{path.resolve().relative_to(pathlib.Path.home()).as_posix()}"
    except ValueError:
        return str(path)


def read_first_data_line(path: pathlib.Path) -> str | None:
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        raise AlphaXivError(f"Could not read {path}: {exc}") from exc
    for line in lines:
        line = line.strip()
        if not line or line.startswith("#") or line.startswith("PASTE_"):
            continue
        return line
    return None


def cookie_header() -> str | None:
    direct = os.environ.get("ALPHAXIV_COOKIE")
    if direct:
        return direct.strip()

    cookie_file = os.environ.get("ALPHAXIV_COOKIE_FILE")
    if not cookie_file:
        return None

    path = pathlib.Path(os.path.expanduser(cookie_file))
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        raise AlphaXivError(f"Could not read ALPHAXIV_COOKIE_FILE: {exc}") from exc

    pairs: list[str] = []
    raw_headers: list[str] = []
    for line in lines:
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        cols = line.split("\t")
        if len(cols) >= 7:
            pairs.append(f"{cols[5]}={cols[6]}")
        elif "=" in line:
            raw_headers.append(line)

    if pairs:
        return "; ".join(pairs)
    if raw_headers:
        return "; ".join(raw_headers)
    return None


def clerk_cookie_header() -> str | None:
    direct = os.environ.get("ALPHAXIV_CLERK_COOKIE")
    if direct:
        return direct.strip()

    cookie_file = os.environ.get("ALPHAXIV_CLERK_COOKIE_FILE")
    if not cookie_file:
        default_path = config_dir() / "clerk-cookie.txt"
        cookie_file = str(default_path) if default_path.exists() else ""
    if not cookie_file:
        return None

    path = pathlib.Path(os.path.expanduser(cookie_file))
    cookie = read_first_data_line(path)
    if cookie and cookie.lower().startswith("cookie:"):
        cookie = cookie.split(":", 1)[1].strip()
    return cookie


def jwt_payload(token: str) -> dict[str, Any]:
    raw = token.strip()
    if raw.lower().startswith("bearer "):
        raw = raw.split(None, 1)[1]
    parts = raw.split(".")
    if len(parts) < 2:
        return {}
    payload = parts[1] + "=" * (-len(parts[1]) % 4)
    try:
        return json.loads(base64.urlsafe_b64decode(payload.encode("ascii")).decode("utf-8"))
    except Exception:
        return {}


def iso_from_ms(value: Any) -> str | None:
    if not isinstance(value, (int, float)):
        return None
    return _dt.datetime.fromtimestamp(value / 1000, tz=_dt.timezone.utc).isoformat()


def iso_from_s(value: Any) -> str | None:
    if not isinstance(value, (int, float)):
        return None
    return _dt.datetime.fromtimestamp(value, tz=_dt.timezone.utc).isoformat()


def clerk_client(timeout: int = 20) -> dict[str, Any]:
    cookie = clerk_cookie_header()
    if not cookie:
        raise AlphaXivError(
            "No Clerk session cookie configured. Export it to ~/.config/alphaxiv/clerk-cookie.txt.",
            status=401,
        )
    data = fetch_json(
        f"{CLERK_BASE}/v1/client",
        {
            "__clerk_api_version": CLERK_API_VERSION,
            "_clerk_js_version": CLERK_JS_VERSION,
        },
        extra_headers={"Cookie": cookie},
        timeout=timeout,
    )
    return data if isinstance(data, dict) else {}


def active_clerk_session(data: dict[str, Any]) -> dict[str, Any]:
    response = data.get("response") if isinstance(data.get("response"), dict) else {}
    sessions = response.get("sessions") if isinstance(response.get("sessions"), list) else []
    active = [s for s in sessions if isinstance(s, dict) and s.get("status") == "active"]
    if active:
        return active[0]
    return sessions[0] if sessions and isinstance(sessions[0], dict) else {}


def clerk_bearer_token(timeout: int = 20) -> str | None:
    session = active_clerk_session(clerk_client(timeout=timeout))
    token = session.get("last_active_token") if isinstance(session.get("last_active_token"), dict) else {}
    jwt = token.get("jwt") if isinstance(token.get("jwt"), str) else None
    return f"Bearer {jwt}" if jwt else None


def authorization_header() -> str | None:
    direct = os.environ.get("ALPHAXIV_AUTHORIZATION") or os.environ.get("ALPHAXIV_BEARER_TOKEN")
    if direct:
        value = direct.strip()
        return value if value.lower().startswith("bearer ") else f"Bearer {value}"

    auth_file = os.environ.get("ALPHAXIV_AUTHORIZATION_FILE")
    if not auth_file:
        default_path = config_dir() / "authorization.txt"
        auth_file = str(default_path) if default_path.exists() else ""
    if auth_file:
        line = read_first_data_line(pathlib.Path(os.path.expanduser(auth_file)))
        if line:
            if line.lower().startswith("authorization:"):
                line = line.split(":", 1)[1].strip()
            return line if line.lower().startswith("bearer ") else f"Bearer {line}"

    return clerk_bearer_token()


def extra_auth_headers() -> dict[str, str]:
    headers: dict[str, str] = {}
    authorization = authorization_header()
    if authorization:
        headers["Authorization"] = authorization

    session_id = os.environ.get("ALPHAXIV_USER_SESSION_ID")
    if session_id:
        headers["user-session-id"] = session_id.strip()
    arxiv_session_id = os.environ.get("ALPHAXIV_ARXIV_SESSION_ID")
    if arxiv_session_id:
        headers["arxiv-session-id"] = arxiv_session_id.strip()
    arxiv_pageview_id = os.environ.get("ALPHAXIV_ARXIV_PAGEVIEW_ID")
    if arxiv_pageview_id:
        headers["arxiv-pageview-id"] = arxiv_pageview_id.strip()
    return headers


def request(
    path_or_url: str,
    params: dict[str, Any] | None = None,
    *,
    accept: str | None = None,
    extra_headers: dict[str, str] | None = None,
    use_cookie: bool = False,
    timeout: int = 20,
) -> tuple[bytes, urllib.response.addinfourl]:
    url = endpoint(path_or_url)
    if params:
        url = f"{url}?{urllib.parse.urlencode(params)}"

    headers = dict(HEADERS)
    if accept:
        headers["Accept"] = accept
    if extra_headers:
        headers.update(extra_headers)
    if use_cookie:
        headers.update(extra_auth_headers())
        cookie = cookie_header()
        if cookie:
            headers["Cookie"] = cookie

    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.read(), resp
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        message = body.strip() or exc.reason
        raise AlphaXivError(message, status=exc.code) from exc
    except urllib.error.URLError as exc:
        if isinstance(exc.reason, socket.timeout):
            raise AlphaXivError("request timed out", status=504) from exc
        raise AlphaXivError(str(exc.reason)) from exc
    except (TimeoutError, socket.timeout) as exc:
        raise AlphaXivError("request timed out", status=504) from exc


def fetch_json(
    path_or_url: str,
    params: dict[str, Any] | None = None,
    *,
    extra_headers: dict[str, str] | None = None,
    use_cookie: bool = False,
    timeout: int = 20,
) -> Any:
    body, _resp = request(
        path_or_url,
        params,
        extra_headers=extra_headers,
        use_cookie=use_cookie,
        timeout=timeout,
    )
    try:
        return json.loads(body.decode("utf-8"))
    except json.JSONDecodeError as exc:
        raise AlphaXivError(f"Expected JSON but got {len(body)} bytes") from exc


def fetch_text(path_or_url: str, *, accept: str = "text/markdown,*/*", timeout: int = 20) -> str:
    body, _resp = request(path_or_url, accept=accept, timeout=timeout)
    return body.decode("utf-8", errors="replace")


def papers_from_response(data: Any) -> list[dict[str, Any]]:
    if isinstance(data, list):
        return [p for p in data if isinstance(p, dict)]
    if isinstance(data, dict):
        papers = data.get("papers") or data.get("trendingPapers") or data.get("results") or []
        if isinstance(papers, list):
            return [p for p in papers if isinstance(p, dict)]
    return []


def first_present(*values: Any) -> Any:
    for value in values:
        if value not in (None, "", [], {}):
            return value
    return None


def normalize_people(value: Any) -> list[Any]:
    if not isinstance(value, list):
        return []
    out: list[Any] = []
    for person in value:
        if isinstance(person, str):
            out.append(person)
        elif isinstance(person, dict):
            name = first_present(
                person.get("name"),
                person.get("full_name"),
                " ".join(
                    part
                    for part in [
                        str(person.get("first_name") or "").strip(),
                        str(person.get("last_name") or "").strip(),
                    ]
                    if part
                ),
            )
            out.append(name or person)
        else:
            out.append(person)
    return out


def normalize_topics(value: Any) -> list[Any]:
    if not isinstance(value, list):
        return []
    out: list[Any] = []
    for topic in value:
        if isinstance(topic, str):
            out.append(topic)
        elif isinstance(topic, dict):
            out.append(first_present(topic.get("name"), topic.get("display_name"), topic.get("slug"), topic))
        else:
            out.append(topic)
    return out


def resource_github(resources: Any) -> Any:
    if isinstance(resources, dict):
        return first_present(resources.get("github"), resources.get("GitHub"), resources.get("github_url"))
    if isinstance(resources, list):
        for item in resources:
            if isinstance(item, dict):
                label = str(first_present(item.get("type"), item.get("label"), item.get("name"), "")).lower()
                url = first_present(item.get("url"), item.get("link"))
                if "github" in label and url:
                    return url
            elif isinstance(item, str) and "github.com" in item:
                return item
    return None


def metrics_from(paper: dict[str, Any], group: dict[str, Any] | None = None) -> dict[str, Any]:
    group = group or {}
    metrics = first_present(paper.get("metrics"), group.get("metrics"), {}) or {}
    visits = metrics.get("visits_count") if isinstance(metrics, dict) else {}
    if not isinstance(visits, dict):
        visits = {}
    return {
        "public_total_votes": metrics.get("public_total_votes") if isinstance(metrics, dict) else None,
        "total_votes": metrics.get("total_votes") if isinstance(metrics, dict) else None,
        "x_likes": metrics.get("x_likes") if isinstance(metrics, dict) else None,
        "github_stars": first_present(paper.get("github_stars"), group.get("github_stars")),
        "visits_all": visits.get("all"),
        "visits_7d": visits.get("last_7_days"),
    }


def merge_dicts(base: dict[str, Any], extra: dict[str, Any]) -> dict[str, Any]:
    merged = dict(base)
    for key, value in extra.items():
        if key == "metrics" and isinstance(value, dict):
            existing = merged.get("metrics") if isinstance(merged.get("metrics"), dict) else {}
            merged["metrics"] = {**existing, **{k: v for k, v in value.items() if v is not None}}
        elif key == "resources" and isinstance(value, dict):
            existing = merged.get("resources") if isinstance(merged.get("resources"), dict) else {}
            merged["resources"] = {**existing, **{k: v for k, v in value.items() if v is not None}}
        elif value not in (None, "", [], {}) and merged.get(key) in (None, "", [], {}):
            merged[key] = value
    return merged


def normalize_legacy(data: dict[str, Any]) -> dict[str, Any]:
    root = data.get("paper") if isinstance(data.get("paper"), dict) else data
    version = first_present(root.get("paper_version"), root.get("paperVersion"), {}) or {}
    group = first_present(root.get("paper_group"), root.get("paperGroup"), {}) or {}
    resources = first_present(group.get("resources"), version.get("resources"), {}) or {}

    universal_id = first_present(
        group.get("universal_paper_id"),
        group.get("universalId"),
        group.get("universal_id"),
        version.get("universal_paper_id"),
        version.get("universalId"),
        version.get("universal_id"),
    )
    version_label = first_present(version.get("version_label"), version.get("versionLabel"), group.get("versionLabel"))
    canonical_id = first_present(
        version.get("canonical_id"),
        version.get("canonicalId"),
        group.get("canonical_id"),
        group.get("canonicalId"),
    )
    if not canonical_id and universal_id and version_label:
        canonical_id = f"{universal_id}{version_label}"

    return normalize_flat(
        {
            **group,
            **version,
            "universal_paper_id": universal_id,
            "canonical_id": canonical_id,
            "version_id": first_present(version.get("id"), version.get("version_id"), version.get("versionId")),
            "paper_group_id": first_present(group.get("id"), group.get("paper_group_id"), group.get("groupId")),
            "resources": resources,
            "metrics": first_present(group.get("metrics"), version.get("metrics")),
            "github_stars": first_present(group.get("github_stars"), version.get("github_stars")),
            "github_url": first_present(group.get("github_url"), version.get("github_url"), resource_github(resources)),
            "authors": first_present(group.get("authors"), version.get("authors")),
            "topics": first_present(group.get("topics"), version.get("topics")),
        }
    )


def normalize_flat(paper: dict[str, Any]) -> dict[str, Any]:
    universal_id = first_present(
        paper.get("universal_paper_id"),
        paper.get("universalId"),
        paper.get("universal_id"),
        paper.get("paperId"),
    )
    raw_id = paper.get("id")
    if not universal_id and isinstance(raw_id, str) and not UUID_RE.match(raw_id):
        universal_id = raw_id

    version_label = first_present(paper.get("versionLabel"), paper.get("version_label"))
    canonical_id = first_present(paper.get("canonical_id"), paper.get("canonicalId"))
    if not canonical_id and universal_id and version_label:
        canonical_id = f"{universal_id}{version_label}"
    if not canonical_id and universal_id and VERSIONED_ID_RE.match(str(universal_id)):
        canonical_id = universal_id

    alphaxiv_id = str(universal_id) if universal_id else None
    if alphaxiv_id and VERSIONED_ID_RE.match(alphaxiv_id):
        match = ARXIV_ID_RE.match(alphaxiv_id)
        if match:
            alphaxiv_id = match.group("id")

    group_id = first_present(
        paper.get("paper_group_id"),
        paper.get("groupId"),
        paper.get("group_id"),
        raw_id if isinstance(raw_id, str) and UUID_RE.match(raw_id) else None,
    )
    version_id = first_present(paper.get("version_id"), paper.get("versionId"))
    resources = paper.get("resources") or {}
    github_url = first_present(paper.get("github_url"), resource_github(resources))
    summary = first_present(
        paper.get("summary"),
        paper.get("abstract"),
        paper.get("paper_summary"),
        paper.get("overview"),
    )

    return {
        "source": "alphaxiv",
        "title": paper.get("title"),
        "alphaxiv_id": alphaxiv_id,
        "canonical_id": canonical_id,
        "version_id": version_id,
        "group_id": group_id,
        "url": f"{WEB_BASE}/abs/{alphaxiv_id}" if alphaxiv_id else None,
        "overview_url": f"{WEB_BASE}/overview/{alphaxiv_id}" if alphaxiv_id else None,
        "pdf_url": f"{PDF_BASE}/{canonical_id}" if canonical_id else None,
        "authors": normalize_people(paper.get("authors")),
        "topics": normalize_topics(paper.get("topics")),
        "summary": summary,
        "metrics": metrics_from(paper),
        "resources": {"github": github_url} if github_url else {},
    }


def normalize_paper(data: dict[str, Any]) -> dict[str, Any]:
    if isinstance(data.get("paper"), dict) or isinstance(data.get("paper_group"), dict):
        return normalize_legacy(data)
    return normalize_flat(data)


def dedupe_papers(papers: list[dict[str, Any]]) -> list[dict[str, Any]]:
    seen: set[str] = set()
    out: list[dict[str, Any]] = []
    for paper in papers:
        key = first_present(paper.get("canonical_id"), paper.get("alphaxiv_id"), paper.get("title"))
        if key and key in seen:
            continue
        if key:
            seen.add(str(key))
        out.append(paper)
    return out


def fetch_feed_page(
    *,
    page_num: int,
    page_size: int,
    sort: str,
    interval: str,
    timeout: int,
) -> tuple[list[dict[str, Any]], int]:
    sizes = []
    for size in [page_size, 100, 50, 20]:
        if size not in sizes and size <= page_size:
            sizes.append(size)
    last_error: AlphaXivError | None = None
    for size in sizes:
        try:
            data = fetch_json(
                "/papers/v3/feed",
                {
                    "pageNum": page_num,
                    "pageSize": size,
                    "sort": sort,
                    "interval": interval,
                    "topics": "[]",
                },
                use_cookie=sort == "Recommended",
                timeout=timeout,
            )
            return [normalize_paper(p) for p in papers_from_response(data)], size
        except AlphaXivError as exc:
            last_error = exc
            if exc.status == 401 and sort == "Recommended":
                raise AlphaXivError(
                    "Recommended feed requires a valid alphaXiv login. Run auth-status to inspect the local Clerk cookie, or run auth-refresh after explicit user authorization.",
                    status=401,
                ) from exc
            if exc.status not in (500, 504, None):
                raise
    raise last_error or AlphaXivError("feed request failed")


def cmd_feed(args: argparse.Namespace) -> None:
    papers: list[dict[str, Any]] = []
    page_num = 0
    page_size = args.page_size
    while len(papers) < args.limit:
        batch, actual_size = fetch_feed_page(
            page_num=page_num,
            page_size=min(page_size, max(args.limit - len(papers), 20)),
            sort=args.sort,
            interval=args.interval,
            timeout=args.timeout,
        )
        if actual_size != page_size:
            page_size = actual_size
        if not batch:
            break
        papers.extend(batch)
        papers = dedupe_papers(papers)
        if len(batch) < page_size:
            break
        page_num += 1

    print_json(
        {
            "source": "alphaxiv",
            "query": {
                "sort": args.sort,
                "interval": args.interval,
                "limit": args.limit,
                "page_size": page_size,
            },
            "count": min(len(papers), args.limit),
            "papers": papers[: args.limit],
        }
    )


def cmd_search(args: argparse.Namespace) -> None:
    mode = "fast" if args.fast else "full"
    errors: list[dict[str, Any]] = []
    if args.fast:
        data = fetch_json(
            "/search/v2/paper/fast",
            {"q": args.query, "includePrivate": "false"},
            timeout=args.timeout,
        )
    else:
        try:
            data = fetch_json("/v1/search/paper", {"q": args.query}, timeout=args.timeout)
        except AlphaXivError as exc:
            mode = "fast_fallback"
            errors.append({"endpoint": "full_search", "status": exc.status, "message": str(exc)})
            data = fetch_json(
                "/search/v2/paper/fast",
                {"q": args.query, "includePrivate": "false"},
                timeout=args.timeout,
            )
    papers = [normalize_paper(p) for p in papers_from_response(data)]
    print_json(
        {
            "source": "alphaxiv",
            "query": args.query,
            "search": mode,
            "count": min(len(papers), args.limit),
            "papers": papers[: args.limit],
            "errors": errors,
        }
    )


def fetch_paper_bundle(paper_id: str, *, timeout: int, include_preview: bool = True) -> dict[str, Any]:
    compact: dict[str, Any] = {}
    legacy: dict[str, Any] = {}
    preview: dict[str, Any] = {}
    errors: list[dict[str, Any]] = []

    for label, path in [
        ("compact", f"/papers/v3/{paper_id}"),
        ("legacy", f"/papers/v3/legacy/{paper_id}"),
    ]:
        try:
            if label == "compact":
                compact = fetch_json(path, timeout=timeout)
            else:
                legacy = fetch_json(path, timeout=timeout)
        except AlphaXivError as exc:
            errors.append({"endpoint": label, "status": exc.status, "message": str(exc)})

    if include_preview:
        try:
            preview = fetch_json(f"/papers/v3/{paper_id}/preview", timeout=timeout)
        except AlphaXivError as exc:
            errors.append({"endpoint": "preview", "status": exc.status, "message": str(exc)})

    normalized: dict[str, Any] = {}
    if compact:
        normalized = normalize_paper(compact)
    if legacy:
        normalized = merge_dicts(normalized, normalize_paper(legacy))
    if preview:
        normalized = merge_dicts(normalized, normalize_paper(preview))

    return {
        "source": "alphaxiv",
        "id": paper_id,
        "normalized": normalized,
        "compact": compact,
        "legacy": legacy,
        "preview": preview,
        "errors": errors,
    }


def cmd_paper(args: argparse.Namespace) -> None:
    bundle = fetch_paper_bundle(args.paper_id, timeout=args.timeout, include_preview=not args.no_preview)
    if args.raw:
        print_json(bundle)
    else:
        print_json(
            {
                "source": "alphaxiv",
                "id": args.paper_id,
                "paper": bundle["normalized"],
                "errors": bundle["errors"],
            }
        )


def resolve_version_id(paper_id: str, *, timeout: int) -> str:
    compact = fetch_json(f"/papers/v3/{paper_id}", timeout=timeout)
    version_id = first_present(compact.get("versionId"), compact.get("version_id"))
    if not version_id:
        raise AlphaXivError(f"No versionId found for {paper_id}")
    return str(version_id)


def resolve_canonical_id(paper_id: str, *, timeout: int) -> str:
    if VERSIONED_ID_RE.match(paper_id):
        return paper_id
    bundle = fetch_paper_bundle(paper_id, timeout=timeout, include_preview=False)
    canonical_id = bundle["normalized"].get("canonical_id")
    if not canonical_id:
        raise AlphaXivError(f"No canonical/versioned ID found for {paper_id}")
    return str(canonical_id)


def cmd_overview(args: argparse.Namespace) -> None:
    version_id = args.paper_id if UUID_RE.match(args.paper_id) else resolve_version_id(args.paper_id, timeout=args.timeout)
    suffix = "status" if args.status else args.lang
    data = fetch_json(f"/papers/v3/{version_id}/overview/{suffix}", timeout=args.timeout)
    print_json({"source": "alphaxiv", "id": args.paper_id, "version_id": version_id, "overview": data})


def cmd_markdown(args: argparse.Namespace) -> None:
    route = "abs" if args.kind == "abs" else "overview"
    url = f"{WEB_BASE}/{route}/{args.paper_id}.md"
    text = fetch_text(url, timeout=args.timeout)
    if args.output:
        path = pathlib.Path(args.output).expanduser()
        path.write_text(text, encoding="utf-8")
    shown = text if args.max_chars <= 0 else text[: args.max_chars]
    if args.format == "text":
        print(shown, end="" if shown.endswith("\n") else "\n")
    else:
        print_json(
            {
                "source": "alphaxiv",
                "id": args.paper_id,
                "kind": args.kind,
                "url": url,
                "chars": len(text),
                "truncated": args.max_chars > 0 and len(text) > args.max_chars,
                "output": str(path) if args.output else None,
                "markdown": shown,
            }
        )


def cmd_pdf(args: argparse.Namespace) -> None:
    canonical_id = resolve_canonical_id(args.paper_id, timeout=args.timeout)
    url = f"{PDF_BASE}/{canonical_id}"
    result: dict[str, Any] = {
        "source": "alphaxiv",
        "id": args.paper_id,
        "canonical_id": canonical_id,
        "pdf_url": url,
    }
    if args.download:
        body, resp = request(url, accept="application/pdf,*/*", timeout=args.timeout)
        path = pathlib.Path(args.download).expanduser()
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(body)
        result.update(
            {
                "download": str(path),
                "bytes": len(body),
                "final_url": resp.geturl(),
            }
        )
    print_json(result)


def arxiv_base_id(value: str | None) -> str | None:
    if not value:
        return None
    match = ARXIV_ID_RE.match(value)
    return match.group("id") if match else None


def metrics_note(paper: dict[str, Any]) -> str:
    metrics = paper.get("metrics") if isinstance(paper.get("metrics"), dict) else {}
    today = _dt.date.today().isoformat()
    rows = [
        "## alphaXiv metrics",
        "",
        f"- alphaXiv URL: {paper.get('url') or ''}",
        f"- Overview: {paper.get('overview_url') or ''}",
        f"- Public likes: {metrics.get('public_total_votes')}",
        f"- Internal votes: {metrics.get('total_votes')}",
        f"- X likes: {metrics.get('x_likes')}",
        f"- GitHub stars: {metrics.get('github_stars')}",
        f"- Visits: {metrics.get('visits_all')}",
        f"- Retrieved: {today}",
    ]
    return "\n".join(rows)


def cmd_zotero_plan(args: argparse.Namespace) -> None:
    bundle = fetch_paper_bundle(args.paper_id, timeout=args.timeout, include_preview=False)
    paper = bundle["normalized"]
    alpha_id = paper.get("alphaxiv_id") or args.paper_id
    canonical_id = paper.get("canonical_id")
    url = paper.get("url") or f"{WEB_BASE}/abs/{args.paper_id}"
    commands: list[str] = []
    import_strategy = "url"

    base_id = arxiv_base_id(str(alpha_id)) or arxiv_base_id(str(canonical_id))
    if base_id:
        import_strategy = "arxiv"
        commands.append(f"zcli import arxiv {shlex.quote(base_id)} --dry-run --format json")
    elif canonical_id:
        import_strategy = "pdf"
        pdf_path = f"/tmp/{canonical_id}.pdf"
        commands.append(f"zcli alphaxiv pdf {shlex.quote(args.paper_id)} --download {shlex.quote(pdf_path)} --format json")
        commands.append(f"zcli import pdf {shlex.quote(pdf_path)} --dry-run --format json")
        commands.append(f"zcli import url {shlex.quote(str(url))} --dry-run --format json")
    else:
        commands.append(f"zcli import url {shlex.quote(str(url))} --dry-run --format json")

    print_json(
        {
            "source": "alphaxiv",
            "id": args.paper_id,
            "paper": paper,
            "import_strategy": import_strategy,
            "dry_run_commands": commands,
            "note_markdown": metrics_note(paper),
            "write_note_command_template": "zcli write note ITEMKEY --content '<note_markdown>' --dry-run --format json",
            "mutation_policy": "Do not add --execute unless the user explicitly approves the Zotero mutation in the current turn.",
            "errors": bundle["errors"],
        }
    )


def auth_status_payload(timeout: int = 20) -> dict[str, Any]:
    data = clerk_client(timeout=timeout)
    response = data.get("response") if isinstance(data.get("response"), dict) else {}
    session = active_clerk_session(data)
    token = session.get("last_active_token") if isinstance(session.get("last_active_token"), dict) else {}
    jwt = token.get("jwt") if isinstance(token.get("jwt"), str) else ""
    payload = jwt_payload(jwt) if jwt else {}
    now = _dt.datetime.now(tz=_dt.timezone.utc)

    session_expires_at = iso_from_ms(session.get("expire_at"))
    token_expires_at = iso_from_s(payload.get("exp"))
    session_warning = None
    if isinstance(session.get("expire_at"), (int, float)):
        expires = _dt.datetime.fromtimestamp(session["expire_at"] / 1000, tz=_dt.timezone.utc)
        days_left = (expires - now).total_seconds() / 86400
        if days_left <= 1:
            session_warning = "expires within 24 hours; refresh alphaXiv login soon"
        elif days_left <= 2:
            session_warning = "expires within 2 days; refresh alphaXiv login soon"

    return {
        "source": "alphaxiv",
        "auth": {
            "configured": True,
            "clerk_cookie_file": display_path(config_dir() / "clerk-cookie.txt"),
            "session_status": session.get("status"),
            "session_id_hint": str(session.get("id", ""))[:8] if session.get("id") else None,
            "session_expires_at": session_expires_at,
            "session_abandon_at": iso_from_ms(session.get("abandon_at")),
            "client_cookie_expires_at": iso_from_ms(response.get("cookie_expires_at")),
            "short_token_available": bool(jwt),
            "short_token_issued_at": iso_from_s(payload.get("iat")),
            "short_token_expires_at": token_expires_at,
            "warning": session_warning,
        },
    }


def cmd_auth_status(args: argparse.Namespace) -> None:
    print_json(auth_status_payload(timeout=args.timeout))


def cmd_auth_refresh(args: argparse.Namespace) -> None:
    script = pathlib.Path(__file__).with_name("alphaxiv_auth_refresh.mjs")
    if not script.exists():
        raise AlphaXivError(f"missing auth refresh helper: {script}")
    command = [
        "node",
        str(script),
        "--output",
        str(config_dir() / "clerk-cookie.txt"),
        "--limit",
        str(args.limit),
        "--interval",
        args.interval,
    ]
    if args.no_open:
        command.append("--no-open")
    if args.no_validate_feed:
        command.append("--no-validate-feed")
    result = subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.stdout.strip():
        print(result.stdout.strip())
    if result.returncode != 0:
        message = result.stderr.strip() or "auth refresh failed"
        if not result.stdout.strip():
            print_json({"source": "alphaxiv", "action": "auth-refresh", "ok": False, "error": {"message": message}})
        raise SystemExit(result.returncode)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Query alphaXiv public discovery APIs.")
    parser.add_argument("--timeout", type=int, default=20, help="HTTP timeout in seconds.")
    sub = parser.add_subparsers(dest="command", required=True)

    auth = sub.add_parser("auth-status", help="Check Recommended-feed Clerk session status without printing credentials.")
    auth.set_defaults(func=cmd_auth_status)

    auth_refresh = sub.add_parser("auth-refresh", help="Refresh Recommended-feed auth from the current Chrome alphaXiv login via CDP.")
    auth_refresh.add_argument("--limit", type=int, default=5, help="Recommended feed validation limit.")
    auth_refresh.add_argument("--interval", default="7 Days", choices=["3 Days", "7 Days", "30 Days", "90 Days", "All time"])
    auth_refresh.add_argument("--no-open", action="store_true", help="Do not open alphaXiv if no alphaXiv tab exists.")
    auth_refresh.add_argument("--no-validate-feed", action="store_true", help="Only export Clerk cookie; skip Recommended feed validation.")
    auth_refresh.set_defaults(func=cmd_auth_refresh)

    feed = sub.add_parser("feed", help="Fetch a public alphaXiv feed.")
    feed.add_argument("--sort", default="Hot", choices=["Hot", "Comments", "Views", "Likes", "GitHub", "Twitter", "Recommended"])
    feed.add_argument("--interval", default="All time", choices=["3 Days", "7 Days", "30 Days", "90 Days", "All time"])
    feed.add_argument("--limit", type=int, default=100)
    feed.add_argument("--page-size", type=int, default=100)
    feed.set_defaults(func=cmd_feed)

    search = sub.add_parser("search", help="Search alphaXiv papers.")
    search.add_argument("query")
    search.add_argument("--limit", type=int, default=20)
    search.add_argument("--fast", action="store_true", help="Use autocomplete endpoint instead of full search.")
    search.set_defaults(func=cmd_search)

    paper = sub.add_parser("paper", help="Resolve metadata and metrics for one paper.")
    paper.add_argument("paper_id")
    paper.add_argument("--raw", action="store_true", help="Include raw compact/legacy/preview payloads.")
    paper.add_argument("--no-preview", action="store_true")
    paper.set_defaults(func=cmd_paper)

    overview = sub.add_parser("overview", help="Fetch overview JSON by paper ID or versionId.")
    overview.add_argument("paper_id")
    overview.add_argument("--lang", default="en")
    overview.add_argument("--status", action="store_true")
    overview.set_defaults(func=cmd_overview)

    markdown = sub.add_parser("markdown", help="Fetch alphaXiv markdown routes.")
    markdown.add_argument("paper_id")
    markdown.add_argument("--kind", default="abs", choices=["abs", "overview"])
    markdown.add_argument("--format", default="json", choices=["json", "text"])
    markdown.add_argument("--max-chars", type=int, default=0, help="Truncate markdown in output; 0 means full text.")
    markdown.add_argument("--output", help="Save markdown to this path.")
    markdown.set_defaults(func=cmd_markdown)

    pdf = sub.add_parser("pdf", help="Resolve or download PDF through alphaXiv fetcher.")
    pdf.add_argument("paper_id")
    pdf.add_argument("--download", help="Download PDF to this path.")
    pdf.set_defaults(func=cmd_pdf)

    zotero = sub.add_parser("zotero-plan", help="Print dry-run-first zcli import plan for an alphaXiv paper.")
    zotero.add_argument("paper_id")
    zotero.set_defaults(func=cmd_zotero_plan)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        args.func(args)
        return 0
    except AlphaXivError as exc:
        print_json({"source": "alphaxiv", "error": {"status": exc.status, "message": str(exc)}})
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
