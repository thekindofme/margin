#!/usr/bin/env python3
"""Snapshot GitHub traffic, repo counters and release download counts.

GitHub keeps traffic data for 14 days only, so a scheduled workflow
(.github/workflows/repo-stats.yml) runs this daily and commits the output to
the `stats` branch. Everything here is aggregate data GitHub already shows on
the repo's Insights page; nothing identifies a visitor.

Usage: GITHUB_TOKEN=... python3 collect.py OWNER/REPO OUT_DIR
The token needs read access to the repo's traffic ("Administration: read" on a
fine-grained token); the release and repo counters work with any token.
"""
import csv
import datetime as dt
import json
import os
import sys
import urllib.request

API = "https://api.github.com"


def get(path):
    req = urllib.request.Request(
        API + path,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": "Bearer " + os.environ["GITHUB_TOKEN"],
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "milkrice-stats",
        },
    )
    with urllib.request.urlopen(req) as resp:
        return json.load(resp)


def upsert_csv(path, key_fields, fields, rows):
    """Merge rows into a CSV keyed by key_fields; newer values win."""
    existing = {}
    if os.path.exists(path):
        with open(path, newline="") as f:
            for r in csv.DictReader(f):
                existing[tuple(r[k] for k in key_fields)] = r
    for r in rows:
        existing[tuple(str(r[k]) for k in key_fields)] = {k: r[k] for k in fields}
    with open(path, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields)
        w.writeheader()
        for k in sorted(existing):
            w.writerow(existing[k])


def asset_kind(name):
    if name.endswith(".dmg"):
        return "dmg"
    if name == "latest.json":
        return "update-check"
    if name.endswith(".app.tar.gz"):
        return "update-bundle"
    if name.endswith(".sig") or name.startswith("SHA256SUMS"):
        return "checksum"
    return "other"


def main():
    repo, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    today = dt.datetime.now(dt.timezone.utc).date().isoformat()
    base = f"/repos/{repo}"

    # Daily views and clones (GitHub returns the last 14 days; we keep them all).
    for kind in ("views", "clones"):
        data = get(f"{base}/traffic/{kind}?per=day")
        upsert_csv(
            os.path.join(out, f"traffic-{kind}.csv"),
            ["date"],
            ["date", "count", "uniques"],
            [{"date": d["timestamp"][:10], "count": d["count"], "uniques": d["uniques"]}
             for d in data[kind]],
        )

    # Referrers and popular paths are 14-day rolling totals; keep a daily snapshot.
    upsert_csv(
        os.path.join(out, "referrers.csv"),
        ["snapshot", "referrer"],
        ["snapshot", "referrer", "count", "uniques"],
        [{"snapshot": today, "referrer": r["referrer"], "count": r["count"], "uniques": r["uniques"]}
         for r in get(f"{base}/traffic/popular/referrers")],
    )
    upsert_csv(
        os.path.join(out, "paths.csv"),
        ["snapshot", "path"],
        ["snapshot", "path", "count", "uniques"],
        [{"snapshot": today, "path": p["path"], "count": p["count"], "uniques": p["uniques"]}
         for p in get(f"{base}/traffic/popular/paths")],
    )

    r = get(base)
    upsert_csv(
        os.path.join(out, "repo.csv"),
        ["date"],
        ["date", "stars", "forks", "watchers", "open_issues"],
        [{"date": today, "stars": r["stargazers_count"], "forks": r["forks_count"],
          "watchers": r["subscribers_count"], "open_issues": r["open_issues_count"]}],
    )

    # Cumulative download counts per release asset, snapshotted daily so the
    # report can show downloads per day/week. latest.json is fetched by the Mac
    # app's update check, so its count is a rough signal of installed copies.
    rows, page = [], 1
    while True:
        releases = get(f"{base}/releases?per_page=100&page={page}")
        for rel in releases:
            for a in rel["assets"]:
                rows.append({"date": today, "tag": rel["tag_name"], "asset": a["name"],
                             "kind": asset_kind(a["name"]), "downloads": a["download_count"]})
        if len(releases) < 100:
            break
        page += 1
    upsert_csv(
        os.path.join(out, "release-downloads.csv"),
        ["date", "tag", "asset"],
        ["date", "tag", "asset", "kind", "downloads"],
        rows,
    )
    print(f"Saved stats for {repo} on {today}")


if __name__ == "__main__":
    main()
