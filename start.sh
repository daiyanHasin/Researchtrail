#!/usr/bin/env sh
# Start ResearchTrail. Usage: ./start.sh [project folder]
cd "$(dirname "$0")" || exit 1
if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust isn't installed. Install it from https://rustup.rs, then run this again."
  exit 1
fi
if [ ! -x target/release/researchtrail ] || [ -n "$(find src web catalog Cargo.toml -newer target/release/researchtrail 2>/dev/null | head -n 1)" ]; then
  echo "Building ResearchTrail (the first time takes a few minutes)..."
  cargo build --release || exit 1
fi
exec ./target/release/researchtrail "$@"
