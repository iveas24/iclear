#!/bin/sh
# Installs the iclear privileged helper as a launchd daemon.
# Runs as root (through osascript "with administrator privileges").
# Usage: install.sh <helper-binary> <allowed-uid> <label>
set -eu

SRC="$1"
ALLOWED_UID="$2"
LABEL="$3"
DEST="/Library/PrivilegedHelperTools/$LABEL"
PLIST="/Library/LaunchDaemons/$LABEL.plist"
CONF_DIR="/Library/Application Support/iclear"

case "$ALLOWED_UID" in
  ''|*[!0-9]*) echo "uid inválido" >&2; exit 1 ;;
esac

launchctl bootout "system/$LABEL" 2>/dev/null || true

mkdir -p /Library/PrivilegedHelperTools "$CONF_DIR"
cp "$SRC" "$DEST"
chown root:wheel "$DEST"
chmod 755 "$DEST"

# Only this uid may talk to the helper.
printf '{"allowed_uid":%s}\n' "$ALLOWED_UID" > "$CONF_DIR/helper-auth.json"
chown root:wheel "$CONF_DIR/helper-auth.json"
chmod 644 "$CONF_DIR/helper-auth.json"

cat > "$PLIST" <<PLISTEOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key><array><string>$DEST</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardErrorPath</key><string>/Library/Logs/iclear-helper.log</string>
  <key>StandardOutPath</key><string>/Library/Logs/iclear-helper.log</string>
</dict>
</plist>
PLISTEOF
chown root:wheel "$PLIST"
chmod 644 "$PLIST"

launchctl bootstrap system "$PLIST"
launchctl enable "system/$LABEL"
echo "ok"
