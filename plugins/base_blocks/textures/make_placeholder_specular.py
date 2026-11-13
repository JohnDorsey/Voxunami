"""Makes lamp_s.png and leaves_s.png, the specular pictures (LabPBR `_s`) of lamp.png and leaves.png: PLACEHOLDERS, drawn from the pictures themselves until the owner's art replaces them.

Run it from this folder (`python make_placeholder_specular.py`). Nothing but the writing of the files is done by a library, so the result does not depend on the machine.

The channels are LabPBR's: red is the smoothness and green the reflectance (both 0 here: no glint), blue from 65 up is the share of the sun's light the pixel lets through
(`(byte - 64) / 191`; up to 64 is porosity, which is not read), and alpha is the emission (0 to 254 is none to all; 0 is none here).

- The lamp glows where its picture is bright: none below a luminance of 190, all from 240, smoothly between (so the rim of the lamp stays dark in the dark and its middle shines).
- The leaves let light through by 0.55 to 1: the brighter a leaf pixel is, the thinner it is taken to be. A hole of the picture (clear) is given the mean of the leaf pixels, since
  the smaller levels of a companion are the plain average of four pixels and a hole's 0 would drain the scatter out of the far levels.
"""

from PIL import Image

LAMP_NO_GLOW_BELOW = 190.0
LAMP_FULL_GLOW_FROM = 240.0
LEAF_LEAST_SCATTER = 0.55


def luminance(pixel):
    """The brightness of an sRGB pixel by Rec. 709 weights on the bytes as they are (a rough measure; it only orders the pixels)."""
    return 0.2126 * pixel[0] + 0.7152 * pixel[1] + 0.0722 * pixel[2]


def smoothstep(low, high, value):
    t = min(max((value - low) / (high - low), 0.0), 1.0)
    return t * t * (3.0 - 2.0 * t)


def lamp_specular(picture):
    """The lamp's: alpha is the glow of each pixel."""
    result = Image.new("RGBA", picture.size)
    for y in range(picture.height):
        for x in range(picture.width):
            glow = smoothstep(LAMP_NO_GLOW_BELOW, LAMP_FULL_GLOW_FROM, luminance(picture.getpixel((x, y))))
            result.putpixel((x, y), (0, 0, 0, round(glow * 254)))
    return result


def leaves_specular(picture):
    """The leaves': blue is how much each leaf pixel lets through."""
    opaque = [
        luminance(picture.getpixel((x, y)))
        for y in range(picture.height)
        for x in range(picture.width)
        if picture.getpixel((x, y))[3] > 0
    ]
    darkest, brightest = min(opaque), max(opaque)
    blues = {}
    for y in range(picture.height):
        for x in range(picture.width):
            if picture.getpixel((x, y))[3] > 0:
                thinness = (luminance(picture.getpixel((x, y))) - darkest) / (brightest - darkest)
                share = LEAF_LEAST_SCATTER + (1.0 - LEAF_LEAST_SCATTER) * thinness
                blues[(x, y)] = 64 + round(share * 191)
    mean_blue = round(sum(blues.values()) / len(blues))
    result = Image.new("RGBA", picture.size)
    for y in range(picture.height):
        for x in range(picture.width):
            result.putpixel((x, y), (0, 0, blues.get((x, y), mean_blue), 0))
    return result


if __name__ == "__main__":
    lamp_specular(Image.open("lamp.png").convert("RGBA")).save("lamp_s.png")
    leaves_specular(Image.open("leaves.png").convert("RGBA")).save("leaves_s.png")
