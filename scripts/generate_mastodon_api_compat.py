#!/usr/bin/env python3

from __future__ import annotations

import re
import sys
import urllib.request
from dataclasses import dataclass
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
ROUTER_RS = REPO_ROOT / "crates/cfwdon-worker/src/router.rs"
DOC_DIR = REPO_ROOT / "docs/mastodon-api-compat"

UPSTREAM_ROUTES_RB = "https://raw.githubusercontent.com/mastodon/mastodon/main/config/routes.rb"
UPSTREAM_API_RB = "https://raw.githubusercontent.com/mastodon/mastodon/main/config/routes/api.rb"


@dataclass(frozen=True)
class Route:
    method: str
    path: str


@dataclass(frozen=True)
class LocalRoute:
    method: str
    path: str
    handler: str


@dataclass(frozen=True)
class InventoryGroup:
    name: str
    match_prefixes: tuple[str, ...]
    include_exact: tuple[str, ...] = ()


DISCOVERY_ROUTES = [
    Route("GET", "/.well-known/oauth-authorization-server"),
    Route("GET", "/.well-known/nodeinfo"),
    Route("GET", "/.well-known/webfinger"),
    Route("GET", "/oauth/userinfo"),
    Route("POST", "/oauth/userinfo"),
    Route("GET", "/api/oembed"),
]

EXCLUDED_PREFIXES = (
    "/api/v1/admin/",
    "/api/v2/admin/",
    "/api/web/",
)

EXCLUDED_EXACT = {
    "/api/v1/admin",
    "/api/v2/admin",
    "/api/web",
}

EXCLUDED_ROUTE_KEYS = {
    ("POST", "/api/v1/measures"),
    ("POST", "/api/v1/dimensions"),
    ("POST", "/api/v1/retention"),
    ("GET", "/api/v1/canonical_email_blocks"),
    ("POST", "/api/v1/canonical_email_blocks"),
    ("GET", "/api/v1/canonical_email_blocks/:id"),
    ("DELETE", "/api/v1/canonical_email_blocks/:id"),
    ("POST", "/api/v1/canonical_email_blocks/test"),
    ("GET", "/api/v1/tags"),
    ("PUT", "/api/v1/tags/:id"),
    ("PATCH", "/api/v1/tags/:id"),
}

GROUPS = [
    InventoryGroup(
        "Discovery / OAuth / Meta",
        match_prefixes=("/.well-known/", "/oauth/", "/api/oembed"),
        include_exact=("/api/oembed",),
    ),
    InventoryGroup(
        "Experimental / Alpha",
        match_prefixes=("/api/v1_alpha",),
    ),
    InventoryGroup(
        "Instance / Apps / Trends",
        match_prefixes=(
            "/api/v1/instance",
            "/api/v2/instance",
            "/api/v1/apps",
            "/api/v1/custom_emojis",
            "/api/v1/preferences",
            "/api/v1/announcements",
            "/api/v1/trends",
            "/api/v1/suggestions",
            "/api/v2/suggestions",
            "/api/v1/donation_campaigns",
            "/api/v1/annual_reports",
            "/api/v1/emails",
        ),
    ),
    InventoryGroup(
        "Timelines / Search / Streaming",
        match_prefixes=(
            "/api/v1/timelines",
            "/api/v1/streaming",
            "/api/v1/search",
            "/api/v2/search",
        ),
    ),
    InventoryGroup(
        "Statuses / Polls / Media",
        match_prefixes=(
            "/api/v1/statuses",
            "/api/v1/scheduled_statuses",
            "/api/v1/polls",
            "/api/v1/media",
            "/api/v2/media",
        ),
    ),
    InventoryGroup(
        "Accounts / Relationships / Tags",
        match_prefixes=(
            "/api/v1/accounts",
            "/api/v1/profile",
            "/api/v1/blocks",
            "/api/v1/mutes",
            "/api/v1/favourites",
            "/api/v1/bookmarks",
            "/api/v1/directory",
            "/api/v1/follow_requests",
            "/api/v1/followed_tags",
            "/api/v1/tags",
            "/api/v1/featured_tags",
            "/api/v1/endorsements",
        ),
    ),
    InventoryGroup(
        "Notifications / Conversations / Lists / Filters / Push",
        match_prefixes=(
            "/api/v1/notifications",
            "/api/v2/notifications",
            "/api/v1/conversations",
            "/api/v1/lists",
            "/api/v1/filters",
            "/api/v2/filters",
            "/api/v1/push",
            "/api/v1/domain_blocks",
            "/api/v1/peers",
            "/api/v1/markers",
            "/api/v1/reports",
        ),
    ),
]

COMPAT_GAPS = {}

TODO_COMPAT_NOTES = {}


def fetch_text(url: str) -> str:
    with urllib.request.urlopen(url) as response:
        return response.read().decode()


def normalize_path(path: str) -> str:
    path = re.sub(r"//+", "/", path)
    if not path.startswith("/"):
        path = "/" + path
    return path


def canonicalize_path(path: str) -> str:
    path = re.sub(r":[A-Za-z_][A-Za-z0-9_]*", ":param", path)
    path = re.sub(r"\*([A-Za-z_][A-Za-z0-9_]*)", "*param", path)
    return path


def normalize_upstream_path(path: str) -> str:
    if path == "/oembed":
        return "/api/oembed"
    if path.startswith(("/v1/", "/v2/", "/web/", "/v1_alpha/")):
        return "/api" + path
    return path


def parse_only_arg(rest: str) -> list[str] | None:
    match = re.search(r"only:\s*(\[[^\]]*\]|:\w+)", rest)
    if not match:
        return None
    value = match.group(1)
    if value == "[]":
        return []
    if value.startswith(":"):
        return [value[1:]]
    return re.findall(r":(\w+)", value)


def parse_param(rest: str) -> str:
    match = re.search(r"param:\s*:(\w+)", rest)
    return match.group(1) if match else "id"


def nested_param_name(resource_name: str, rest: str) -> str:
    explicit = parse_param(rest)
    if explicit != "id":
        return explicit
    if resource_name.endswith("ies"):
        singular = resource_name[:-3] + "y"
    elif resource_name.endswith("ses"):
        singular = resource_name[:-2]
    elif resource_name.endswith("s"):
        singular = resource_name[:-1]
    else:
        singular = resource_name
    return f"{singular}_id"


def singular_routes() -> dict[str, list[tuple[str, str]]]:
    return {
        "show": [("GET", "")],
        "create": [("POST", "")],
        "update": [("PUT", ""), ("PATCH", "")],
        "destroy": [("DELETE", "")],
    }


def plural_routes() -> dict[str, list[tuple[str, str]]]:
    return {
        "index": [("GET", "")],
        "show": [("GET", "/:id")],
        "create": [("POST", "")],
        "update": [("PUT", "/:id"), ("PATCH", "/:id")],
        "destroy": [("DELETE", "/:id")],
    }


def parse_upstream_api_routes(api_text: str) -> list[Route]:
    @dataclass
    class Frame:
        nested: str
        collection: str
        member: str

    frames = [Frame("", "", "")]
    routes: list[Route] = []

    for raw_line in api_text.splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue

        if line == "end":
            if len(frames) > 1:
                frames.pop()
            continue

        current = frames[-1]

        namespace_match = re.match(r"namespace\s+:(\w+)\b.*\bdo$", line)
        if namespace_match:
            name = namespace_match.group(1)
            nested = normalize_path(current.nested + "/" + name)
            frames.append(Frame(nested, nested, nested))
            continue

        if (line.startswith("scope module:") or line.startswith("with_options")) and line.endswith("do"):
            frames.append(Frame(current.nested, current.collection, current.member))
            continue

        if line == "member do":
            frames.append(Frame(current.member, current.collection, current.member))
            continue

        if line == "collection do":
            frames.append(Frame(current.collection, current.collection, current.member))
            continue

        resources_match = re.match(r"resources\s+:(\w+)(.*?)(\s+do)?$", line)
        if resources_match:
            name, rest, has_block = resources_match.group(1), resources_match.group(2), resources_match.group(3)
            only = parse_only_arg(rest)
            if only is None:
                only = list(plural_routes())
            param = parse_param(rest)
            base = normalize_path(current.nested + "/" + name)
            for action in only:
                for method, suffix in plural_routes()[action]:
                    routes.append(Route(method, normalize_path(base + suffix.replace(":id", f":{param}"))))
            if has_block:
                nested_param = nested_param_name(name, rest)
                frames.append(Frame(normalize_path(base + f"/:{nested_param}"), base, normalize_path(base + f"/:{param}")))
            continue

        resource_match = re.match(r"resource\s+:(\w+)(.*?)(\s+do)?$", line)
        if resource_match:
            name, rest, has_block = resource_match.group(1), resource_match.group(2), resource_match.group(3)
            only = parse_only_arg(rest)
            if only is None:
                only = list(singular_routes())
            base = normalize_path(current.nested + "/" + name)
            for action in only:
                for method, suffix in singular_routes()[action]:
                    routes.append(Route(method, normalize_path(base + suffix)))
            if has_block:
                frames.append(Frame(base, base, base))
            continue

        direct_match = re.match(r"(get|post|put|patch|delete)\s+([:\"'/][^,\s]*)", line)
        if direct_match:
            method = direct_match.group(1).upper()
            token = direct_match.group(2).strip("\"'")
            if token.startswith(":"):
                path = normalize_path(current.nested + "/" + token[1:])
            else:
                path = normalize_path(current.nested + "/" + token.lstrip("/"))
            routes.append(Route(method, path))
            continue

    deduped: list[Route] = []
    seen: set[tuple[str, str]] = set()
    for route in DISCOVERY_ROUTES + routes:
        normalized = Route(route.method, normalize_upstream_path(route.path))
        key = (normalized.method, normalized.path)
        if key in seen:
            continue
        seen.add(key)
        deduped.append(normalized)
    return deduped


def parse_local_routes(router_text: str) -> list[LocalRoute]:
    pattern = re.compile(
        r"\.(get|post|put|patch|delete)_async\(\s*\"([^\"]+)\"\s*,\s*\|.*?\|\s*async move \{\s*([a-zA-Z0-9_]+)",
        re.S,
    )
    routes = [
        LocalRoute(method=method.upper(), path=path, handler=handler)
        for method, path, handler in pattern.findall(router_text)
    ]
    return routes


def should_track(route: Route) -> bool:
    if (route.method, route.path) in EXCLUDED_ROUTE_KEYS:
        return False
    if route.path in EXCLUDED_EXACT:
        return False
    return not any(route.path.startswith(prefix) for prefix in EXCLUDED_PREFIXES)


def classify(upstream: Route, local_map: dict[tuple[str, str], LocalRoute]) -> tuple[str, str, str]:
    local = local_map.get((upstream.method, canonicalize_path(upstream.path)))
    if local is None:
        return "-", "missing", ""
    note = compat_note_for_route(COMPAT_GAPS, upstream.method, upstream.path, local.path)
    if note:
        return f"`{local.handler}`", "compat-gap", note
    return f"`{local.handler}`", "implemented", ""


def compat_note_for_route(
    notes: dict[tuple[str, str], str],
    method: str,
    upstream_path: str,
    local_path: str,
) -> str:
    direct = notes.get((method, upstream_path), "") or notes.get((method, local_path), "")
    if direct:
        return direct

    upstream_canonical = canonicalize_path(upstream_path)
    local_canonical = canonicalize_path(local_path)
    for (note_method, note_path), note in notes.items():
        if note_method != method:
            continue
        note_canonical = canonicalize_path(note_path)
        if note_canonical == upstream_canonical or note_canonical == local_canonical:
            return note
    return ""


def group_routes(routes: list[Route]) -> dict[str, list[Route]]:
    result: dict[str, list[Route]] = {group.name: [] for group in GROUPS}
    for route in routes:
        for group in GROUPS:
            matched = route.path in group.include_exact or any(route.path.startswith(prefix) for prefix in group.match_prefixes)
            if matched:
                result[group.name].append(route)
                break
        else:
            raise RuntimeError(f"ungrouped route: {route.method} {route.path}")
    return result


def format_inventory(upstream_routes: list[Route], local_routes: list[LocalRoute]) -> str:
    local_map = {(route.method, canonicalize_path(route.path)): route for route in local_routes}
    grouped = group_routes(upstream_routes)

    lines = [
        "# Inventory",
        "",
        "このファイルは `scripts/generate_mastodon_api_compat.py` で生成する。",
        "",
        "`cfwdon` のローカル route は `crates/cfwdon-worker/src/router.rs` の handler 名にマッピングする。",
        "",
    ]

    for group in GROUPS:
        lines.append(f"## {group.name}")
        lines.append("")
        lines.append("| Method | Mastodon route | cfwdon handler | Status | Note |")
        lines.append("| --- | --- | --- | --- | --- |")
        for route in grouped[group.name]:
            handler, status, note = classify(route, local_map)
            lines.append(f"| {route.method} | `{route.path}` | {handler} | `{status}` | {note} |")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def format_unimplemented(upstream_routes: list[Route], local_routes: list[LocalRoute]) -> str:
    local_keys = {(route.method, canonicalize_path(route.path)) for route in local_routes}
    grouped = group_routes(upstream_routes)

    lines = [
        "# TODO: Unimplemented",
        "",
        "このファイルは `scripts/generate_mastodon_api_compat.py` で生成する。",
        "",
        "`inventory.md` で `missing` のものだけを追う。",
        "",
    ]

    for group in GROUPS:
        missing = [
            route
            for route in grouped[group.name]
            if (route.method, canonicalize_path(route.path)) not in local_keys
        ]
        if not missing:
            continue
        lines.append(f"## {group.name}")
        lines.append("")
        for route in missing:
            lines.append(f"- [ ] `{route.method} {route.path}`")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def format_compat() -> str:
    grouped: dict[str, list[tuple[tuple[str, str], str]]] = {
        "Experimental / Alpha": [],
        "Discovery / OAuth / Meta": [],
        "Instance / Apps / Trends": [
        ],
        "Timelines / Search": [],
        "Statuses / Polls": [],
        "Accounts / Profile": [
        ],
        "Notifications": [
        ],
        "Notification Requests": [
        ],
        "Domain Blocks": [
        ],
        "Accounts / Endorsements": [
        ],
        "Tags": [
        ],
        "Status Extras": [
        ],
        "Scheduled Statuses": [
        ],
    }

    lines = [
        "# TODO: Compatibility Gaps",
        "",
        "このファイルは `scripts/generate_mastodon_api_compat.py` で生成する。",
        "",
        "route はあるが、互換性の詰めが残っているもの。",
        "",
    ]

    for section, items in grouped.items():
        active_items = [
            ((method, path), note)
            for (method, path), note in items
            if (method, path) in COMPAT_GAPS
        ]
        if not active_items:
            continue
        lines.append(f"## {section}")
        lines.append("")
        for (method, path), note in active_items:
            lines.append(f"- [ ] `{method} {path}`")
            lines.append(f"  {note}")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def format_readme(local_routes: list[LocalRoute], upstream_routes: list[Route]) -> str:
    local_keys = {(route.method, canonicalize_path(route.path)) for route in local_routes}
    local_map = {(route.method, canonicalize_path(route.path)): route for route in local_routes}
    upstream_keys = {(route.method, canonicalize_path(route.path)) for route in upstream_routes}
    extra_routes = [
        route
        for route in local_routes
        if route.path.startswith("/api/")
        and not route.path.startswith("/api/v1/admin")
        and not route.path.startswith("/api/v2/admin")
        and (route.method, canonicalize_path(route.path)) not in upstream_keys
    ]
    implemented_count = sum(
        1
        for route in upstream_routes
        if classify(route, local_map)[1] == "implemented"
    )
    compat_gap_count = sum(
        1
        for route in upstream_routes
        if classify(route, local_map)[1] == "compat-gap"
    )
    missing_count = sum(
        1
        for route in upstream_routes
        if (route.method, canonicalize_path(route.path)) not in local_keys
    )

    lines = [
        "# Mastodon API Compatibility",
        "",
        "`cfwdon` の Mastodon API 互換作業を、Mastodon upstream の定義に対するマッピングとして管理する。",
        "",
        "## Source of Truth",
        "",
        "- Upstream route definition:",
        f"  - `{UPSTREAM_ROUTES_RB}`",
        f"  - `{UPSTREAM_API_RB}`",
        "- Local route definition:",
        "  - `crates/cfwdon-worker/src/router.rs`",
        "- Existing project TODO:",
        "  - `docs/full-todo.md`",
        "",
        "`docs.joinmastodon.org` と `config/routes/api.rb` の間で deprecated endpoint の記載差分があるため、このディレクトリでは upstream の route 定義を優先する。",
        "",
        "## Scope",
        "",
        "初回 inventory の対象は、Mastodon の外部公開 API のうち `cfwdon` が互換対象として追う価値が高いものに絞る。",
        "",
        "- discovery / OAuth metadata",
        "- `/api/oembed`",
        "- `/api/v1_alpha`",
        "- `/api/v1`",
        "- `/api/v2`",
        "",
        "現時点では次は対象外にしている。",
        "",
        "- `/api/v1/admin`, `/api/v2/admin`",
        "- `/api/web`",
        "- ActivityPub actor / inbox / outbox そのもの",
        "",
        "## Status Labels",
        "",
        "- `implemented`: upstream route と同じ path/method が `cfwdon` にある",
        "- `compat-gap`: route はあるが、既存 TODO や実装メモ上で互換差分が分かっている",
        "- `missing`: upstream route が `cfwdon` に無い",
        "- `extra`: `cfwdon` にはあるが、current upstream route には無い",
        "",
        "## Files",
        "",
        "- `inventory.md`: upstream API 一覧と `cfwdon` マッピング",
        "- `todo-unimplemented.md`: `missing` のみを抜き出した TODO",
        "- `todo-compat.md`: `compat-gap` のみを抜き出した TODO",
        "- `../scripts/generate_mastodon_api_compat.py`: inventory / TODO 再生成スクリプト",
        "",
        "## Refresh",
        "",
        "```bash",
        "rtk python scripts/generate_mastodon_api_compat.py",
        "```",
        "",
        "## Current Extra Routes In cfwdon",
        "",
        "current upstream の `config/routes/api.rb` には無いが、`cfwdon` にはある route。",
        "",
    ]

    for route in extra_routes:
        lines.append(f"- `{route.method} {route.path}` via `{route.handler}`")

    lines.extend(
        [
            "",
            "deprecated route を残している可能性があるので、削除ではなく upstream 側の扱いを確認してから整理する。",
            "",
            "## Snapshot",
            "",
            f"- tracked upstream routes: `{len(upstream_routes)}`",
            f"- local tracked routes: `{sum(1 for route in local_routes if route.path.startswith('/api/'))}`",
            f"- implemented routes: `{implemented_count}`",
            f"- compatibility gaps: `{compat_gap_count}`",
            f"- missing routes: `{missing_count}`",
            f"- extra routes: `{len(extra_routes)}`",
        ]
    )
    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    api_text = fetch_text(UPSTREAM_API_RB)
    router_text = ROUTER_RS.read_text()

    upstream_routes = [route for route in parse_upstream_api_routes(api_text) if should_track(route)]
    local_routes = parse_local_routes(router_text)

    DOC_DIR.mkdir(parents=True, exist_ok=True)
    (DOC_DIR / "README.md").write_text(format_readme(local_routes, upstream_routes))
    (DOC_DIR / "inventory.md").write_text(format_inventory(upstream_routes, local_routes))
    (DOC_DIR / "todo-unimplemented.md").write_text(format_unimplemented(upstream_routes, local_routes))
    (DOC_DIR / "todo-compat.md").write_text(format_compat())

    print(f"wrote {DOC_DIR / 'README.md'}")
    print(f"wrote {DOC_DIR / 'inventory.md'}")
    print(f"wrote {DOC_DIR / 'todo-unimplemented.md'}")
    print(f"wrote {DOC_DIR / 'todo-compat.md'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
