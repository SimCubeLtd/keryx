---
title: "Versions and links"
description: "Keep one draft through revisions and link to immutable versions."
---

A draft has a stable ID and a sequence of versions. Re-uploading the same file path adds a version to the same draft. To remove ambiguity across file moves or sessions, target its ID:

```sh
keryx upload ./revised-plan.html --draft <draft-id>
```

Use `--new` only when you want a separate document.

## Link to the right version

| Path | Meaning |
| --- | --- |
| `/d/<id>` | Latest HTML |
| `/d/<id>/raw` | Latest raw HTML |
| `/d/<id>/v/<n>` | A specific version |
| `/d/<id>/v/<n>/raw` | Raw HTML for a specific version |

Both public and raw routes return the stored HTML bytes. A version's content does not change, but disabling or deleting the draft can make its links unavailable.

## Inspect a draft

```sh
keryx list
keryx open <draft-id>
keryx raw <draft-id> --version 2
```

Use `keryx tui` for a terminal browser and version history. The web dashboard at your server's root also shows drafts and their versions.

## Provenance

Uploads record best-effort Git metadata from the directory where you run the command: repository, branch, commit and dirty state. Run uploads from the project checkout when this context matters.

## Export a fixed version

```sh
keryx publish --id <draft-id> --version 2 --output ./plan-v2.pdf
```

See [PDF publishing](../pdf/) for formatting requirements.
