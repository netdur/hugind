# coder_context

`coder_context` builds task-aware repository context for `agent/coder`.

It reads a task file, scans the project, ranks likely target files, and writes
a machine-readable context artifact (`context.json`) that downstream agents can
consume.

## Purpose

Use this agent when task instructions do not clearly include a direct
`Target module` path, or when you want a pre-analysis step before code edits.

The agent helps by:

- extracting intent and hints from `task.md`
- identifying candidate files in the project
- scoring and ranking those candidates
- returning confidence and recommendations for edit guardrails

## CLI

```bash
hugind agent run agent/coder_context -- \
  --task <path> \
  --project <path> \
  --context <path> \
  [--cwd <path>] \
  [--max-files <n>] \
  [--max-scan-files <n>] \
  [--max-file-bytes <n>] \
  [--debug]
```

### Required flags

- `--task`: path to task markdown file
- `--project`: project root to scan (relative to cwd or absolute)
- `--context`: output path for generated context JSON

### Optional flags

- `--cwd`: runtime working directory override
- `--max-files`: max number of top-ranked target files (default: `8`)
- `--max-scan-files`: max files to walk while scanning (default: `800`)
- `--max-file-bytes`: max file size sampled for content scoring (default: `200000`)
- `--debug`: print extracted hints/signals

## Output

Writes a JSON file at `--context` with schema version `coder_context/v1`.

Top-level fields:

- `schema_version`
- `generated_at`
- `task`
- `project`
- `confidence`: `low | medium | high`
- `target_files`: ranked primary candidates
- `supporting_files`: additional candidates
- `recommendations`
- `project.profile` (inferred framework/languages/architecture hints)

Each entry in `target_files` / `supporting_files` includes:

- `path`: project-relative path
- `score`: ranking score
- `reasons`: short explanation list

## Confidence semantics

- `high`: strong signal (explicit target or clearly dominant file)
- `medium`: reasonable candidate set, but not decisive
- `low`: weak signal; task likely needs explicit target path/hints

Recommended downstream behavior:

- enforce edits to `target_files` when confidence is `high` or `medium`
- fail fast / request clarification when confidence is `low`
- if `recommendations.likely_requires_new_files` is `true`, prefer creating new
  feature files under `recommendations.suggested_new_file_roots`

## Example

```bash
./target/release/hugind agent run agent/coder_context --cwd /repo/.worktrees/trial-002 -- \
  --task task.md \
  --project src \
  --context context.json
```

Then pass the generated context to `agent/coder` (once coder consumes it) to
improve target selection and reduce incorrect edits.
