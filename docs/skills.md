# Skills

Skills are installable instruction packages that any agentic agent can discover and activate on demand. A single `ma-developer` agent can write Rust, PHP, or Python -- depending on which skills are installed and what the task requires.

Skills are **not** tied to specific agents. They are installed globally and auto-discovered by every agentic agent at runtime.

## How it works

1. You install skills to `~/.hugind/skills/`
2. When an agentic agent starts, it sees a **catalog** of all installed skills (just names and descriptions) in its system prompt
3. The LLM reads the task, decides which skills are relevant, and calls the built-in `activate_skill` tool
4. The runtime returns the full skill instructions as the tool result
5. The LLM now has the instructions in context and proceeds with the actual work

Only activated skills consume tokens. A "Build a Rust API" task activates `rust` and `api-design` but not `php`.

## SKILL.md format

A skill is a directory containing a `SKILL.md` file:

```
~/.hugind/skills/rust/
  SKILL.md
  examples/           # optional supporting files
    error_handling.rs
```

`SKILL.md` uses YAML frontmatter followed by markdown instructions:

```markdown
---
name: rust
version: "1.0"
description: "Rust development: idiomatic patterns, error handling, cargo workflows"
tags: [rust, coding, systems]
---

## Rust Development

When writing Rust code:
- Use `thiserror` for library errors, `anyhow` for application errors
- Prefer `&str` over `String` in function parameters
- Run `cargo clippy` before considering code complete
- Use `#[derive(Debug, Clone)]` on public types
- Handle errors with `?` operator, avoid `.unwrap()` in library code
```

### Frontmatter fields

| Field | Required | Description |
|---|---|---|
| `name` | yes | Unique identifier for the skill |
| `version` | yes | Semantic version |
| `description` | yes | One-line summary -- the LLM reads this to decide relevance |
| `tags` | no | List of tags for categorization |

The **description** is critical. It's what the LLM sees in the catalog to decide whether to activate the skill. Make it specific and informative.

## CLI commands

### Install a skill

From a local directory:
```bash
hugind skill install ./my-skills/rust
hugind skill install /path/to/skill/SKILL.md
```

From a URL or GitHub:
```bash
hugind skill install https://github.com/user/hugind-skills/tree/main/rust
```

The installer validates that `SKILL.md` exists and parses correctly, then copies the skill directory to `~/.hugind/skills/{name}/`.

### List installed skills

```bash
hugind skill list

NAME                 VERSION      DESCRIPTION
rust                 1.0          Rust development: idiomatic patterns, error handling, cargo workflows
php                  1.0          PHP development: Laravel patterns, PSR standards, Composer workflows
api-design           1.0          REST API design principles and OpenAPI patterns
```

### Remove a skill

```bash
hugind skill remove rust
```

## How the LLM activates skills

The system prompt seen by the LLM includes:

```
## Available Skills

You have access to the following skills. To activate a skill that is
relevant to your current task, use the activate_skill tool.

- rust: Rust development: idiomatic patterns, error handling, cargo workflows
- php: PHP development: Laravel patterns, PSR standards, Composer workflows
- api-design: REST API design principles and OpenAPI patterns
```

The `activate_skill` tool is registered automatically alongside the agent's own tools:

```
- activate_skill(name): Load a skill's full instructions into context
- write_file(path, content): Write a file
- read_file(path): Read a file
```

When the LLM decides a skill is relevant, it calls:

```
<tool_call>{"name":"activate_skill","args":{"name":"rust"}}</tool_call>
```

The runtime loads the full `SKILL.md` body and returns it as the tool result. The LLM now has the instructions in its conversation history and follows them for the rest of the task.

## Writing good skills

### Keep descriptions precise

The description is what the LLM uses to decide whether to activate. Be specific:

```yaml
# Good -- the LLM knows exactly when this applies
description: "Rust development: idiomatic patterns, error handling, cargo workflows"

# Bad -- too vague, LLM can't judge relevance
description: "Programming help"
```

### Keep instructions actionable

Skills should contain concrete rules and patterns, not general advice:

```markdown
## Rust Error Handling

- Define error types in `src/error.rs` using `thiserror::Error`
- Use `anyhow::Result` in binary crates, custom errors in libraries
- Always add context with `.context("what failed")`
- Never use `.unwrap()` outside of tests
```

### Use supporting files

A skill directory can contain files alongside `SKILL.md`:

```
~/.hugind/skills/rust/
  SKILL.md
  examples/
    error_handling.rs
    api_pattern.rs
  templates/
    module.rs.template
```

Reference them in the instructions. If the agent has a `read_file` tool, the LLM can read them using the absolute path (shown in trace output).

### Scope skills narrowly

One skill per domain. Don't create a "programming" skill -- create `rust`, `python`, `typescript`, `go` separately. This lets the LLM activate only what's needed and keeps context focused.

## Skills in multi-agent teams

When using `hugind agent team`, the coordinator LLM also sees the skill catalog. It can mention relevant skills in task descriptions to guide agents:

```bash
hugind agent team "Build a REST API in Rust with tests" \
  --agents agent/ma-architect,agent/ma-developer,agent/ma-tester
```

The coordinator might produce:

```json
[
  {"title": "Design API", "description": "Design the REST API spec", "assignee": "ma-architect"},
  {"title": "Implement API", "description": "Implement in Rust (activate rust and api-design skills)", "assignee": "ma-developer"},
  {"title": "Test API", "description": "Test all endpoints", "assignee": "ma-tester"}
]
```

Each agent then independently activates the skills it needs.

## WASM agents

WASM agents have access to the same skill system via hostcalls:

```
hugind.get_skill_catalog() -> string    # names + descriptions
hugind.activate_skill(name) -> string   # full instructions
```

Using the AssemblyScript SDK:

```typescript
import { getSkillCatalog, activateSkill } from "./hugind";

const catalog = getSkillCatalog();     // skill list for system prompt
const instructions = activateSkill("rust");  // full SKILL.md body
```

## Example skills

### Rust development

```markdown
---
name: rust
version: "1.0"
description: "Rust development: idiomatic patterns, error handling, cargo workflows"
tags: [rust, coding, systems]
---

## Rust Development

### Project setup
- Use `cargo init` for new projects
- Structure: `src/main.rs` or `src/lib.rs`, modules in `src/`
- Add dependencies with `cargo add`

### Error handling
- Use `thiserror` for library error types
- Use `anyhow::Result` in application code
- Add context: `.context("failed to read config")`
- Never `.unwrap()` outside tests

### Code style
- Run `cargo fmt` and `cargo clippy` before finishing
- Derive `Debug` on all public types
- Use `&str` parameters, return `String`
- Prefer iterators over manual loops
```

### Git workflow

```markdown
---
name: git-workflow
version: "1.0"
description: "Git branching strategy and commit message conventions"
tags: [git, workflow]
---

## Git Workflow

### Branches
- Feature branches: `feature/<short-description>`
- Bug fixes: `fix/<short-description>`
- Always branch from `main`

### Commits
- Format: `type: short description`
- Types: feat, fix, refactor, docs, test, chore
- Keep commits atomic -- one logical change per commit
- Write in imperative mood: "add feature" not "added feature"
```

### API design

```markdown
---
name: api-design
version: "1.0"
description: "REST API design: resource naming, status codes, error responses"
tags: [api, rest, http]
---

## REST API Design

### URLs
- Use nouns, not verbs: `/users` not `/getUsers`
- Use plural: `/users` not `/user`
- Nest for relationships: `/users/{id}/posts`

### Status codes
- 200: success with body
- 201: created (return the created resource)
- 204: success, no body (DELETE)
- 400: bad request (validation errors)
- 404: not found
- 500: server error (never expose internals)

### Error responses
Always return structured errors:
```json
{"error": {"code": "VALIDATION_ERROR", "message": "Email is required"}}
```
```
