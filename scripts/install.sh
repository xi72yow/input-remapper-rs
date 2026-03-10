#!/bin/bash
set -euo pipefail

# Install script for the input-remapper-rs APT repository
# Usage: curl -fsSL https://xi72yow.github.io/input-remapper-rs/install.sh | sudo bash

REPO_URL="${REPO_URL:-https://xi72yow.github.io/input-remapper-rs}"

echo "Adding input-remapper-rs APT repository..."

# Download and install the GPG key
curl -fsSL "${REPO_URL}/key.gpg" | gpg --dearmor -o /usr/share/keyrings/input-remapper-rs.gpg

# Add the repository
echo "deb [arch=amd64 signed-by=/usr/share/keyrings/input-remapper-rs.gpg] ${REPO_URL} stable main" \
  > /etc/apt/sources.list.d/input-remapper-rs.list

# Update and install
apt-get update
apt-get install -y input-remapper-rs

echo "input-remapper-rs has been installed successfully!"
echo "The systemd service is enabled. Check status with: systemctl status input-remapper-rs"
