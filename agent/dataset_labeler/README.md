# dataset_labeler

Generate YOLO-style labels from image datasets using a vision-capable Hugind model.

The agent uses the runtime working directory as dataset root (`--cwd /path/to/dataset`).
No dataset path arguments are required.

## Expected dataset layout

```text
dataset/
  images/
    train/
    val/                   # optional
    test/                  # optional
  labels/                  # auto-created if missing
    train/
    val/
    test/
  meta/
    classes.txt            # required
    raw/                   # auto-created, stores raw model output per image
```

## classes.txt format

One class per line, in class-id order (0-based by line index).

Supported line formats:

```text
person | visible full/partial human body
person - visible full/partial human body
car | passenger vehicle, sedan/suv/truck
traffic_light
```

Rules:

- `class_name | description` or `class_name - description` (description optional)
- empty lines are ignored
- lines starting with `#` are ignored

## Run

```bash
./target/release/hugind agent run agent/dataset_labeler --cwd /path/to/dataset
```

Resume mode (skip done files, process new + failed):

```bash
./target/release/hugind agent run agent/dataset_labeler --cwd /path/to/dataset -- --resume
```

Retry failed only:

```bash
./target/release/hugind agent run agent/dataset_labeler --cwd /path/to/dataset -- --retry-failed-only
```

List failed files (no inference, just print paths):

```bash
# default mode lists failed source image paths
./target/release/hugind agent run agent/dataset_labeler --cwd /path/to/dataset -- --list-failed

# alternatives: raw | label | all
./target/release/hugind agent run agent/dataset_labeler --cwd /path/to/dataset -- --list-failed=raw
./target/release/hugind agent run agent/dataset_labeler --cwd /path/to/dataset -- --list-failed=label
./target/release/hugind agent run agent/dataset_labeler --cwd /path/to/dataset -- --list-failed=all
```

## Current behavior

- Validate required inputs before processing:
  - `images/` exists
  - `images/train` exists (`images/val` and `images/test` optional)
  - `meta/classes.txt` exists and parses
- Create missing output folders:
  - `labels/<split>` for detected splits (always `train`, optional `val`, optional `test`)
  - `meta/raw/<split>` for detected splits
- For each image:
  - send vision request with streaming (`llm.chat_stream`)
  - produce one YOLO label file in `labels/<split>/<image_stem>.txt`
  - write raw model output to `meta/raw/<split>/<image_stem>.json`
- Save run logs automatically to:
  - `meta/dataset_labeler.log`

## Label format

Each output label file uses YOLO normalized format:

```text
<class_id> <x_center> <y_center> <width> <height>
```

Notes:

- `class_id` is the line index from `meta/classes.txt`
- Coordinates are normalized to `[0,1]`
- Empty file means no valid detections for that image
