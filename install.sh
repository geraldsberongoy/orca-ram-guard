#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NODE_BIN="$(command -v node)"
ORCA_BIN="${ORCA_CLI_COMMAND:-$(command -v orca || echo "/Applications/Orca.app/Contents/Resources/bin/orca")}"
PLIST_PATH="$HOME/Library/LaunchAgents/com.orca.ram-guard.plist"

echo "🔧 Installing Orca RAM & Port Guard from: $REPO_DIR"
chmod +x "$REPO_DIR/orca-ram-guard-check.mjs" "$REPO_DIR/orca-port-guard-check.mjs"

# 1. Install macOS LaunchAgent (runs every 20 minutes)
mkdir -p "$HOME/Library/LaunchAgents"
cat <<EOF > "$PLIST_PATH"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.orca.ram-guard</string>
  <key>ProgramArguments</key>
  <array>
    <string>$NODE_BIN</string>
    <string>$REPO_DIR/orca-ram-guard-check.mjs</string>
    <string>--min-idle-mins=20</string>
    <string>--min-heavy-mb=40</string>
    <string>--min-total-idle-mb=150</string>
  </array>
  <key>StartInterval</key>
  <integer>1200</integer>
  <key>RunAtLoad</key>
  <false/>
  <key>StandardOutPath</key>
  <string>/tmp/orca-ram-guard.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/orca-ram-guard.err.log</string>
</dict>
</plist>
EOF

launchctl unload "$PLIST_PATH" 2>/dev/null || true
launchctl load "$PLIST_PATH"
echo "✅ Loaded macOS LaunchAgent: $PLIST_PATH"

# 2. Load the Orca Status-Bar Plugin
echo "🔌 Loading Orca status-bar plugin..."
set +e
LOAD_OUT="$("$ORCA_BIN" plugin dev load "$REPO_DIR" --json 2>&1)"
LOAD_CODE=$?
set -e

if [ $LOAD_CODE -eq 3 ]; then
  FINGERPRINT="$(echo "$LOAD_OUT" | grep -oE 'ap1-[a-f0-9]+' | head -n 1)"
  if [ -n "$FINGERPRINT" ]; then
    echo "🔐 Approving plugin with fingerprint $FINGERPRINT..."
    "$ORCA_BIN" plugin dev load "$REPO_DIR" --approve "$FINGERPRINT" --json >/dev/null
    echo "✅ Orca RAM & Port Guard plugin loaded and running!"
  else
    echo "$LOAD_OUT"
  fi
elif [ $LOAD_CODE -eq 0 ]; then
  echo "✅ Orca RAM & Port Guard plugin loaded and running!"
else
  echo "⚠️ Could not auto-load into Orca runtime (is Orca open?). Output:"
  echo "$LOAD_OUT"
fi

echo ""
echo "🎉 Setup complete! Try running:"
echo "  node \"$REPO_DIR/orca-ram-guard-check.mjs\" --force-dialog"
echo "  node \"$REPO_DIR/orca-port-guard-check.mjs\""
