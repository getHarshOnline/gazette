# Write with Gazette in VS Code

Gazette checks the pages and configuration of a publication while you write. It supplies diagnostics, completion, outlines and navigation using the same native program that checks and builds your site.

## Connect your publication

1. Install the VSIX for your host's operating system and architecture through **Extensions → Install from VSIX…**. For a remote workspace, use the package for the remote host.
2. Open the folder containing your publication's `gazette.toml` and trust the folder if you know its contents.
3. Open `gazette.toml` or one of its configured pages. Gazette selects the publication's files from its configuration, including sources outside the open folder. The package includes the native program; no separate executable setting is needed.

This extension works with an existing Gazette publication. It does not create a publication, content folder or theme for you.

## Write and check

Open **Problems** to see Gazette's findings and select a finding to reach its source. In a page's frontmatter, invoke **Trigger Suggest** to see supported fields and publication references. Hover over a supported field or reference for its meaning. Use **Outline** for page headings and **Go to Definition** on supported publication references.

A new page beneath a configured content source receives feedback when you open it. Gazette checks unsaved edits too: you can correct a configuration error without restarting the server or losing unsaved article text.

Run commands from the **Command Palette**. **Gazette: Check Saved Site** checks saved inputs; save your edits first when you want that check to include them. **Gazette: Run Command…** lists the native application operations. Review the selected operation's inputs and any confirmation before running it. Editor feedback does not publish your work.

## Recover a connection

If startup fails, the notification offers **Choose Gazette Program**, **Open Settings** and **Show Output**. The output explains which program Gazette tried to start and why the connection failed. After correcting the program or settings, run **Gazette: Restart Server**.

To use another Gazette executable, set **Gazette: Server Path** to its absolute path in **User Settings** on the computer or remote host running the extension. It must support native editor discovery. Clear that setting to return to the bundled program.

Folders without the selected configuration stay inactive. For a configuration named differently from `gazette.toml`, set **Gazette: Workspace Configuration** to its path relative to the open folder. Leave the fixed file-list settings empty to use automatic discovery. If two open Gazette folders include the same pages, turn off **Gazette: Authoring Enabled** for the folder that should not supply their editor feedback.

## Licence

Gazette is licensed under AGPL-3.0-or-later. The package includes its licence, native program notices and notices for the bundled editor dependencies.
