#!/bin/bash

set -e
trap 'echo "An error occurred. Exiting..."; exit 1;' ERR

cd "$(dirname "$0")" || exit 1

echo "Checking for Homebrew..."
if ! command -v brew &>/dev/null; then
  echo "Error: Homebrew is not installed. Please install it from https://brew.sh/ and re-run this script."
  exit 1
fi

echo "Checking for pip3..."
if ! command -v pip3 &>/dev/null; then
  echo "Error: pip3 is not installed. Please install Python 3 and pip3 before continuing."
  exit 1
fi

echo "Installing Python requirements..."
pip3 install -r requirements.txt

echo "Checking for GMP..."
if ! brew list gmp &>/dev/null; then
  echo "Installing GMP via Homebrew..."
  brew install gmp
else
  echo "GMP already installed."
fi

echo "Relinking GMP..."
brew unlink gmp && brew link gmp

ARCH=$(uname -m)
if [ "$ARCH" = "arm64" ]; then
  # Apple Silicon
  export CFLAGS="-I/opt/homebrew/include"
  export LDFLAGS="-L/opt/homebrew/lib"
else
  # Intel
  export CFLAGS="-I/usr/local/include"
  export LDFLAGS="-L/usr/local/lib"
fi

echo "Installing libp2p with correct compile flags..."
pip3 install libp2p==0.2.4

trap - ERR
echo "Setup complete!"
