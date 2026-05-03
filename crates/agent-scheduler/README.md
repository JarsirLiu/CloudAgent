# agent-scheduler

`agent-scheduler` is reserved for future scheduled and wakeup-style automation support.

This crate is not part of the interactive agent turn loop. It should not become a second runtime
or a place to stash unrelated agent flow logic.

## Intended Responsibilities

- waking jobs or agents at scheduled times
- coordinating delayed or recurring execution
- triggering background follow-up work outside the live chat turn path

## Non-Responsibilities

- normal user turn execution
- built-in tool routing
- approval flow for interactive turns
- model request assembly for the main chat loop

If code is required for the live interactive agent path, it does not belong here.
