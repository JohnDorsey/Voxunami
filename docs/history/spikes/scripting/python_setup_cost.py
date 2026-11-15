"""How much does Python cost for render-pipeline setup? Pure CPython, no Rust boundary, so the numbers are a floor.

Three shapes of "setup code":
  A. declare-once: a pack declares textures, passes and settings when it loads (Aperture's configurePipeline).
  B. per-frame hook: a function computes a few dozen uniforms every frame (Aperture's onNewFrame; custom uniforms in Iris).
  C. rebuild-per-frame: the whole pipeline description is rebuilt every frame (Frostbite FrameGraph, Unreal RDG style).
Each is timed several times; the median and the worst are printed.
"""
import math
import statistics
import subprocess
import sys
import time


class Builder:
    """Stands in for a builder-style pipeline API: every call records a value and returns self."""

    def __init__(self):
        self.items = []

    def width(self, v):
        self.items.append(("w", v))
        return self

    def height(self, v):
        self.items.append(("h", v))
        return self

    def format(self, v):
        self.items.append(("f", v))
        return self

    def target(self, slot, tex):
        self.items.append(("t", slot, tex))
        return self

    def export_bool(self, name, v):
        self.items.append(("b", name, v))
        return self

    def build(self):
        return tuple(self.items)


def declare_pipeline(textures, passes, settings):
    out = []
    for t in range(textures):
        out.append(Builder().width(1920).height(1080).format("RGBA16F").build())
    for p in range(passes):
        b = Builder().target(0, p % textures).export_bool("fancy", p % 2 == 0)
        for s in range(settings // passes):
            b.export_bool(f"opt{s}", True)
        out.append(b.build())
    return out


def per_frame_uniforms(frame, count):
    """Dozens of values: sun direction from the tick, jitter, exposure curve."""
    t = frame * 0.01667
    values = []
    for i in range(count):
        a = t * (i + 1) * 0.1
        values.append(math.sin(a) * 0.5 + math.cos(a * 1.3) * 0.25 + (i & 7) * 0.125)
    return values


def timed(fn, repeats):
    samples = []
    for _ in range(repeats):
        start = time.perf_counter_ns()
        fn()
        samples.append(time.perf_counter_ns() - start)
    return statistics.median(samples), max(samples)


def show(label, median_ns, worst_ns, per=1):
    print(f"{label:62s} median {median_ns / per / 1000:10.2f} us   worst {worst_ns / per / 1000:10.2f} us")


def interpreter_start(flags):
    samples = []
    for _ in range(10):
        start = time.perf_counter_ns()
        subprocess.run([sys.executable, *flags, "-c", "pass"], check=True)
        samples.append(time.perf_counter_ns() - start)
    return statistics.median(samples) / 1e6, max(samples) / 1e6


if __name__ == "__main__":
    print(sys.version)
    print("(A) declare once at pack load")
    for textures, passes, settings in [(20, 40, 200), (60, 120, 500), (200, 400, 2000)]:
        m, w = timed(lambda: declare_pipeline(textures, passes, settings), 200)
        show(f"  {textures} textures, {passes} passes, {settings} settings", m, w)
    print("(B) per-frame hook")
    for count in (16, 64, 256):
        m, w = timed(lambda: per_frame_uniforms(1234, count), 5000)
        show(f"  {count} uniforms computed", m, w)
    print("(C) rebuild the whole description every frame")
    for textures, passes, settings in [(20, 40, 200), (60, 120, 500)]:
        m, w = timed(lambda: declare_pipeline(textures, passes, settings), 500)
        show(f"  {textures} textures, {passes} passes, {settings} settings", m, w)
    print("(D) call overhead, the floor")
    m, w = timed(lambda: [per_frame_uniforms(1, 1) for _ in range(1000)], 50)
    show("  one tiny Python function call with a loop of 1 (per call)", m, w, per=1000)
    print("(E) interpreter start-up in a fresh process (ms): median, worst")
    for flags in ([], ["-S"], ["-I", "-S"]):
        m, w = interpreter_start(flags)
        print(f"  python {' '.join(flags) or '(defaults)':10s} median {m:7.1f} ms   worst {w:7.1f} ms")
