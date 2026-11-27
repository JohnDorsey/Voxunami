"""The worked example in the primer of docs/history/seed-secrecy-survey.md. Standard library only; the root is a toy, a real one is random."""
import hashlib, hmac

def sha(data): return hashlib.sha256(data).hexdigest()
def keyed(key, message): return hmac.new(key, message, hashlib.sha256).digest()
def number(value): return value.to_bytes(8, "little")

for text in (b"iron", b"irom"):
    print("sha256", text.decode(), sha(text))
a, b = int(sha(b"iron"), 16), int(sha(b"irom"), 16)
print("bits that differ:", bin(a ^ b).count("1"), "of 256")

root = bytes([42]) * 32
materials = keyed(root, b"materials v1")
terrain = keyed(root, b"terrain v1")
ids = keyed(root, b"ids v1")
for planet in (7, 8):
    planet_key = keyed(materials, number(planet))
    material_key = keyed(planet_key, number(0))
    density = 500 + int.from_bytes(keyed(material_key, b"density")[:8], "little") % 19501
    print("planet", planet)
    print("  terrain seed", keyed(terrain, number(planet))[:8].hex())
    print("  planet key  ", planet_key.hex())
    print("  material key", material_key.hex())
    print("  density     ", density)
    print("  texture seed", keyed(material_key, b"texture")[:8].hex())
    print("  id          ", "world:m" + keyed(ids, number(planet) + number(0))[:16].hex())
