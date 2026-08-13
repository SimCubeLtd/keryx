---
name: keryx-archive
description: Retire a finished Keryx draft. Optionally save its approved version into the repo as a self-contained HTML file, then hard-purge the draft and all its versions from the Keryx server. Use when the user says a plan is finished, done or implemented, or asks to archive, retire, clean up or purge a Keryx draft.
---

# Keryx Archive

Retire a Keryx draft once its work is finished. Optionally keep a copy in the
repo, then remove the draft and every version of it from the server.

Run only when the user asks. Never start this because a plan looks complete.
The purge is irreversible and is not a call to make on your own initiative.

## Steps

1. Resolve the draft ID and the version to keep. The ID is the segment after
   `/d/` in the URL the user gives; `keryx list` finds it otherwise. Name the
   approved version explicitly rather than assuming the latest is it. If which
   version was approved is unclear, ask.

2. If the user wants it archived, decide where it goes before writing anything.

   Layout follows the repo. Look for an existing plan-archive convention: a
   directory such as `docs/decisions/plan-archive/` or `docs/plans/`, or
   whatever `docs/index.md` or `CONTRIBUTING.md` points at. If one exists, put
   the file there and match the neighbouring files' naming style, changing only
   the extension. Alongside `FooBar.plan.md` files, write `NewThing.plan.html`.
   If the repo has no such convention, create `docs/plans/` and name the file
   `<YYYY-MM-DD>-<slug>.html`.

   Format does not follow the repo. The archive is always the HTML, even when
   the neighbouring files are markdown.

   Then write the approved version:

   ```sh
   keryx raw <draft-id> -v <n> > <path>
   ```

   Copy the bytes. Do not reformat, convert to markdown, summarise, or add
   frontmatter. The archived file must be identical to what was approved,
   because it is the record of what was agreed. Converting it would file a
   re-authoring of the record in place of the record.

3. Read the written file back. Confirm it is non-empty and ends with a closing
   `</html>`. If the read fails or the file looks truncated, stop and report.
   Do not purge.

4. Purge the draft and all of its versions:

   ```sh
   keryx delete <draft-id> --purge --yes
   ```

   `--yes` is required because there is no interactive terminal. The read-back
   in step 3 is the safety gate, so never reorder these two steps.

5. Report the archived path, if any, and the purged draft ID.

Leave the archived file unstaged. Do not run git or GitButler commands, do not
stage, and do not commit. Staging and the commit message belong to the
developer.

## Purging without archiving

Archiving is optional. Purging is the point of this skill and always happens.

When the user declines the archive, the draft is the only copy of that plan and
it is about to be gone. Do not block on a prompt, but say so plainly in the
result, for example: `Purged c6o35xkyp210. No copy retained.`

## Never

- Never run bare `keryx purge`. It hard-deletes every soft-deleted draft on the
  server, not only this one. That is the user's command, not yours.
- Never purge before the archive has been written and read back.
- Never purge a draft the user has not named.
- Never purge as a cleanup step tacked onto some other task.
- Never convert the archive to markdown or any other format to match the files
  around it. Take the directory and the naming style from the repo, never the
  format.

Use Keryx's default local API at `http://localhost:7812`. Do not configure
authentication or pass an API URL override. If a command fails, report its
actual error and leave the draft in place.
