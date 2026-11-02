"""THROWAWAY. Checks the 254-bit CHECK lines the bench prints against mpmath at 1000 bits; error in units of the last place (2^-254)."""
import subprocess
import sys
from pathlib import Path
from mpmath import mp, mpf, sin, cos, pi

mp.prec = 1000
executable = Path(__file__).parent / "target" / "release" / ("cordic_bench.exe" if sys.platform == "win32" else "cordic_bench")
output = subprocess.run([str(executable)], capture_output=True, text=True).stdout
worst = 0
for line in output.splitlines():
    if not line.startswith("CHECK"):
        continue
    tag, turns_hex, sine_hex, cosine_hex = line.split()
    limbs = int(tag[5:])
    bits = 64 * limbs
    turns = mpf(int(turns_hex, 16)) / mpf(2) ** bits

    def signed(text):
        value = int(text, 16)
        return value - (1 << bits) if value >> (bits - 1) else value

    expected_sine = sin(2 * pi * turns) * mpf(2) ** (bits - 2)
    expected_cosine = cos(2 * pi * turns) * mpf(2) ** (bits - 2)
    error_sine = abs(mpf(signed(sine_hex)) - expected_sine)
    error_cosine = abs(mpf(signed(cosine_hex)) - expected_cosine)
    worst = max(worst, error_sine, error_cosine)
    print(f"{tag} turns {turns_hex[:12]}...: error {float(error_sine):6.2f} / {float(error_cosine):6.2f} ulp")
print(f"worst error {float(worst):.2f} ulp")
sys.exit(0 if worst < 1000 else 1)
