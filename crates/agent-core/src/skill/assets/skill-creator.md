---
name: skill-creator
description: Use this skill when the user wants to create a new CloudAgent skill or refine an existing one under .cloudagent/skills or ~/.cloudagent/skills.
policy:
  allow_implicit_invocation: true
---

# Skill Creator

This skill helps create or improve CloudAgent skills that follow the local `SKILL.md` package pattern.

CloudAgent packages skill creation and validation natively. Do not depend on Python or any external interpreter when scaffolding a new skill.

When creating a skill, prefer the built-in tool flow:

1. Run `create_skill_scaffold` with the normalized skill name, the parent skills directory, and the optional resource folders to create.
2. Edit the generated `SKILL.md` to replace the placeholders with real guidance.
3. Run `validate_skill` on the finished skill directory before declaring it ready.

## What a CloudAgent skill is

A skill is a local instruction package rooted at a directory with a required `SKILL.md` file.

Typical layout:

```text
.cloudagent/
  skills/
    my-skill/
      SKILL.md
      references/
      scripts/
      assets/
```

Required:

- `SKILL.md`

Optional:

- `references/` for supporting docs the agent may read on demand
- `scripts/` for helper scripts the agent may execute or edit
- `assets/` for templates or output resources

## When creating a skill

1. Confirm the skill solves a repeatable workflow rather than a one-off task.
2. Prefer a short lowercase hyphenated folder name.
3. Create the scaffold directly under the chosen skills directory.
4. Write YAML frontmatter with:
   - `name`
   - `description`
   - optional `policy.allow_implicit_invocation`
   - `dependencies.tools`
5. Keep the body focused on:
   - when to use the skill
   - the workflow
   - any supporting files to read or run

## SKILL.md template

```md
---
name: my-skill
description: Use this skill when the user needs ...
policy:
  allow_implicit_invocation: true
dependencies:
  tools: []
---

# My Skill

## When to use

...

## Workflow

1. ...
2. ...
3. ...

## Extra files

- Read `references/...` when ...
- Run `scripts/...` when ...
```

## Guidance

- Keep the description specific enough that the agent can decide when to use it.
- Omit `policy.allow_implicit_invocation` when the skill is safe for implicit use; set it to `false` for explicit-only skills.
- Do not turn a skill into a giant knowledge dump.
- Put detailed material in `references/` instead of bloating `SKILL.md`.
- If a script gives more reliable behavior than ad hoc generation, prefer adding a script.
- When updating an existing skill, preserve its intent and tighten the trigger description before adding more body text.

## Placement rules

- Workspace-local skills belong under `<workspace>/.cloudagent/skills/`
- User-wide skills belong under `~/.cloudagent/skills/`
- Do not place skill implementations in `agent-tools`; skills are context packages, not tools

## Creation workflow

1. Choose workspace-local or user-wide placement.
2. Use `create_skill_scaffold` to create `<skills-root>/<skill-name>/SKILL.md`.
3. Add optional `scripts/`, `references/`, or `assets/` only when the workflow needs them.
4. Edit the generated `SKILL.md` to replace the TODO sections with real guidance.
5. Run `validate_skill` on the finished skill before relying on it broadly.
6. Use `$skill-name` in a later turn to exercise the new skill.

## Error handling

- Normalize user-provided names to lowercase hyphen-case before creating the skill.
- If the target skill already exists, stop and ask whether to update it instead of overwriting silently.
- If validation reports a frontmatter or directory mismatch, fix the generated files before declaring the skill ready.
