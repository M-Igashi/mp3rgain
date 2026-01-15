# mp3rgain Project Rules

## Winget Package Submission

winget-pkgs への PR は GitHub CLI (`gh`) を使用して直接作成する。リポジトリ全体をクローンしない。

### 手順

1. **sparse checkout で必要な部分だけ取得**
   ```bash
   cd /tmp && mkdir -p winget-pr && cd winget-pr
   git init
   git remote add origin https://github.com/M-Igashi/winget-pkgs.git
   git config core.sparseCheckout true
   echo "manifests/m/M-Igashi/" > .git/info/sparse-checkout
   git fetch --depth 1 origin master
   git checkout master
   ```

2. **ブランチ作成とマニフェストコピー**
   ```bash
   git checkout -b mp3rgain-<version>
   mkdir -p manifests/m/M-Igashi/mp3rgain/<version>
   cp /Users/masanarihigashi/Projects/mp3rgain/packages/winget/*.yaml manifests/m/M-Igashi/mp3rgain/<version>/
   ```

3. **コミットとプッシュ**
   ```bash
   git add -A
   git commit -m "New package: M-Igashi.mp3rgain version <version>"
   git push origin mp3rgain-<version>
   ```

4. **gh で PR 作成**
   ```bash
   gh pr create --repo microsoft/winget-pkgs --base master --head M-Igashi:mp3rgain-<version> \
     --title "New package: M-Igashi.mp3rgain version <version>" \
     --body "..."
   ```

5. **クリーンアップ**
   ```bash
   rm -rf /tmp/winget-pr
   ```

### マニフェスト更新時の注意

- リリース公開後に SHA256 を取得して `packages/winget/*.yaml` を更新
- VCRedist 依存は不要（静的 CRT リンク済み）
- `ReleaseDate` は実際のリリース日に更新

## Release Workflow

### 必要なシークレット

以下のシークレットが GitHub リポジトリに設定されていること:

- `SCOOP_BUCKET_TOKEN` - scoop-bucket へのプッシュ用 GitHub PAT
- `HOMEBREW_TAP_TOKEN` - homebrew-tap へのプッシュ用 GitHub PAT  
- `CARGO_REGISTRY_TOKEN` - crates.io API トークン

### シークレット設定方法

```bash
gh auth token | gh secret set SCOOP_BUCKET_TOKEN --repo M-Igashi/mp3rgain
gh auth token | gh secret set HOMEBREW_TAP_TOKEN --repo M-Igashi/mp3rgain
# crates.io トークンは ~/.cargo/credentials.toml から取得
cat ~/.cargo/credentials.toml  # token を確認
echo "<token>" | gh secret set CARGO_REGISTRY_TOKEN --repo M-Igashi/mp3rgain
```

### Windows ビルド

Windows バイナリは静的 CRT リンクを使用:
```yaml
env:
  RUSTFLAGS: ${{ contains(matrix.target, 'windows') && '-C target-feature=+crt-static' || '' }}
```

これにより VCRUNTIME140.dll への依存がなくなり、VCRedist 不要で動作する。
