Hi, Im Dave (call me Prom). You're my agent. We will be working together a lot so I thought it'd be nice to introduce myself.

I'm known for being a technological architect / evangelist. I'm co-Owner / CTO of SimCube Ltd.

I love to build and innovate. I focus on building complex things as simple as possible. I love to find ways to reduce complexity when solving problems.

I wanted to share some of my preferences here so we can be more aligned while we work together.

# Code Preferences - general

- Keep things simple. Channel "yagni" energy unless told otherwise.
- Typesafety is useful - take advantage of it.
- Don't be scared to propose bold ideas if they can be meaningfully beneficial to our work.
- Be careful with Destructive actions that are not explicitly requested by the user.
- Tests are good! Endless smoke tests, "regression tests" for feature deletions etc, mush less good. Tests should be focused not slop.
- Comments are a great way to clarify functionality and how code is used. Don't comment every line, but feel free to (concisely) describe how functions are used above function definitions, classes, etc.
- Keep comments up to date! When making changes it's important to keep things in sync.

# Coding Preferences - typescript related

- 'any' is the enemy. Inferred types are our friend.Our system should adapt to changes, instead of requiring changes everywhere.
- If your TS code looks like a Python dev wrote it, it is bad TS code.
- Avoid one line functions that are just casting wrappers.
- Write Typescript in ways that Matt Pocock would be proud of.
- If not already specified in the project, I generally like to use the following tech: Tailwind, React, Vite, Pnpm, Astro (and Starlight)
- When building more complex web, tauri and react native apps, I like to pull in Zustand, Tanstack Query, Tanstack Start, Better-Auth and ArkType (or zod if perf isn't an issue)

# Questions are read-only

- A question is a request for an answer, not for changes. If the message opens with "how hard would it be", "what are your thoughts", "why does", "should we", "is it possible", "can X do Y" or otherwise asks rather than instructs: answer it, and do not edit files.
- If the answer is obvious and the change is trivial, still answer first and offer the change. Ask before making it.

# Match ceremony to the task

- Do not spawn subagents or a multi-agent panel for work a single agent finishes in one pass. Delegation is for breadth or adversarial review, not for ordinary tasks.
- When several agents do work in parallel, state file ownership up front so they do not collide

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

# Blast radius

- Never touch production, live databases, or daily driver build/preview channels unless explicitly told to. When a task is adjacent to any of them, name what you are about to touch before touching it.

## Version control

- Use GitButler (`but`) for version-control inspection and write operations, including status, diffs, branching, committing, pushing, and history edits.
- Assume multiple agents may be working in this repository. Do not move, amend, squash, discard, commit, push, or otherwise modify another agent's work unless the user asks.
- For commit just/only/specific changes on a new branch (selected-change requests), use the two-command fast path from the GitButler skill: `but diff`, then `but commit -b <branch> -m "message" <id> <id>`.
- For that fast path, after the commit succeeds, stop and summarize; do not run separate branch, staging, status, or diff commands unless the commit output is missing information you need.
- Use the installed GitButler skill for command recipes and syntax before guessing flags, using `--help`, or translating Git habits directly.
- Mutation commands report their result without appending workspace status. Add `--status-after` only when the next step needs resulting workspace IDs or details; otherwise do not rerun status or diff to verify success.
- Use a dedicated GitButler branch for each agent session, unless the user asks for a different branch structure. Commit only changes that belong to that session.
- Do not push or open pull requests unless the user asks.
- Keep commit messages and pull request descriptions succinct: explain what changed, why it changed, and any important decision.
- Do not want you to include any CoAuthor entries, or trailers when writing commit messages.

### Amend local fixes into the right commits

- For small cleanup or follow-up fixes, amend an unpublished local commit when the change clearly belongs with that commit's intent.
- Do not create tiny fixup commits unless the user asks.
- Use GitButler to move the relevant changes into the commit where they belong.
- Ask before rewriting pushed, reviewed, shared, or ambiguous history.

### Split unrelated changes into separate commits

- If one file contains unrelated changes, split them by hunk instead of committing the whole file.
- Keep tests with the behavior they verify.
- Split generated output, docs-only edits, or mechanical cleanup into separate commits when each commit remains coherent on its own.
- If the split is ambiguous, summarize the options before committing.

### Create stacked pull requests

- If this session depends on another in-flight branch, stack its branch on top of that dependency instead of mixing the changes.
- If this session is working in a stack, put commits on the branch where they belong.
- Ask before moving commits onto lower, pushed, reviewed, or shared branches.
- Use `but move` for branch stacking and restacking. Do not recreate branches to simulate stacking.
- For stacked branches, create pull requests with `but pr`, not `gh`, so GitButler keeps the right PR base branches and stack metadata.

### Update from the target branch automatically

- When GitButler status shows new changes on the target branch and the workspace holds only this session's branches, update with `but pull` directly — its output reports the result and `but undo` reverts it.
- If an update you started on your own initiative reports conflicted commits, stop and ask before resolving them (`but undo` reverts the pull if the user prefers).
- When other agents' branches are applied, run `but pull --check` first and ask before updating if it reports conflicts or their branches would move.
- If the user asks you to handle update conflicts, use GitButler's conflict tools. Ask before resolving semantic conflicts, dependency updates, generated files, or conflicts involving another person's work.

### Open draft pull requests by default

- When asked to open a pull request, create it as a draft with GitButler unless the user says it is ready for review.
- Remember that creating a draft pull request still publishes the branch.

# Stop points

When I give you a stop point, stop there. Do not commit, push, or deploy past it.
