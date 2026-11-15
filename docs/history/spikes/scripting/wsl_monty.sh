#!/bin/bash
# Run the Monty float/hash probe on Linux (WSL Ubuntu) to compare with the Windows output.
# python3-venv is missing there, so the wheels were downloaded on Windows and are unpacked into /tmp.
SPIKE=/mnt/c/Users/Argh/AppData/Local/Temp/claude/2fa6efb36249c319/62c69e06-e270-4185-8fb7-ccd8df9a8baf/scratchpad/spike
PKGS=/tmp/monty_linux_pkgs
uname -a
python3 --version
rm -rf "$PKGS"
mkdir -p "$PKGS"
for wheel in "$SPIKE"/linux_wheels/*.whl; do
  python3 -c "import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])" "$wheel" "$PKGS"
done
find "$PKGS" -iname 'monty*' -maxdepth 3 | head -20
BINARY="$PKGS/pydantic_monty_runtime-1.6.0.data/scripts/monty"
echo "binary: $BINARY"
chmod +x "$BINARY"
export MONTY_BIN="$BINARY"
export PYTHONPATH="$PKGS"
python3 -I -c "import sys; sys.path.insert(0, '$PKGS'); import pydantic_monty; print('pydantic_monty', pydantic_monty.__file__)"
python3 -u -c "
import sys
sys.path.insert(0, '$PKGS')
sys.argv = ['monty_floats.py']
exec(open('$SPIKE/monty_floats.py').read())
" > "$SPIKE/monty_floats_linux.txt" 2>&1
echo "probe exit=$?"
