#!/bin/bash
# ─────────────────────────────────────────────────────────────────────────────
# push_tag.sh — Interactive release script
#
# Usage: ./push_tag.sh
#   → Shows current tag, next tag, then asks for commit message interactively
# ─────────────────────────────────────────────────────────────────────────────

set -e

# ── 1. Fetch latest tags from remote ─────────────────────────────────────────
echo "📡 Fetching latest tags from remote..."
git fetch --tags --quiet

# ── 2. Find the latest tag ───────────────────────────────────────────────────
LATEST_TAG=$(git tag --sort=-v:refname | head -n 1)
LATEST_TAG=${LATEST_TAG:-v0.0.0}

# ── 3. Parse version components ──────────────────────────────────────────────
VERSION="${LATEST_TAG#v}"
IFS='.' read -r MAJOR MINOR PATCH <<< "$VERSION"
MAJOR=${MAJOR:-0}
MINOR=${MINOR:-0}
PATCH=${PATCH:-0}

# ── 4. Build next tag ─────────────────────────────────────────────────────────
NEW_PATCH=$((PATCH + 1))
if [[ "$LATEST_TAG" == v* ]]; then
  NEW_TAG="v${MAJOR}.${MINOR}.${NEW_PATCH}"
else
  NEW_TAG="${MAJOR}.${MINOR}.${NEW_PATCH}"
fi

# ── 5. Show info and prompt user ──────────────────────────────────────────────
echo ""
echo "┌─────────────────────────────────────────┐"
echo "│         Baytraq Release Script          │"
echo "├─────────────────────────────────────────┤"
printf  "│  Current tag : %-25s │\n" "$LATEST_TAG"
printf  "│  Next tag    : %-25s │\n" "$NEW_TAG"
echo "└─────────────────────────────────────────┘"
echo ""
echo "💬 Enter commit message (or press Enter for: \"New release ${NEW_TAG}\"):"
read -r USER_MSG

# ── 6. Build final commit message ─────────────────────────────────────────────
if [[ -z "$USER_MSG" ]]; then
  COMMIT_MSG="New release ${NEW_TAG}"
else
  COMMIT_MSG="$USER_MSG"
fi

echo ""
echo "📝 Commit message: \"$COMMIT_MSG\""
echo ""

# ── 7. Stage all changes ──────────────────────────────────────────────────────
git add -A

# ── 8. Commit if there are staged changes ────────────────────────────────────
if git diff --cached --quiet; then
  echo "ℹ️  No changes to commit. Creating tag only."
else
  git commit -m "$COMMIT_MSG"
  echo "✅ Committed."
fi

# ── 9. Create local tag ───────────────────────────────────────────────────────
git tag "$NEW_TAG"
echo "🏷️  Tag $NEW_TAG created."

# ── 10. Push branch + tag ─────────────────────────────────────────────────────
BRANCH=$(git rev-parse --abbrev-ref HEAD)
echo "📤 Pushing branch '$BRANCH'..."
git push origin "$BRANCH"

echo "📤 Pushing tag $NEW_TAG..."
git push origin "$NEW_TAG"

echo ""
echo "🎉 Done! Released $NEW_TAG successfully."
# git push origin master 