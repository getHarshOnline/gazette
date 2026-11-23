# Note identity and addresses

An editor or note-processing tool needs to distinguish the note itself from its display text and public address.

## Stable note identity

For example, the starter's first note declares:

```toml
collections = ['content:field-notes']

[content]
id = 'content:keep-the-context'
title = 'Keep the observation with its context'
slug = 'keep-the-context'
```

This excerpt shows the fields relevant to identity and address; keep the note's other declarations. Use `id` to recognize the same note across title or address changes and `title` as display text. The configured content source supplies its site. The publication selects `content:field-notes` as its home at `/`, so the note's collection placement and slug produce `https://field-notes.example/keep-the-context/`.

Read the resolved page's address from Gazette's model when integrating it. A source declaration need not contain `route.path`: routing combines the publication's declared roots, the page's slug and its editorial parent. An explicit route remains available for a deliberate exception. Do not reconstruct an address from a title, file name or content ID.

Changing the title leaves the identity and address unchanged. The article's leading heading should agree with its title, so change both display values while preserving the other declarations and body.

Prose links such as `[[id:content:keep-the-context|original observation]]` resolve the identity to its current address. A deliberate slug or collection-placement change can change that address without editing incoming identity links. An already released address still needs an explicit alias if readers should continue to reach it; changing a slug does not create that alias automatically.

[Return to the author task](https://field-notes.example/writing/rename-a-note/#check-the-renamed-note) to save and inspect the renamed note in the publication.
