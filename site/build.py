#!/usr/bin/env python3
"""Build the ProxyZip website into _site/.

Copies the hand-written files in site/, renders docs/*.md and site/content/features/*.md
into HTML pages that share one layout, and writes sitemap.xml and robots.txt.
Requires the `markdown` package.
"""
import html
import json
import re
import shutil
import sys
from datetime import date
from pathlib import Path

import markdown

ROOT = Path(__file__).resolve().parent.parent
SITE = ROOT / "site"
OUT = ROOT / "_site"
BASE = "https://luispeixoto.github.io/ProxyZip"
REPO = "https://github.com/LuisPeixoto/ProxyZip"
SKIP = {"build.py", "content", "README.md", "index.html.tmp"}

DOC_GROUPS = [
    ("Start", ["getting-started", "certificates", "devices", "troubleshooting"]),
    ("Use", ["inspecting", "filtering", "tools", "scripting", "plugins", "sessions"]),
    ("APIs", ["mocks", "suites", "workspaces", "mcp", "http3"]),
    ("Reference", ["cli", "distribution", "architecture", "roadmap"]),
]

FEATURES = [
    ("https-debugging", "HTTPS debugging"),
    ("breakpoints-rewrite", "Breakpoints and rewrite"),
    ("mock-api-server", "Mock API server"),
    ("api-testing", "API testing"),
    ("grpc-protobuf", "gRPC and Protobuf"),
    ("mobile-debugging", "Mobile debugging"),
    ("ai-assistants-mcp", "AI assistants (MCP)"),
]


def front_matter(text):
    meta = {}
    if text.startswith("---\n"):
        head, _, text = text[4:].partition("\n---\n")
        for line in head.splitlines():
            k, _, v = line.partition(":")
            meta[k.strip()] = v.strip()
    return meta, text.lstrip("\n")


def gh_slug(value, sep):
    """Anchor ids the way GitHub writes them, so links in the Markdown keep working."""
    value = re.sub(r"[^\w\- ]", "", value.strip().lower())
    return value.replace(" ", sep)


def md(text):
    return markdown.markdown(text, extensions=["fenced_code", "tables", "toc", "sane_lists"], extension_configs={"toc": {"slugify": gh_slug}})


def head(title, description, canonical, extra=""):
    t, d = html.escape(title), html.escape(description, quote=True)
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{t}</title>
<meta name="description" content="{d}">
<link rel="canonical" href="{canonical}">
<meta name="theme-color" content="#FAF9F5">
<meta property="og:type" content="article">
<meta property="og:site_name" content="ProxyZip">
<meta property="og:title" content="{t}">
<meta property="og:description" content="{d}">
<meta property="og:url" content="{canonical}">
<meta property="og:image" content="{BASE}/og.png">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="{t}">
<meta name="twitter:description" content="{d}">
<meta name="twitter:image" content="{BASE}/og.png">
<link rel="icon" href="/ProxyZip/favicon.svg" type="image/svg+xml">
<link rel="apple-touch-icon" href="/ProxyZip/apple-touch-icon.png">
<link rel="preload" href="/ProxyZip/fonts/ibm-plex-sans-latin-400-normal.woff2" as="font" type="font/woff2" crossorigin>
<link rel="stylesheet" href="/ProxyZip/styles.css">
{extra}</head>
"""


NAV = """<body>
<a class="skip" href="#main">Skip to content</a>
<header class="nav"><div class="wrap">
<a class="brand" href="/ProxyZip/"><img src="/ProxyZip/favicon.svg" width="26" height="26" alt="">ProxyZip</a>
<nav aria-label="Main"><ul>
<li><a href="/ProxyZip/#features">Features</a></li><li><a href="/ProxyZip/docs/getting-started/">Docs</a></li>
<li><a href="%(repo)s/blob/main/CHANGELOG.md">Changelog</a></li><li><a href="%(repo)s">GitHub</a></li>
</ul></nav><span class="spacer"></span><a class="btn primary sm" href="/ProxyZip/#download">Download</a>
</div></header>
""" % {"repo": REPO}

FOOT = """<footer><div class="wrap"><div class="foot">
<div class="about"><a class="brand" href="/ProxyZip/" style="padding:0"><img src="/ProxyZip/favicon.svg" width="26" height="26" alt="">ProxyZip</a><p>An open-source HTTP and HTTPS debugging proxy for macOS, Windows and Linux. Made by Luis Peixoto.</p></div>
<nav aria-label="Product"><p class="h5">Product</p>%(features)s</nav>
<nav aria-label="Learn"><p class="h5">Learn</p><a href="/ProxyZip/docs/getting-started/">Getting started</a><a href="/ProxyZip/docs/cli/">Command line</a><a href="/ProxyZip/docs/mcp/">MCP</a><a href="/ProxyZip/docs/troubleshooting/">Troubleshooting</a></nav>
<nav aria-label="Project"><p class="h5">Project</p><a href="%(repo)s">GitHub</a><a href="%(repo)s/blob/main/CHANGELOG.md">Changelog</a><a href="%(repo)s/blob/main/SECURITY.md">Security</a><a href="%(repo)s/blob/main/LICENSE">MIT license</a></nav>
</div><p class="legal">Use ProxyZip only on traffic you are allowed to inspect.</p></div></footer>
</body>
</html>
"""


def foot():
    feats = "".join(f'<a href="/ProxyZip/features/{s}/">{t}</a>' for s, t in FEATURES[:4])
    return FOOT % {"repo": REPO, "features": feats}


def breadcrumb(items):
    return json.dumps({
        "@context": "https://schema.org", "@type": "BreadcrumbList",
        "itemListElement": [{"@type": "ListItem", "position": i + 1, "name": n, "item": u} for i, (n, u) in enumerate(items)],
    })


def sidebar(current):
    parts = ['<aside aria-label="Documentation"><div class="g"><p class="h5">Features</p>']
    for s, t in FEATURES:
        cur = ' aria-current="page"' if current == ("features", s) else ""
        parts.append(f'<a href="/ProxyZip/features/{s}/"{cur}>{t}</a>')
    parts.append("</div>")
    for name, slugs in DOC_GROUPS:
        parts.append(f'<div class="g"><p class="h5">{name}</p>')
        for s in slugs:
            if s in DOCS:
                cur = ' aria-current="page"' if current == ("docs", s) else ""
                parts.append(f'<a href="/ProxyZip/docs/{s}/"{cur}>{html.escape(DOCS[s]["title"])}</a>')
        parts.append("</div>")
    parts.append("</aside>")
    return "".join(parts)


def rewrite_links(body, is_doc=True):
    def href(m):
        url = m.group(1)
        if re.match(r"^(https?:|mailto:|#|/)", url):
            return m.group(0)
        path, _, frag = url.partition("#")
        frag = "#" + frag if frag else ""
        if path.endswith(".md") and "/" not in path and path[:-3] in DOCS:
            return f'href="/ProxyZip/docs/{path[:-3]}/{frag}"'
        resolved = (Path("docs") / path).as_posix()
        resolved = re.sub(r"[^/]+/\.\./", "", resolved)
        kind = "tree" if not Path(resolved).suffix else "blob"
        return f'href="{REPO}/{kind}/main/{resolved.lstrip("./")}{frag}"'

    if is_doc:
        body = re.sub(r'href="([^"]+)"', href, body)
        body = re.sub(r'src="(?!https?:|/)([^"]+)"', lambda m: f'src="/ProxyZip/docs/{m.group(1)}"', body)
    else:
        body = re.sub(r'href="(?:\.\./)+docs/([a-z0-9-]+)/(#[^"]*)?"', lambda m: f'href="/ProxyZip/docs/{m.group(1)}/{m.group(2) or ""}"', body)
    return body.replace("<img ", '<img loading="lazy" decoding="async" ')


def write_page(rel, title, description, canonical, main_html, crumbs, current):
    ld = f'<script type="application/ld+json">{breadcrumb(crumbs)}</script>\n'
    crumb_html = " / ".join(f'<a href="{u}">{html.escape(n)}</a>' for n, u in crumbs[:-1]) + f" / {html.escape(crumbs[-1][0])}"
    page = (head(title, description, canonical, ld) + NAV +
            f'<main id="main" class="wrap"><div class="page">{sidebar(current)}<article class="prose">'
            f'<p class="crumbs">{crumb_html}</p>{main_html}</article></div></main>' + foot())
    dest = OUT / rel / "index.html"
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(page, encoding="utf-8")


DOCS = {}


def first_paragraph(text):
    for block in re.split(r"\n\s*\n", text):
        b = block.strip()
        if b and not b.startswith(("#", "<", "|", "```", ">", "-", "1.")):
            return re.sub(r"\s+", " ", re.sub(r"[`*_]|\[([^\]]+)\]\([^)]+\)", lambda m: m.group(1) or "", b))
    return ""


def load_docs():
    for f in sorted((ROOT / "docs").glob("*.md")):
        text = f.read_text(encoding="utf-8")
        m = re.match(r"#\s+(.+)", text)
        title = m.group(1).strip() if m else f.stem.title()
        DOCS[f.stem] = {"title": title, "text": text}


def build_docs():
    for slug, d in DOCS.items():
        text = re.sub(r"^#\s+.+\n", "", d["text"], count=1)
        desc = first_paragraph(text)[:155].rstrip()
        body = rewrite_links(md(text))
        main_html = f"<h1>{html.escape(d['title'])}</h1>{body}"
        url = f"{BASE}/docs/{slug}/"
        write_page(f"docs/{slug}", f"{d['title']} · ProxyZip docs", desc, url, main_html,
                   [("ProxyZip", BASE + "/"), ("Docs", BASE + "/docs/getting-started/"), (d["title"], url)], ("docs", slug))
    src = ROOT / "docs" / "images"
    if src.exists():
        shutil.copytree(src, OUT / "docs" / "images", dirs_exist_ok=True)


def build_features():
    titles = dict(FEATURES)
    for slug, _ in FEATURES:
        meta, text = front_matter((SITE / "content" / "features" / f"{slug}.md").read_text(encoding="utf-8"))
        img = meta.get("image")
        fig = ""
        if img:
            fig = (f'<figure class="shot"><div class="frame"><img src="/ProxyZip/images/{img}-1200.webp" '
                   f'srcset="/ProxyZip/images/{img}-1200.webp 1200w, /ProxyZip/images/{img}.webp 2400w" sizes="(min-width:1240px) 760px, 92vw" '
                   f'alt="{html.escape(meta.get("image_alt", ""), quote=True)}" loading="lazy" decoding="async"></div></figure>')
        body = rewrite_links(md(text), is_doc=False)
        related = "".join(f'<li><a href="/ProxyZip/features/{s}/">{t}</a></li>' for s, t in FEATURES if s != slug)
        main_html = (f'<h1>{html.escape(meta["h1"])}</h1>{fig}{body}'
                     f'<div class="related"><h2 style="border:0;margin-top:0;padding:0;font-size:20px">More you can do</h2><ul>{related}</ul>'
                     f'<p><a class="btn primary" href="/ProxyZip/#download">Download ProxyZip</a></p></div>')
        url = f"{BASE}/features/{slug}/"
        write_page(f"features/{slug}", f"{meta['title']} · ProxyZip", meta["description"], url, main_html,
                   [("ProxyZip", BASE + "/"), ("Features", BASE + "/#features"), (titles[slug], url)], ("features", slug))
        DOCS_URLS.append(url)


DOCS_URLS = []


def copy_static():
    if OUT.exists():
        shutil.rmtree(OUT)
    OUT.mkdir()
    for p in SITE.iterdir():
        if p.name in SKIP or p.name.startswith("."):
            continue
        if p.is_dir():
            shutil.copytree(p, OUT / p.name)
        else:
            shutil.copy2(p, OUT / p.name)
    (OUT / ".nojekyll").write_text("")


def write_sitemap():
    today = date.today().isoformat()
    urls = [BASE + "/"] + DOCS_URLS + [f"{BASE}/docs/{s}/" for s in DOCS]
    items = "".join(f"<url><loc>{u}</loc><lastmod>{today}</lastmod></url>" for u in urls)
    (OUT / "sitemap.xml").write_text(
        f'<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">{items}</urlset>\n')
    (OUT / "robots.txt").write_text(f"User-agent: *\nAllow: /\n\nSitemap: {BASE}/sitemap.xml\n")


def main():
    copy_static()
    load_docs()
    build_features()
    build_docs()
    write_sitemap()
    print(f"built {len(DOCS)} docs, {len(FEATURES)} feature pages into {OUT}")


if __name__ == "__main__":
    sys.exit(main())
