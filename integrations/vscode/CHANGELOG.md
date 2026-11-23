# Changelog

## 0.1.9 — Unreleased

- Include a native Gazette program in each platform package, with an optional override in User Settings.
- Discover publication pages and configuration from `gazette.toml`, including configured sources outside the open folder and newly opened pages beneath a content source.
- Keep unsaved page text and the language-server connection while a configuration error is corrected.
- Export and inspect discovered workspaces using saved-file hashes and only unsaved text, retaining clean open versions and refusing changed saved inputs. Native reconstruction requires separate receiving-side read permission.
- Bundle the editor client while preserving dependency licences and its process-termination helper.
- Include installation, writing and recovery help, and retain a Field Notes screenshot for release preparation.

This entry describes local release preparation. Platform qualification and publication are separate steps.
