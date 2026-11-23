#!/usr/bin/env python3
"""Exercise the persistent starter through an explicitly supplied Gazette build."""

import argparse
import html
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


EXAMPLE = Path(__file__).resolve().parent


def render(binary, workspace, stages):
    result = subprocess.run(
        [str(binary), "render-site", "--local", "--site", "site:field-notes",
         "--model-output", "model.json", "--output-parent", str(stages)],
        input=workspace.read_text(), text=True, capture_output=True,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip())
    directory = Path(json.loads(result.stdout)["directory"])
    return directory, json.loads((directory / "model.json").read_text())


def check_manual(binary, workspace, stages, work):
    documentation = f"""[brands.publications.documentation_repository]
name = 'notes-code-docs'
path = {json.dumps(str(work / 'notes-code-docs'))}
public_base_url = 'https://docs.field-notes.example/'
[brands.publications.documentation_repository.build]
target = {{ kind = 'mintlify', theme = 'mint', primary_color = '#315c4a' }}
[[brands.publications.documentation_repository.build.modules]]
project = 'project:notes-tool'
slug = 'notes-tool'
title = 'Notes tooling'
manual = {{ source = 'source:notes-tool-manual' }}
navigation = [{{ kind = 'manual', title = 'Build an integration', pages = [
  {{ title = 'Notes tooling', route = 'notes-tool', input = {{ kind = 'file', path = 'index.md' }} }},
  {{ title = 'Note identity and addresses', route = 'notes-tool/read-a-note', input = {{ kind = 'file', path = 'reading-notes.md' }} }},
] }}]

"""
    declarations = f"""
[[projects]]
id = 'project:notes-tool'
name = 'Notes tooling'
lifecycle = 'active'
visibility = 'public'
availability = 'unavailable'
[[sources]]
id = 'source:notes-tool-manual'
location = {{ kind = 'filesystem', root = {json.dumps(str(EXAMPLE / 'documentation/manual'))} }}
"""
    source = workspace.read_text().replace("[[sources]]", documentation + "[[sources]]", 1) + declarations
    result = subprocess.run(
        [str(binary), "assemble-docs", "--local", "--site", "site:field-notes", "--output-parent", str(stages)],
        input=source, text=True, capture_output=True,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip())
    directory = Path(json.loads(result.stdout)["directory"])
    index = (directory / "notes-tool.md").read_text()
    manual = (directory / "notes-tool/read-a-note.md").read_text()
    assert "read-a-note#stable-note-identity" in index
    assert "[[id:content:keep-the-context|original observation]]" in manual
    assert "slug = 'keep-the-context'" in manual
    assert "https://field-notes.example/writing/rename-a-note/#check-the-renamed-note" in manual


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gazette", required=True, type=Path)
    args = parser.parse_args()
    binary = args.gazette.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="gazette-first-publication-") as name:
        work = Path(name)
        publication = work / "publication"
        subprocess.run(["python3", str(EXAMPLE / "create.py"), str(publication)],
                       capture_output=True, text=True, check=True)
        refused = subprocess.run(["python3", str(EXAMPLE / "create.py"), str(publication)],
                                 capture_output=True, text=True)
        assert refused.returncode != 0, "the creator must preserve an existing publication"
        stages = work / "stages"
        stages.mkdir()
        workspace = publication / "gazette.toml"
        first, initial = render(binary, workspace, stages)
        for identity, path in [("field-notes", "/"), ("keep-the-context", "/keep-the-context/"), ("review-a-note", "/review-a-note/")]:
            page = initial["pages"][f"content:{identity}"]
            assert page["content"]["site"] == "site:field-notes"
            assert page["route"]["url"] == f"https://field-notes.example{path}"
        first_html = (first / "keep-the-context/index.html").read_text()
        assert 'href="https://field-notes.example/review-a-note/"' in first_html
        assert 'href="https://field-notes.example/keep-the-context/"' in (first / "review-a-note/index.html").read_text()
        assert len(re.findall(r"<h1(?:\s|>)", first_html)) == 1
        assert "article-credit" not in re.sub(r"<style>.*?</style>", "", first_html, flags=re.S)
        assert '<summary>Cite this article</summary>' not in first_html
        assert '<summary>Notes</summary>' not in first_html

        workspace.write_text(workspace.read_text().replace(
            "kind = 'person'", "kind = 'person'\nname = 'A writer <&>'\n\n[[principals]]\nid = 'owner:holder'\nkind = 'organization'\nname = 'Example rights holder'", 1
        ).replace(
            "[[sources]]", "[brands.publications.machine]\njson_ld = true\n\n[[sources]]", 1
        ))
        article = publication / "content/keep-the-context/index.md"
        prefix, metadata, body = article.read_text().split("+++", 2)
        metadata = metadata.replace("language = 'en'", "language = 'en'\nlicense = 'https://example.test/writing-license/'")
        metadata = metadata.replace("role = 'author' }]", "role = 'author' }, { principal = 'owner:holder', role = 'rights-holder' }]")
        metadata += "\n[content.citation]\ntext = 'A citation <&> for this note.'\nbibtex = 'reference.bib'\n"
        article.write_text("+++".join([prefix, metadata, body]))
        bibtex = "@misc{context, title={Keep the context}}\n"
        (article.parent / "reference.bib").write_text(bibtex)
        (article.parent / "notes.md").write_text("# Comparing observations\n\nAn extended explanation used only in this note.\n")
        second, model = render(binary, workspace, stages)
        page = model["pages"]["content:keep-the-context"]
        document = (second / "keep-the-context/index.html").read_text()
        markup = document.split("<main>", 1)[1].split("</main>", 1)[0]
        assert len(re.findall(r"<h1(?:\s|>)", markup)) == 1
        assert markup.index("<h1") < markup.index('class="article-credit"') < markup.index("Write what you noticed")
        assert "By A writer &lt;&amp;&gt;" in markup
        assert 'Rights holder: Example rights holder' in markup
        assert 'class="article-citation"' in markup and '<summary>Cite this article</summary>' in markup
        assert 'class="article-notes" id="article-notes"' in markup and '<summary>Notes</summary>' in markup
        assert markup.count("An extended explanation used only in this note.") == 1
        assert '<details class="article-citation" open' not in markup and '<details class="article-notes" id="article-notes" open' not in markup
        assert '<p class="citation-text">A citation &lt;&amp;&gt; for this note.</p>' in markup
        assert page["citation"]["bibtex"] in html.unescape(markup)
        assert (second / "keep-the-context/reference.bib").read_text() == bibtex
        assert '>Content licence</a>' in markup and page["content"]["license"] in html.unescape(markup)
        data = re.search(r'<script type="application/ld\+json">(.*?)</script>', document, re.S)
        assert data and json.loads(data[1]) == page["json_ld"]
        workspace.write_text(workspace.read_text().replace(
            "[composition]", "[composition]\npage_templates = { 'content:keep-the-context' = 'field-notes/reading.html' }", 1
        ))
        third, _ = render(binary, workspace, stages)
        reading = (third / "keep-the-context/index.html").read_text()
        assert 'font-family: Georgia, serif' in reading
        assert '<summary>Cite this article</summary>' in reading and '<summary>Notes</summary>' in reading
        assert reading.count('An extended explanation used only in this note.') == 1
        assert (first / "keep-the-context/index.html").read_text() == first_html
        assert article.exists() and (publication / "preview-request.json").exists()

        # Follow the title-edit tutorial and include its companion help pages.
        article.write_text(article.read_text().replace(
            "Keep the observation with its context", "Context worth keeping"
        ))
        for name in ("writing", "rename-a-note"):
            shutil.copytree(EXAMPLE / "documentation/product" / name, publication / "content" / name)
        fourth, renamed = render(binary, workspace, stages)
        page = renamed["pages"]["content:keep-the-context"]
        assert page["content"]["title"] == "Context worth keeping"
        assert page["route"]["url"] == initial["pages"]["content:keep-the-context"]["route"]["url"]
        assert "Context worth keeping" in (fourth / "index.html").read_text()
        for identity, path in [("field-notes-writing-help", "/writing/"), ("field-notes-rename-a-note", "/writing/rename-a-note/")]:
            page = renamed["pages"][f"content:{identity}"]
            assert page["route"]["url"] == f"https://field-notes.example{path}"
            assert (fourth / path.strip("/") / "index.html").exists()

        # A deliberate slug change updates incoming identity links without editing callers.
        caller = publication / "content/review-a-note/index.md"
        caller_before = caller.read_bytes()
        article.write_text(article.read_text().replace("slug = 'keep-the-context'", "slug = 'context-worth-keeping'"))
        fifth, moved = render(binary, workspace, stages)
        assert moved["pages"]["content:keep-the-context"]["route"]["url"] == "https://field-notes.example/context-worth-keeping/"
        assert (fifth / "context-worth-keeping/index.html").exists()
        assert not (fifth / "keep-the-context/index.html").exists(), "slug changes do not invent aliases"
        assert 'href="https://field-notes.example/context-worth-keeping/"' in (fifth / "review-a-note/index.html").read_text()
        assert caller.read_bytes() == caller_before
        assert (first / "keep-the-context/index.html").read_text() == first_html
        check_manual(binary, workspace, stages, work)
    print("PASS: persistent create/refusal; derived starter/help addresses and source-bound sites; stable ID links through title and slug changes; assembled manual pages and heading link; one heading and body; escaped visible authorship, rights holder and citation; retained BibTeX; one native Notes disclosure; selected embedded JSON-LD; reading variant; earlier output unchanged")


if __name__ == "__main__":
    main()
