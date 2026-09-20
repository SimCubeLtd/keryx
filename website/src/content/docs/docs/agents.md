---
title: "Connect your agent"
description: "Teach your agent to publish, read and export Keryx documents."
---

Your agent needs access to the Keryx executable, a reachable Keryx server and instructions for writing and publishing HTML.

## Configure the connection

The CLI defaults to `http://localhost:7812`. For another server, set `client.api_url` in [the configuration file](../configuration/), use `KERYX_API_URL`, or pass `--api-url` to a command.

If the server requires an API key, store it with `keryx auth set <api-key>`. Keryx verifies the key and saves it in the client's credentials file. Keep real credentials out of your agent instructions and repository.

## Install the supplied skills

Follow the [skills guide](../skills/) to install the four Keryx skills and review their behaviour. Your agent needs access to the executable and server from the environment where it runs, not just from your own terminal.

## Set up AGENTS.md or CLAUDE.md

Skills explain how to carry out a task. Your main instruction file tells the agent when to choose them, whether to publish automatically, and where to stop for your review.

| Agent | Personal instructions for all projects | Project instructions |
| --- | --- | --- |
| Codex | `~/.codex/AGENTS.md`, or `AGENTS.md` under your custom `CODEX_HOME` | `AGENTS.md` in the repository root |
| Claude Code | `~/.claude/CLAUDE.md` | `CLAUDE.md` in the repository root |

Use personal instructions for your own default workflow, or project instructions when the whole team should follow it. Merge the example below into your existing file; do not replace unrelated instructions. Keep skill installation separate: pasting a skill name into this file does not install it.

See the official [Codex instruction guide](https://learn.chatgpt.com/docs/agent-configuration/agents-md) and [Claude Code memory guide](https://code.claude.com/docs/en/memory) for discovery and scope rules.

## Example main instructions

The following is Prom's example, reproduced verbatim. It makes HTML publishing the default for substantial written work, requires a mockup pick before implementation, and gives documents a specific visual style.

These are personal workflow choices, not Keryx requirements. Adapt the style section to your preferences. The plan-mode paragraph mentions a particular agent environment and `ExitPlanMode`; retain or adapt it only if your agent provides that workflow. The word `blue` in the animation list is preserved from the original example.

```md wrap
# Visual and design work

- Do not edit real components first. For any non-trivial UI, layout or copy change, build several distinct static mocks, publish them with 'html-communication' skill, report the URL and stop. Wait for a pick before implementing.
- Mocks follow the target product's design system and the look the feature needs, not my document style. If the app is light-themed, mock it light.
- When asked to review a keryx plan, use the 'keryx-read' skill.
- Avoid continuously repainting CSS animations (pulse, shimmer, blue, spinners); they peg the GPU on high-refresh displays.

# Plans and written deliverables

- Plans, specs, reviews, findings, comparisons and reports are delivered as HTML via the 'html-communication' skill and published to Keryx. Report the URL. This is the default output format. I should not have to ask for it.
- Plan mode: the harness will tell you the .md plan file is the only file you may edit. Obey that, call ExitPlanMode as normal, then publish the same plan as HTML in your next turn without being asked. The .md is scaffolding, not the deliverable.
- Outside plan mode, skip the .md and go straight to HTML.
- Exception: an answer that fits in a few paragraphs of chat stays in chat. If it has sections, a table, or a file list, it is a document.
- When a plan's implementation is merged, offer to retire its Keryx draft with the 'keryx-archive' skill. Offer only. Never archive or purge unprompted.

# Style for documents I write about the work

Applies to plans, specs, reviews, findings and reports published with 'html-communication'. Does NOT apply to UI mocks of a product, or to any real application code.

- Dark mode, true black (#000) background, white primary text, gray secondary.
- Typeset, not rendered markdown: display-scale page title with tight letter-spacing, uppercase letter-spaced kicker labels, hairline-rule grid layouts instead of stacked prose, and a header meta block. Long documents get a sticky jump nav.
- One recurring identity accent color per document is welcome on kickers, badges, nav highlights, and scores. Not a license for rainbow palettes.
- Information-dense, minimal copy, no marketing voice, no light-gray subtitle lines above sections. No em dashes. No marketing hero (vague tagline, CTA button); a typeset document header with title, one-line lede, and meta block is not a hero.
- Structure earns emphasis. Callouts that carry weight (traps, risks, verified facts, caveats) may use a bordered panel, a colored left rule, and a short uppercase label. Sparingly, and only where the emphasis is load-bearing.
- Semantic accent colors carry meaning: amber for risk, green for verified. A couple per document at most, on top of the identity accent.
- What is banned is chrome for its own sake: ordinary prose wrapped in cards, gradients, rounded pills as ornament, drop shadows, icon garnish.
- No continuously repainting CSS animations (pulse, shimmer, spinners).
```

## Check the setup

Ask your agent to use `html-communication` to publish a small test document. It should return the public URL, raw HTML URL and version number. Give that URL back and ask it to use `keryx-read` to read the document. A revision should add a version to the same draft.

If the agent cannot find a skill, check that its directory contains `SKILL.md` in the correct discovery location. If it cannot reach Keryx, check the CLI connection from the agent's environment. Review [the bundled defaults](../skills/#adapt-the-bundled-defaults) when using a remote server or different filesystem layout.

## A minimal publishing instruction

For agents without skill support, start with this instruction:

```text
Write the deliverable as a complete, self-contained static HTML file.
Use inline CSS and semantic HTML. Follow the server's HTML policy.
Run keryx upload '<file>' from the repository directory.
Return the public URL, raw HTML URL and version number to me.
When revising an existing draft, use --draft '<draft-id>'.
Do not create another draft for the same document.
```

Run the upload from the project directory so Keryx can record the repository, branch and commit. Provenance is metadata, not an access-control mechanism.

## Read and revise

Use `keryx raw <draft-id>` to fetch current HTML before editing. Use `--version <number>` when you need a specific version. Upload the revised file with `--draft <draft-id>` to preserve the document's history.

See the [HTML policy](../html-policy/) for upload restrictions and [PDF publishing](../pdf/) for document structure that exports well.
