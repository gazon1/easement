---
status: accepted
---

# A policy input with an unknown past judges the run in full

`--diff` loads the policy at the merge-base and attributes to the change only what the change to the policy causes ([#97](https://github.com/mokevnin/easement/issues/97), ADR-0002). A policy input outside the repository, such as a shared policy that `extends` names by a path above the root, has no past there. #97 said such an input is treated as changed. easement goes one step further: whenever such an input exists, changed or not, the run is judged in full, because nothing can tell whether it changed. A narrowed run over a policy whose old half is unknown could pass a change that the new policy rejects, and AGENTS.md asks for the conservative choice when in doubt.

## Considered options

- **Treat only that input as changed and narrow the rest.** Rejected: what the input contributes to the old policy is unknown, so the delta it causes cannot be computed.

## Consequences

- A project that wants narrowed `--diff` runs keeps its shared policy inside the repository, as a submodule or a copy.
- The header of the run and `diff.in_full` say why it was judged in full, and `review` names the input in `inputs.past_unknown`.
