# 🚀 Hugind Release Workflow

This guide covers how to build, tag, and publish a new version of Hugind for macOS (Homebrew).

## Prerequisites
- You are on the `main` branch.
- Your working directory is clean.
- You have the `homebrew-hugind` repo cloned nearby (e.g., `../homebrew-hugind`).

---

## Step 1: Bump Version
Update the version number in `pubspec.yaml`.

```yaml
version: 0.1.1  # <--- Change this
```

Commit the change:
```bash
git add pubspec.yaml
git commit -m "Bump version to 0.1.1"
```

---

## Step 2: Build the Artifact
Run the release script. This compiles the Dart binary and bundles it with the required `.dylib` files and config templates.

```bash
bash build_release.sh
```

**⚠️ IMPORTANT:**
At the end of the script, copy the **SHA256 Checksum**. You will need this for Homebrew.
> Example output: `SHA256: c661bf85...`

---

## Step 3: Push Tag to GitHub
Create a git tag matching the version and push it.

```bash
git tag v0.1.1
git push origin v0.1.1
```

---

## Step 4: Create GitHub Release
1. Go to: [https://github.com/netdur/hugind/releases/new](https://github.com/netdur/hugind/releases/new)
2. **Choose Tag:** Select `v0.1.1`.
3. **Title:** `v0.1.1`.
4. **Attach Binaries (CRITICAL):**
   - Click "Upload assets".
   - Select the file: `dist/hugind-macos-arm64.tar.gz`.
   - *Note: If you don't do this, Homebrew will get a 404 error.*
5. Click **Publish Release**.

---

## Step 5: Update Homebrew Formula
Navigate to your local Homebrew Tap repository.

```bash
cd ~/Workspace/homebrew-hugind
```

Edit `hugind.rb`:

```ruby
class Hugind < Formula
  # ...
  
  # 1. Update the URL Version
  url "https://github.com/netdur/hugind/releases/download/v0.1.1/hugind-macos-arm64.tar.gz"
  
  # 2. Paste the SHA256 from Step 2
  sha256 "PASTE_THE_NEW_HASH_HERE"
  
  # 3. Update the Version string
  version "0.1.1"

  # ...
end
```

Commit and push the formula:

```bash
git add hugind.rb
git commit -m "Update hugind to v0.1.1"
git push origin main
```

---

## Step 6: Verify
Wait about 60 seconds for GitHub to propagate, then test the upgrade locally:

```bash
# Update your local tap
brew update

# Upgrade the package
brew upgrade hugind

# Verification
hugind --version   # (If you implemented a version flag)
which hugind       # Should be /opt/homebrew/bin/hugind
```

## Checklist Summary
- [ ] `pubspec.yaml` updated?
- [ ] `./build_release.sh` run?
- [ ] SHA256 copied?
- [ ] Tag pushed (`git push origin v...`)?
- [ ] **.tar.gz uploaded to GitHub Release?**
- [ ] `hugind.rb` updated and pushed?