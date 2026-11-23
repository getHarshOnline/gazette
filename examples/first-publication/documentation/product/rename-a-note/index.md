+++
collections = ['content:field-notes-writing-help']

[content]
id = 'content:field-notes-rename-a-note'
title = 'Rename a note'
description = 'Change a note title, then check its page and collection entry.'
lifecycle = 'draft'
language = 'en'
slug = 'rename-a-note'
attributions = [{ principal = 'owner:field-notes', role = 'author' }]
structure = { kind = 'page', family = { kind = 'documentation' } }
+++

# Rename a note

You can improve a note's title without changing its address. Open `content/keep-the-context/index.md` and change its title to:

```toml
title = 'Context worth keeping'
```

Change its heading to match:

```markdown
# Context worth keeping
```

Keep the identity, slug and reading relationships. If you are writing a tool that changes these files, read the [note-format explanation](https://docs.field-notes.example/notes-tool/read-a-note#stable-note-identity) for the distinction that tool needs to preserve.

If you added a citation for this article, update its title in the citation text and any BibTeX file too. Those are authored text rather than generated titles.

## Check the renamed note

Save the article and wait for the running preview to rebuild. Refresh its page: the heading should read **Context worth keeping**, and the collection or journey that contains it should use that same title. Its existing address still works.

Follow the note's onward link and return through the collection or journey. The title changed; the reading path did not.
