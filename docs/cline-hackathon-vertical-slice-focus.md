# Hackathon Focus: Vertical Slice

Status: historical sprint focus. The source-backed memory loop has been
completed; future joint Tethers/Lantern Keeper architecture and build order are
governed by
[`architecture/TETHERS_LANTERN_KEEPER_CANONICAL_ARCHITECTURE.md`](architecture/TETHERS_LANTERN_KEEPER_CANONICAL_ARCHITECTURE.md).

For the next few days, our priority is not to build every feature. Our priority is to prove the idea.

The architecture remains the long-term architecture. We are not creating throwaway code or demo hacks that compromise the foundations.

We are building a thin vertical slice through the real system.

During this push:

- Do not redesign core architecture unless it blocks the vertical slice.
- Do not add optional features because they're interesting.
- Do not optimise for every future use case.
- Keep interfaces clean and permanent.
- Stub or simplify only at the edges, never in the core.

Success is defined by one complete workflow:

1. Capture project knowledge.
2. Store it in the canonical memory.
3. Retrieve the right context.
4. Complete work.
5. Write the results back.
6. Demonstrate the updated state.

If a task does not directly help that workflow, defer it.

Every piece of code written during this sprint should either be something we're happy to keep, or a clearly isolated temporary adapter that can be replaced later without touching the core.

We're proving the concept, not shrinking the vision.
