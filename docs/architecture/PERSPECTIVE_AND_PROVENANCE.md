# Perspective and Provenance

Status: **canonical current invariant**

Lantern Keeper does not flatten every sentence into an ownerless fact. Each
assertion may distinguish:

- originator — who produced the proposition;
- speaker — who said or wrote this occurrence;
- transmitter — who relayed it;
- holder — whose belief or perspective is represented;
- stance — endorsing, rejecting, questioning, or another explicit relation;
- transformation — how quotation, relay, correction, or other framing changed
  the route from evidence to assertion.

When dialogue contains quotation or relay, `framing_path` records meaningful
nested actions such as speaking, quoting, endorsing, rejecting, and correcting.

## Evidence route

Claims point to an Episode or Source span where available. Exact source text is
preserved before interpretation. A Claim may remain unmapped when no canonical
predicate exists; the system must not invent a predicate merely to make a
record look complete.

Beliefs are projections with holder, subject, predicate, scope, trust class,
valid time, and dependency-generation fields. A migrated note whose original
author cannot be established uses `legacy:shared` and an explicit migrated
trust class. It is never silently attributed to Matthew.

## Anti-laundering rules

- Quoting is not authorship.
- Repetition is not adoption.
- Silence has zero endorsement weight.
- Assistant output cannot independently prove a proposition about Matthew.
- A generated summary cannot become more authoritative than its sources.
- A Context Pack can cite evidence, but it cannot become evidence by citing
  itself.

These rules protect both current beliefs and historical inspection. Provenance
is not decorative metadata; it is the route needed to explain, correct, and
recover a projection.
