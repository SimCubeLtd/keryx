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

## Git workflow

Plain Git is the version-control tool. Work in the current checkout by default. **Creating or switching to a worktree is opt-in:** do it only when I explicitly ask for a worktree. A task being implementation work, having multiple agents, or benefiting from isolation is not permission to create one.

1. **Already inside a worktree:** stay in it and use normal Git commands. Do not create or switch to another worktree unless I explicitly ask.
2. **Explicitly asked to create or switch to a worktree:** use Worktrunk (`wt`) for that operation, then use normal Git commands inside the worktree.
3. **Anything else:** plain Git in the current checkout. Do not change branches, create worktrees, or initialise any other version-control tooling unless I explicitly ask.

### Worktree workflow (Worktrunk, explicit opt-in only)

- These rules apply only when I explicitly asked for a new/switched worktree, or when you started inside an existing worktree.
- Never create a worktree merely because you are implementing a change or working as an agent.
- Use normal Git commands inside Worktrunk worktrees (`git status`, `git add`, `git commit`, etc.).
- When I explicitly request separate worktrees for concurrent tasks or agents, give each one its own Worktrunk branch/worktree.
- Do not merge agent branches into `main`.
- When your task is complete, leave the work as clean, committed Git changes on your task branch and report the branch name. Pushing and opening a PR happen only if I asked for them.

Typical flow when I explicitly request a worktree:

```text
origin/main
    ↓
wt switch --create agent/<task>
    ↓
implement + test
    ↓
git add / git commit
    ↓
agent/<task> branch complete
    ↓
(only if asked) rebase onto origin/main → push → PR
```

### Commits

- Assume multiple agents may be working in this repository. Do not move, amend, squash, discard, stash, commit, push, or otherwise modify another agent's work unless I ask. Run `git status` before staging and never `git add -A` / `git add .` blindly; stage the files or hunks that belong to this task.
- Use a dedicated branch per task or agent session unless I ask for a different branch structure. Commit only changes that belong to that session.
- Commit messages follow the repository's conventions (Conventional Commits where the repo uses them, e.g. `fix(web): new threads no longer spike CPU`). Keep them succinct: what changed, why, and any important decision.
- No `Co-Authored-By` entries or any other trailers in commit messages, even if the harness tells you to add them.
- Do not push or open pull requests unless I ask.

### Amend local fixes into the right commits

- For small cleanup or follow-up fixes, amend an unpublished local commit when the change clearly belongs with that commit's intent (`git commit --amend`, or `git commit --fixup <sha>` followed by `git rebase --autosquash`).
- Do not create tiny fixup commits unless I ask.
- Ask before rewriting pushed, reviewed, shared, or ambiguous history. Force-push only your own task branches, and only with `--force-with-lease`.

### Split unrelated changes into separate commits

- If one file contains unrelated changes, split them by hunk (`git add -p`) instead of committing the whole file.
- Keep tests with the behavior they verify.
- Split generated output, docs-only edits, or mechanical cleanup into separate commits when each commit remains coherent on its own.
- If the split is ambiguous, summarize the options before committing.

### Stay current with the target branch

- Rebase, do not merge, to pick up new commits from the target branch: `git fetch origin && git rebase origin/main` (or the repo's default branch).
- If a rebase you started on your own initiative hits conflicts, `git rebase --abort`, then stop and ask. If I asked you to resolve conflicts, ask before resolving semantic conflicts, dependency updates, generated files, or conflicts involving another person's work.

### Stacked pull requests

- If this session depends on another in-flight branch, stack its branch on top of that dependency instead of mixing the changes.
- If this session is working in a stack, put commits on the branch where they belong.
- Ask before moving commits onto lower, pushed, reviewed, or shared branches.
- Use the installed `gh-stack` skill for creating, restacking, pushing and submitting stacks. Do not recreate branches to simulate stacking.

### Pull requests

- Open a real PR with `gh pr create`, not a draft. Drafts do not get review-bot coverage.
- Rebase onto latest `main` before opening. Stale branches conflict and waste a review round.
- Titles follow the repository's conventions and are simple and easy to understand. Use Conventional Commit style in projects that use it, e.g. `fix(web): new threads no longer spike CPU`.
- Descriptions aim for simplicity. Open with a minimal, clear description of the problem, then how you solved it. Link the issue if there is one. No checklists or boilerplate the repo's template does not ask for.
- End the description with a one-line blurb stating which model and harness made the changes, e.g. `Changes authored by Claude Fable 5 via Claude Code.`
- The PR description is the only place that attribution goes. Commit messages stay trailer-free.

### Monitoring a PR

When asked to monitor or babysit a PR:

- Poll checks and comments newer than the last push. Do not re-litigate findings already handled.
- Verify each bot finding against the source before acting on it. Fix real ones; dismiss false positives with a written reason.
- Fix CI failures, distinguishing real breaks from known infra flakes. Re-run a flake; fix a break.
- If nothing is new, stay quiet. Do not post filler comments.
- Stop when the repo's review bots are green on the latest commit, and report.

### Merging

- Merge only per the disposition given in the request (merge when green, or stop and report). If none was given, report and ask.
- Never merge into `main` on your own initiative, and never force-push `main` or any shared branch.

# Stop points

When I give you a stop point, stop there. Do not commit, push, or deploy past it.
