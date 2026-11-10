"""Throwaway check for the proposal "keep an offset on the rail after the centre of mass moves".

Setup: a ship's centre of mass is on a circular orbit of radius r (voxels = metres) round a point mass, at the speed of the Game's probe
(761.5 m/s at 70,000 m). A block is added so that the centre of mass moves by delta (0.5 m) in the ship, without the ship's blocks moving.

Model A ("refit", the true point-mass physics): the new centre of mass is at old + delta and has the old velocity: it flies a new Kepler orbit from that state.
Model B ("offset", the proposal): the rail is not changed; the new centre of mass is the rail's point plus a constant offset delta (the ship does not turn).
Both keep every block exactly where it was at the instant of the edit. They differ afterwards.
"""
import math

r = 70_000.0
v0 = 761.5
gm = v0 * v0 * r  # circular
n = v0 / r  # rad/s
period = 2 * math.pi / n


def accel(x, y):
    d2 = x * x + y * y
    d3 = d2 * math.sqrt(d2)
    return -gm * x / d3, -gm * y / d3


def rk4(state, dt):
    def f(s):
        ax, ay = accel(s[0], s[1])
        return (s[2], s[3], ax, ay)

    k1 = f(state)
    k2 = f(tuple(s + 0.5 * dt * k for s, k in zip(state, k1)))
    k3 = f(tuple(s + 0.5 * dt * k for s, k in zip(state, k2)))
    k4 = f(tuple(s + dt * k for s, k in zip(state, k3)))
    return tuple(s + dt / 6 * (a + 2 * b + 2 * c + d) for s, a, b, c, d in zip(state, k1, k2, k3, k4))


def run(delta, direction):
    dx, dy = (delta, 0.0) if direction == "radial" else (0.0, delta)
    truth = (r + dx, 0.0 + dy, 0.0, v0)  # new centre of mass, old velocity
    dt = 0.05
    steps_per_orbit = int(period / dt)
    out = []
    t = 0.0
    for orbit in range(1, 21):
        for _ in range(steps_per_orbit):
            truth = rk4(truth, dt)
            t += dt
        # The rail's point (circular) at time t, plus the constant offset.
        rail_x, rail_y = r * math.cos(n * t), r * math.sin(n * t)
        offset_x, offset_y = rail_x + dx, rail_y + dy
        gap = math.hypot(truth[0] - offset_x, truth[1] - offset_y)
        out.append((orbit, t, gap))
    return out


for direction in ("radial", "along-track"):
    print(f"offset {direction}, 0.5 m, r = {r:.0f} m, period {period:.1f} s")
    for orbit, t, gap in run(0.5, direction)[:20:4] + run(0.5, direction)[19:20]:
        print(f"  after {orbit:2d} orbits ({t / 3600:5.2f} h): offset model is {gap:8.3f} m from the true point-mass path")
