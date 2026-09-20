---
title: "Agent skills"
description: "What the four Keryx skills do, how to install them, and when to use each one."
---

A skill is a directory containing a `SKILL.md` file: instructions an agent can load for a particular task. Keryx's skills teach your agent how to write, publish, read and retire documents using the CLI. They do not run inside the server or install the Keryx executable.

The release archive includes a `skills/` directory. The same files live in [the repository](https://github.com/SimCubeLtd/keryx/tree/main/skills). You can use Keryx manually without installing any skills.

## Which skill does what?

| Skill | Use it when | Result |
| --- | --- | --- |
| `html-communication` | You want a plan, report, proposal or UI mock published as HTML. | A public URL, raw HTML URL and version number. |
| `keryx-read` | You give the agent a Keryx document URL or shared registry reference to read. | The agent reads the actual stored HTML before continuing. |
| `keryx-publish` | You ask for a PDF of a stored Keryx document. | A local PDF exported from an explicitly selected version. |
| `keryx-archive` | You explicitly ask to retire a finished draft. | Optional preservation of the approved HTML in your repository, followed by permanent removal from Keryx. |

## Install the skills

Copy each complete skill directory, including its `SKILL.md`, into a location your agent discovers. Choose user scope for all your projects, or repository scope for one team or project.

| Agent | User scope | Repository scope |
| --- | --- | --- |
| Codex | `~/.agents/skills/` | `.agents/skills/` |
| Claude Code | `~/.claude/skills/` | `.claude/skills/` |

These locations follow the official [Codex skills guide](https://learn.chatgpt.com/docs/build-skills) and [Claude Code skills guide](https://code.claude.com/docs/en/skills). Other agents may use different locations.

For example, a Codex user installation contains:

```text
~/.agents/skills/
  html-communication/SKILL.md
  keryx-read/SKILL.md
  keryx-publish/SKILL.md
  keryx-archive/SKILL.md
```

Copy the four directories from the extracted release's `skills/` folder or the repository checkout. Keep the directory names and files together. If you already have customised copies, compare and merge changes rather than overwriting them.

Then [configure your agent's main instructions](../agents/#set-up-agentsmd-or-claudemd). Installing a skill makes its procedure available; your main instructions define when you want the agent to use it.

## html-communication

This skill writes self-contained HTML and uploads it with `keryx upload`. It includes responsive document styling, static UI variants and structure that can later be exported as PDF.

For an existing document, it fetches or reuses the working HTML and uploads with `--draft <id>`. Revisions keep the same draft URL. Uploads run from the repository directory so Git provenance can be recorded.

Example request:

```text
Use html-communication to write and publish a release plan for this project.
Return the document URL.
```

The bundled skill includes standing permission to upload documents it creates. Review that policy and its styling defaults before adopting it. This is for agent deliverables and mockups, not HTML that ships as application code.

[Read the skill source](https://github.com/SimCubeLtd/keryx/blob/main/skills/html-communication/SKILL.md).

## keryx-read

This skill reads the stored HTML through `keryx raw`, including a specific version when the URL names one. It treats the document as content to review, not instructions to execute. If a review leads to changes, it preserves the draft ID for the next upload.

It also understands explicit OCI version references. It can inspect a registry artifact and save its HTML without importing it into your Keryx server. Importing is a separate user request.

Example request:

```text
Use keryx-read to review this Keryx draft URL and identify missing rollout steps:
<paste the document URL here>
```

[Read the skill source](https://github.com/SimCubeLtd/keryx/blob/main/skills/keryx-read/SKILL.md).

## keryx-publish

This skill exports a stored draft to PDF. It resolves the version first, passes that version explicitly to `keryx publish`, and checks the resulting file. It does not change the source document or create another draft version.

Example request:

```text
Use keryx-publish to export version 3 of this draft to ./release-plan.pdf:
<paste the document URL here>
```

The skill does not replace an existing output file or act as a general HTML-to-PDF converter. See [PDF publishing](../pdf/) for supported content.

[Read the skill source](https://github.com/SimCubeLtd/keryx/blob/main/skills/keryx-publish/SKILL.md).

## keryx-archive

This skill retires a named draft only when you ask. If you want to retain a copy, it fetches the approved version, writes its exact HTML into the repository and reads the file back before purging the draft.

Example request:

```text
Use keryx-archive to save the approved version 3 of this draft in the repository,
then retire it from Keryx:
<paste the document URL here>
```

**Retirement permanently removes the draft and all its versions from Keryx.** Saving a repository copy is optional; it does not preserve the server's complete version history. Merely finishing implementation is not permission to purge a draft.

[Read the skill source](https://github.com/SimCubeLtd/keryx/blob/main/skills/keryx-archive/SKILL.md).

## Adapt the bundled defaults

The shipped instructions assume the default local Keryx API at `http://localhost:7812` and a working directory under `/tmp/keryx`. They tell the agent not to configure authentication or pass API URL overrides while running a skill.

Configure the client connection yourself before use. If you use a remote server, a different working directory or Windows-native paths, adapt your installed skill copies to that setup. A `/tmp` directory is working storage, not a durable archive; persistence depends on your machine and agent environment.

The HTML skill also includes document styling preferences. Change those in your installed copy or give explicit instructions that override them. The [main-instructions example](../agents/#example-main-instructions) shows one complete workflow and style, not a required Keryx configuration.
