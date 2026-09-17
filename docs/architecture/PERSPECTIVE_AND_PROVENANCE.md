# Perspective and Provenance

Status: **canonical current architecture**

Lantern Keeper treats perspective and provenance as first-class data rather than metadata to reconstruct later.

Retained assertions can distinguish:

- originator;
- speaker;
- transmitter;
- holder;
- stance;
- transformation or framing path.

When dialogue contains quotation, relay, endorsement, rejection or correction, a `framing_path` can record that meaningful nesting.

This prevents common attribution failures such as treating a quotation as authorship, assistant repetition as user belief, or a migrated shared note as if its original author were known.

## Evidence linkage

Claims point to an Episode or Source span where available.

New factual conversational capture records the exact supplied evidence as a plain-text Source, creates or reuses a whole-text Episode, and links the Claim to that UTF-8 byte span before reconciliation.

Provenance IDs are created by Lantern rather than being trusted from an ordinary conversational client.

## Belief projections

Beliefs are projections with holder, subject, predicate, scope, trust class, valid time and dependency-generation fields.

Imported canonical material whose original authorship cannot be established remains explicitly shared/legacy material. It is never silently attributed to a particular human or assistant.

## Generated views are not evidence

Generated narrative views and Context Packs are disposable projections. They may cite evidence, but they never become evidence merely because they were generated.

This rule blocks self-citation loops in which an AI-generated summary is later rediscovered and treated as independent support for itself.
