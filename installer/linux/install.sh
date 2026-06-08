#!/usr/bin/env bash
# Installs BoogieBox as a systemd service on Linux.
# Must be run as root (sudo ./install.sh) from the release folder.
set -euo pipefail

INSTALL_DIR="/opt/boogiebox"
DATA_DIR="/var/lib/boogiebox"
SERVICE_NAME="boogiebox"
SERVICE_USER="boogiebox"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RELEASE_DIR="$(dirname "$SCRIPT_DIR")"

if [ "$(id -u)" -ne 0 ]; then
  echo "[ERROR] This script must be run as root."
  echo "        Run: sudo $0"
  exit 1
fi

if ! command -v systemctl &>/dev/null; then
  echo "[ERROR] systemd is required. This installer does not support non-systemd Linux distributions."
  exit 1
fi

echo ""
echo " Installing BoogieBox to ${INSTALL_DIR}..."
echo ""

# Create system user if not present
if ! id -u "$SERVICE_USER" &>/dev/null; then
  echo " Creating system user: ${SERVICE_USER}"
  useradd --system --no-create-home --shell /usr/sbin/nologin "$SERVICE_USER"
fi

# Stop existing service if running
if systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
  echo " Stopping existing service..."
  systemctl stop "$SERVICE_NAME"
fi

# Install binary and resources
echo " Copying files to ${INSTALL_DIR}..."
mkdir -p "$INSTALL_DIR"
cp "${RELEASE_DIR}/boogiebox-server"   "${INSTALL_DIR}/boogiebox-server"
chmod +x "${INSTALL_DIR}/boogiebox-server"

if [ -d "${RELEASE_DIR}/client" ]; then
  rm -rf "${INSTALL_DIR}/client"
  cp -r "${RELEASE_DIR}/client" "${INSTALL_DIR}/client"
fi

if [ -d "${RELEASE_DIR}/resources" ]; then
  rm -rf "${INSTALL_DIR}/resources"
  cp -r "${RELEASE_DIR}/resources" "${INSTALL_DIR}/resources"
fi

# Create data directory
echo " Creating data directory: ${DATA_DIR}..."
mkdir -p "$DATA_DIR"
chown "${SERVICE_USER}:${SERVICE_USER}" "$DATA_DIR"

# Install systemd unit
echo " Installing systemd unit..."
cp "${SCRIPT_DIR}/boogiebox.service" "/etc/systemd/system/${SERVICE_NAME}.service"
systemctl daemon-reload
systemctl enable "$SERVICE_NAME"
systemctl start "$SERVICE_NAME"

# Brief pause then check status
sleep 2
if systemctl is-active --quiet "$SERVICE_NAME"; then
  PORT=${PORT:-3001}
  echo ""
  echo " ================================================================="
  echo "  BoogieBox installed and running."
  echo "  Open http://localhost:${PORT} in your browser."
  echo "  On first launch, complete the setup wizard."
  echo ""
  echo "  Logs:    journalctl -u ${SERVICE_NAME} -f"
  echo "  Stop:    sudo systemctl stop ${SERVICE_NAME}"
  echo "  Start:   sudo systemctl start ${SERVICE_NAME}"
  echo "  Disable: sudo systemctl disable ${SERVICE_NAME}"
  echo " ================================================================="
  echo ""
else
  echo "[ERROR] Service failed to start. Check logs:"
  echo "        journalctl -u ${SERVICE_NAME} -n 50"
  exit 1
fi
