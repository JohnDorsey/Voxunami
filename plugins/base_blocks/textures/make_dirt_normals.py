"""Makes dirt_n.png, the normal picture (LabPBR `_n`) of dirt.png: a height field of noise, as a normal map.

The picture is 16 by 16 like dirt.png and tiles without a seam. Run it from this folder (`python make_dirt_normals.py`); the result does not depend on the machine (the noise is
splitmix64 from a fixed seed, and no library does any of the arithmetic but the writing of the file).

The channels are LabPBR's: red and green are the normal's x (to the right in the picture) and y (toward its top) as 128 plus or minus 127 for a tilt of 1 either way, blue is
the ambient occlusion (255: none) and alpha is the height (0 to 255).
"""

import math

from PIL import Image

SIDE = 16
SEED = 20261112
# How steep the steepest pixel is allowed to be (a tilt of 1 is a normal lying along the face), and the tilt a pixel's neighbours' height difference makes.
LARGEST_TILT = 0.6
STRENGTH = 1.6


def splitmix64(state):
    """The next random 64-bit number and the next state."""
    state = (state + 0x9E3779B97F4A7C15) & 0xFFFFFFFFFFFFFFFF
    value = state
    value = ((value ^ (value >> 30)) * 0xBF58476D1CE4E5B9) & 0xFFFFFFFFFFFFFFFF
    value = ((value ^ (value >> 27)) * 0x94D049BB133111EB) & 0xFFFFFFFFFFFFFFFF
    return value ^ (value >> 31), state


def noise():
    """A SIDE by SIDE list of numbers between 0 and 1."""
    state = SEED
    rows = []
    for _ in range(SIDE):
        row = []
        for _ in range(SIDE):
            value, state = splitmix64(state)
            row.append((value >> 11) / float(1 << 53))
        rows.append(row)
    return rows


def blurred(heights):
    """Each pixel mixed with its eight neighbours (1 2 1 over 2 4 2 over 1 2 1, wrapping round the edges), so that the noise has some grain to it and is not salt and pepper."""
    weights = {(-1, -1): 1, (0, -1): 2, (1, -1): 1, (-1, 0): 2, (0, 0): 4, (1, 0): 2, (-1, 1): 1, (0, 1): 2, (1, 1): 1}
    result = []
    for y in range(SIDE):
        row = []
        for x in range(SIDE):
            total = sum(weight * heights[(y + dy) % SIDE][(x + dx) % SIDE] for (dx, dy), weight in weights.items())
            row.append(total / 16.0)
        result.append(row)
    return result


def stretched(heights):
    """The heights spread over 0 to 1."""
    low = min(min(row) for row in heights)
    high = max(max(row) for row in heights)
    return [[(height - low) / (high - low) for height in row] for row in heights]


def main():
    heights = stretched(blurred(noise()))
    image = Image.new("RGBA", (SIDE, SIDE))
    for y in range(SIDE):
        for x in range(SIDE):
            # The slope of the height to the right, and down the rows (rows count from the top, so the slope toward the top of the picture is the other way round).
            to_the_right = (heights[y][(x + 1) % SIDE] - heights[y][(x - 1) % SIDE]) / 2.0
            down_the_rows = (heights[(y + 1) % SIDE][x] - heights[(y - 1) % SIDE][x]) / 2.0
            tilt_x = -to_the_right * STRENGTH
            tilt_y = down_the_rows * STRENGTH
            steepness = math.hypot(tilt_x, tilt_y)
            if steepness > LARGEST_TILT:
                tilt_x *= LARGEST_TILT / steepness
                tilt_y *= LARGEST_TILT / steepness
            red = round(128 + 127 * tilt_x)
            green = round(128 + 127 * tilt_y)
            image.putpixel((x, y), (red, green, 255, round(255 * heights[y][x])))
    image.save("dirt_n.png")


if __name__ == "__main__":
    main()
