---
status: accepted
---

# `reach` is one switch for the project

[#58](https://github.com/mokevnin/easement/issues/58) offered `reach: symbols` as an opt-in per component, so that `must_not_reach` follows the definitions a call references instead of whole files. It is one key of the policy instead. A reach chain crosses components: with a granularity per component, one chain would follow definitions through some components and whole files through others, and whether `must_not_reach` fires would depend on which components it passes. One switch keeps the meaning of a chain the same from end to end.

## Considered options

- **Per component, as #58 offered.** Rejected for the mixed chains above, and because a component's choice would change the verdict of rules written on other components.

## Consequences

- A project turns `reach: symbols` on for all of its chains at once; a file the index gave no ranges keeps reaching as a whole.
