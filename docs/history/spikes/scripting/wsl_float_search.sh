#!/bin/bash
# Run the float search in Monty and in CPython on Linux (WSL Ubuntu).
SPIKE=/mnt/c/Users/Argh/AppData/Local/Temp/claude/2fa6efb36249c319/62c69e06-e270-4185-8fb7-ccd8df9a8baf/scratchpad/spike
PKGS=/tmp/monty_linux_pkgs
if [ ! -x "$PKGS/pydantic_monty_runtime-1.6.0.data/scripts/monty" ]; then
  rm -rf "$PKGS"; mkdir -p "$PKGS"
  for wheel in "$SPIKE"/linux_wheels/*.whl; do
    python3 -c "import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])" "$wheel" "$PKGS"
  done
  chmod +x "$PKGS/pydantic_monty_runtime-1.6.0.data/scripts/monty"
fi
export MONTY_BIN="$PKGS/pydantic_monty_runtime-1.6.0.data/scripts/monty"
export PYTHONPATH="$PKGS"
python3 --version
ldd --version | head -1
python3 -u "$SPIKE/float_search.py" monty "$SPIKE/fs_monty_linux.txt" 5000
python3 -u "$SPIKE/float_search.py" cpython "$SPIKE/fs_cpython_linux.txt" 5000
