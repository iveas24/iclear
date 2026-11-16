#!/bin/sh
# Removes the iclear privileged helper. Runs as root.
# Usage: uninstall.sh <label>
set -u
LABEL="$1"
launchctl bootout "system/$LABEL" 2>/dev/null || true
rm -f "/Library/LaunchDaemons/$LABEL.plist" "/Library/PrivilegedHelperTools/$LABEL"
rm -rf "/Library/Application Support/iclear"
rm -f /var/run/iclear-helper.sock
echo "ok"
