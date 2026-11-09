# Coding agent setup

[Documentation](README.md) · [Project overview](../README.md)

- [Set up with a coding agent](#set-up-with-a-coding-agent)
- [Agent skill](#agent-skill)

## Set up with a coding agent

Use the [copyable setup prompt in the project README](../README.md#set-up-with-a-coding-agent). It installs easement and the indexers, proposes a policy, sets up CI and installs the agent skill. Review the proposed rules before adopting them.

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
