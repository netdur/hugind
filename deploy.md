# Hugind Release Workflow (Rust)

This guide covers how to build, tag, and publish a new Hugind release for macOS (Homebrew).

## Prerequisites
- You are on the `main` branch.
- Your working directory is clean.
- Rust toolchain is installed (`cargo --version`).
- You have the `homebrew-hugind` tap repo cloned locally (for example `~/Workspace/homebrew-hugind`).

---

## Step 1: Bump Version
Update the crate version in `Cargo.toml`:

```toml
[package]
version = "0.11.1" # <--- Change this
```

Commit the change:

```bash
git add Cargo.toml
git commit -m "Bump version to 0.11.1"
```

---

## Step 2: Build the Release Artifact
Run the release script:

```bash
bash build_release.sh
```

What it does:
- Builds the Rust binary with `cargo build --release`.
- Copies `assets/config` into `dist/config`.
- Creates `hugind-macos-arm64.tar.gz` in the repository root.
- Prints the archive SHA256.

Important: copy the SHA256 printed by the script. You will use it in the Homebrew formula.

---

## Step 3: Push Tag to GitHub
Create and push a tag that matches the version:

```bash
git tag v0.11.1
git push origin v0.11.1
```

---

## Step 4: Create GitHub Release
1. Open: [https://github.com/netdur/hugind/releases/new](https://github.com/netdur/hugind/releases/new)
2. Choose tag: `v0.11.1`
3. Release title: `v0.11.1`
4. Upload release asset: `hugind-macos-arm64.tar.gz`
5. Publish release

If the asset is not uploaded, Homebrew install/upgrade will fail with `404`.

---

## Step 5: Update Homebrew Formula
Go to your tap repository:

```bash
cd ~/Workspace/homebrew-hugind
```

Edit `hugind.rb`:

```ruby
class Hugind < Formula
  # ...
  url "https://github.com/netdur/hugind/releases/download/v0.11.1/hugind-macos-arm64.tar.gz"
  sha256 "PASTE_THE_NEW_HASH_HERE"
  version "0.11.1"
  # ...
end
```

Commit and push:

```bash
git add hugind.rb
git commit -m "Update hugind to v0.11.1"
git push origin main
```

---

## Step 6: Verify Installation
Wait about 60 seconds for GitHub release propagation, then verify locally:

```bash
brew update
# or, if needed:
brew tap netdur/hugind

brew upgrade hugind
hugind --version
which hugind
```

Expected binary path on Apple Silicon: `/opt/homebrew/bin/hugind`.

## Checklist Summary
- [ ] `Cargo.toml` version updated?
- [ ] `bash build_release.sh` run successfully?
- [ ] SHA256 copied from script output?
- [ ] Tag pushed (`git push origin v...`)?
- [ ] `hugind-macos-arm64.tar.gz` uploaded to GitHub Release?
- [ ] `hugind.rb` updated and pushed?
