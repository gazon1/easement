---
status: proposed
---

# A narrowed verdict and an evidence gap are reported separately

A verdict can fall short of the whole project for two unrelated reasons. One is that the caller asked for a part (`--diff`, `--component`, `--scope`): a narrowed verdict. The other is that the check could not read or judge some code: an evidence gap. One `confidence: partial` value plus a flat list of prose reasons hid which of the two applied, so an agent could neither gate on missing evidence without rejecting every deliberate diff run, nor see accepted gaps (`uncovered_ok`), which were dropped from the output. So reports carry narrowing, evidence gaps and accepted gaps as separate typed lists. Cache provenance is a fourth, independent note: a cache hit is not a gap. `confidence` and `confidence_reasons`, kept a while for agents that read them, are gone: the three lists are the one place that says what a verdict leaves out.

## Considered options

- **Keep one `confidence` value and add a gate flag that filters its reasons.** Rejected because it treats the symptom: the gate would have to re-parse prose, and every other consumer would still be unable to tell the two cases apart.
- **Keep `confidence` beside the new lists.** First chosen, since agents read it; dropped in a breaking release, as two answers to one question invite reading the vaguer one.
