# Coding agent setup

[Documentation](README.md) · [Project overview](../README.md)

- [Set up with a coding agent](#set-up-with-a-coding-agent)
- [Agent skill](#agent-skill)

## Set up with a coding agent

Copy this prompt into Claude Code, Codex, Cursor or another coding agent opened in your project. The agent installs `easement`, the indexers and the [agent skill](#agent-skill), writes `easement.yml` for the architecture it finds, wires the check into CI and points the project's agent instructions at the skill.

```text
Set up easement (https://github.com/mokevnin/easement), an architecture quality gate, in this project. Read https://github.com/mokevnin/easement/blob/main/docs/README.md first. Use installation.md and languages.md for tools and indexers, configuration.md for rules, and adoption.md for baseline and CI setup; these pages are in the same docs directory.

1. Install easement the way this project installs its tools: a line in mise.toml if the project uses mise ("github:mokevnin/easement" = "latest"), otherwise Homebrew or the install script from installation.md.
2. Run `easement doctor --format json` and install every indexer it reports missing, again the way the project installs tools. Repeat until it exits 0.
3. Run `easement init`, then `easement check`. If the report says files are not covered by an indexer, fix the cause (a tsconfig include, a missing go.mod) or list them in `uncovered_ok` with a reason.
4. Study the code and turn the generated config into the intended architecture: remove the allowances that should not exist, give every rule a `because` that tells where the code should go instead, and use `layers`, `public`, `no_cycles` and `tests: auto` where they fit. Do not change application code to make the check pass. When the config is final, record the existing violations with `easement baseline`.
5. Add the check to CI: the GitHub Action from adoption.md with `diff` on pull requests if the project uses GitHub Actions, otherwise `easement check` in the existing pipeline. If the project has git hooks, run it on pre-push.
6. Install the easement skill into the project with `npx skills add mokevnin/easement`, for the agents this team uses, and commit the files it writes, so every agent that works here gets it.
7. Add these rules to the project's agent instructions (AGENTS.md, CLAUDE.md or whatever file the project already uses; create AGENTS.md if there is none):
   - Use the easement skill before adding a file or an import and whenever `easement check` fails.
   - After changing code, run `easement check`; it must exit 0.
   - Never edit easement.yml, regenerate the baseline or add an `easement:disable` comment without asking a human.
8. Run `easement check` once more and report: the components and rules you chose and why, the number of baselined violations, and anything you were unsure about.
```

Review the rules it proposes: an agent sees the code as it is, and only you know which of today's dependencies are mistakes.

## Agent skill

[`skills/easement/SKILL.md`](../skills/easement/SKILL.md) teaches an agent the daily work with easement: `explain` before adding code, `check` after it, fixing a violation by its `because` instead of editing the config, shrinking the baseline, and changing `easement.yml` when a human agrees. It follows the [Agent Skills](https://agentskills.io) format, so Claude Code, Codex, Cursor and others read it. The skill comes from this repository and changes with easement, while rules copied into a project's AGENTS.md go stale.

Install it into a project with the [skills](https://github.com/vercel-labs/skills) CLI (add `-g` for every project on the machine):

```sh
npx skills add mokevnin/easement
```

In Claude Code it is also a plugin:

```text
/plugin marketplace add mokevnin/easement
/plugin install easement@easement
```
