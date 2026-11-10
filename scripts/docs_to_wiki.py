#!/usr/bin/env python3
"""Mirror docs/ into a GitHub wiki checkout. docs/ stays the source of truth.

Usage: docs_to_wiki.py <repo-root> <wiki-dir> <owner/repo>
Each page is named after its first "# " heading; docs/features/README.md becomes Home.
Links between docs become wiki links; images and other repo files point at main.
"""
import re
import sys
from pathlib import Path

root, wiki, repo = Path(sys.argv[1]).resolve(), Path(sys.argv[2]), sys.argv[3]
docs = root / "docs"
blob = f"https://github.com/{repo}/blob/main/"
raw = f"https://raw.githubusercontent.com/{repo}/main/"
IMG = {".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"}

# Lower-case files are guides; upper-case ones (NOTICE, CONTRIBUTING, ...) stay repo pages.
sources = sorted(docs.glob("features/*.md")) + sorted(p for p in docs.glob("*.md") if p.name.islower())
pages = {}
for src in sources:
    title = next(l[2:].strip() for l in src.read_text().splitlines() if l.startswith("# "))
    pages[src] = "Home" if src == docs / "features/README.md" else title.replace(" ", "-")
assert len(set(pages.values())) == len(pages), "two docs share a title"


def link(src, target):
    if re.match(r"[a-z]+:|#", target):
        return target
    path, _, anchor = target.partition("#")
    dest = (src.parent / path).resolve()
    anchor = "#" + anchor if anchor else ""
    if dest in pages:
        return pages[dest] + anchor
    rel = dest.relative_to(root).as_posix()
    return (raw if dest.suffix.lower() in IMG else blob) + rel + anchor


for old in wiki.glob("*.md"):
    old.unlink()
for src, name in pages.items():
    body = re.sub(r"\]\(([^)\s]+)\)", lambda m: "](" + link(src, m.group(1)) + ")", src.read_text())
    rel = src.relative_to(root).as_posix()
    body += f"\n\n---\n*This page is generated from [{rel}]({blob}{rel}). Edit it there; changes made here are overwritten.*\n"
    (wiki / f"{name}.md").write_text(body)

side = ["**[Home](Home)**", "", "**Features**"]
side += [f"- [{pages[s].replace('-', ' ')}]({pages[s]})" for s in sources if s.parent.name == "features" and pages[s] != "Home"]
side += ["", "**More**"] + [f"- [{pages[s].replace('-', ' ')}]({pages[s]})" for s in sources if s.parent == docs]
(wiki / "_Sidebar.md").write_text("\n".join(side) + "\n")
