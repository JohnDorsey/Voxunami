"""Tests for painting the PBR channels of VirtualAtlas: what each byte of a companion file means (the codecs), how a blend
mode combines a painted value with what is there, and how a whole stroke changes the planes of a tile. No window.

The numbers are worked out by hand in the tests (a smoothness of 0.2 is the byte 51), not taken from the program.

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import sys
import unittest
from pathlib import Path

import numpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_pbr_files import (  # noqa: E402
    EMISSION,
    HEIGHT,
    LABPBR,
    NORMAL_X,
    NORMAL_Y,
    OCCLUSION,
    POROSITY,
    REFLECTANCE,
    SMOOTHNESS,
    neutral_planes,
)
from test_virtual_atlas import va  # noqa: E402


def bytes_of(*values):
    return numpy.array(values, dtype=numpy.uint8)


def one_row(old_bytes):
    """A (1, n) plane of bytes."""
    return bytes_of(*old_bytes).reshape(1, -1)


def full_coverage(shape):
    return numpy.ones(shape, dtype=numpy.float64)


class CodecTests(unittest.TestCase):
    def decode(self, identity, *values):
        return va.CHANNEL_CODECS[identity].decode(bytes_of(*values)).tolist()

    def encode(self, identity, *values):
        return va.CHANNEL_CODECS[identity].encode(numpy.array(values, dtype=numpy.float64)).tolist()

    def test_every_channel_of_every_profile_has_a_codec(self):
        for profile in va.PBR_PROFILES.values():
            for channel in profile.channels:
                self.assertIn(channel.identity, va.CHANNEL_CODECS, f"{profile.name}: {channel.identity}")

    def test_the_neutral_byte_of_every_channel_decodes_to_its_neutral_meaning(self):
        neutral_meaning = {
            "smoothness": 0.0, "reflectance": 0.0, "porosity_scattering": 0.0, "emission": 0.0,
            "normal_x": 0.0, "normal_y": 0.0, "ambient_occlusion": 0.0, "height": 1.0,
        }
        for channel in va.PBR_PROFILES[LABPBR].channels:
            decoded = va.CHANNEL_CODECS[channel.identity].decode(bytes_of(channel.neutral))
            self.assertEqual(decoded.tolist(), [neutral_meaning[channel.identity]], channel.identity)

    def test_every_byte_comes_back_the_same_after_decode_and_encode_except_two_known_cases(self):
        everything = numpy.arange(256, dtype=numpy.uint8)
        for identity, codec in va.CHANNEL_CODECS.items():
            again = codec.encode(codec.decode(everything))
            different = numpy.flatnonzero(again != everything).tolist()
            if identity in ("normal_x", "normal_y"):
                # Byte 0 and byte 1 both mean "tilted all the way back"; the byte written for it is 1.
                self.assertEqual(different, [0], identity)
                self.assertEqual(int(again[0]), 1, identity)
            elif identity == "reflectance":
                # A level of a dielectric stops at 229; the bytes from 230 are the metals, which a level never makes.
                self.assertEqual(different, list(range(230, 256)), identity)
                self.assertTrue((again[230:] == 229).all(), identity)
            else:
                self.assertEqual(different, [], identity)

    def test_smoothness_is_the_byte_over_255(self):
        self.assertEqual(self.decode("smoothness", 0, 255), [0.0, 1.0])
        self.assertEqual(self.encode("smoothness", 0.0, 0.2, 1.0), [0, 51, 255])

    def test_porosity_scattering_is_one_scale_over_the_whole_byte(self):
        self.assertEqual(self.encode("porosity_scattering", 0.2), [51])
        self.assertEqual(self.decode("porosity_scattering", 255), [1.0])

    def test_emission_255_is_none_and_the_other_bytes_are_one_step_up_from_zero(self):
        self.assertEqual(self.decode("emission", 255), [0.0])
        self.assertEqual(self.decode("emission", 254), [1.0])
        self.assertEqual(self.decode("emission", 0), [1 / 255])
        self.assertEqual(self.encode("emission", 0.0), [255])
        self.assertEqual(self.encode("emission", 1.0), [254])
        self.assertEqual(self.encode("emission", 1 / 255), [0])
        self.assertEqual(self.encode("emission", 0.2), [50])

    def test_normal_axes_are_zero_at_the_neutral_byte_and_one_at_each_end(self):
        self.assertEqual(self.decode("normal_x", 128, 255, 1, 0), [0.0, 1.0, -1.0, -1.0])
        self.assertEqual(self.encode("normal_x", 0.0, 1.0, -1.0, 0.4), [128, 255, 1, 179])
        self.assertEqual(self.encode("normal_y", 2.0, -2.0), [255, 1])  # beyond the range is the end of it

    def test_occlusion_is_how_much_light_is_lost_so_the_neutral_255_is_zero(self):
        self.assertEqual(self.decode("ambient_occlusion", 255, 0), [0.0, 1.0])
        self.assertEqual(self.encode("ambient_occlusion", 0.0, 1.0, 0.2), [255, 0, 204])

    def test_height_is_the_byte_over_255_and_the_neutral_is_the_top(self):
        self.assertEqual(self.decode("height", 255, 0), [1.0, 0.0])
        self.assertEqual(self.encode("height", 1.0, 0.2), [255, 51])

    def test_a_dielectric_reflectance_stops_at_229_and_the_metals_start_at_230(self):
        self.assertEqual(va.METAL_FIRST_BYTE, 230)
        self.assertEqual(va.METAL_COUNT, 26)
        self.assertEqual(self.encode("reflectance", 1.0, 0.0, 0.2), [229, 0, 51])
        self.assertEqual(self.decode("reflectance", 229, 240), [229 / 255, 240 / 255])


class BlendTests(unittest.TestCase):
    OLD = [0, 51, 102, 255]

    def blend(self, mode, value=0.2, coverage=1.0, old=None, identity="smoothness"):
        old_plane = one_row(self.OLD if old is None else old)
        return va.blend_channel(identity, old_plane, value, mode, numpy.full(old_plane.shape, coverage)).ravel().tolist()

    def test_the_modes_are_these_seven_in_this_order(self):
        self.assertEqual(va.BLEND_MODES, ("none", "replace", "add", "subtract", "multiply", "min", "max"))

    def test_none_changes_nothing(self):
        self.assertEqual(self.blend("none"), self.OLD)

    def test_replace_gives_every_pixel_the_value(self):
        self.assertEqual(self.blend("replace"), [51, 51, 51, 51])

    def test_add_adds_the_value_and_stops_at_the_top(self):
        self.assertEqual(self.blend("add"), [51, 102, 153, 255])

    def test_subtract_takes_the_value_away_and_stops_at_the_bottom(self):
        self.assertEqual(self.blend("subtract"), [0, 0, 51, 204])

    def test_multiply_multiplies_the_meanings_not_the_bytes(self):
        self.assertEqual(self.blend("multiply"), [0, 10, 20, 51])

    def test_min_and_max_keep_the_smaller_and_the_larger(self):
        self.assertEqual(self.blend("min"), [0, 51, 51, 51])
        self.assertEqual(self.blend("max"), [51, 51, 102, 255])

    def test_partial_coverage_goes_that_far_from_what_was_there(self):
        # 0.25 of the way from 0 to 0.2 is 0.05, which is the byte 12.75, written as 13.
        self.assertEqual(self.blend("replace", coverage=0.25, old=[0, 255]), [13, 255 - 51])  # 255 + 0.25 * (51 - 255) = 204

    def test_no_coverage_leaves_the_byte_exactly_even_when_it_would_be_written_differently(self):
        # Under the normal codec the byte 0 is written back as 1, so a pixel that is not touched must not be rewritten.
        self.assertEqual(self.blend("add", value=0.0, coverage=0.0, old=[0, 1, 128], identity="normal_x"), [0, 1, 128])
        self.assertEqual(self.blend("add", value=0.0, coverage=1.0, old=[0, 1, 128], identity="normal_x"), [1, 1, 128])

    def test_the_value_of_a_normal_axis_is_its_meaning_from_minus_one_to_one(self):
        self.assertEqual(self.blend("add", value=0.4, old=[128, 255], identity="normal_x"), [179, 255])
        self.assertEqual(self.blend("subtract", value=1.0, old=[128, 255], identity="normal_x"), [1, 128])

    def test_adding_to_occlusion_darkens_and_so_lowers_the_byte(self):
        self.assertEqual(self.blend("add", value=0.2, old=[255, 51], identity="ambient_occlusion"), [204, 0])

    def test_emission_painted_to_zero_is_none_again(self):
        self.assertEqual(self.blend("replace", value=0.0, old=[10, 254], identity="emission"), [255, 255])

    def test_an_unknown_mode_is_an_error(self):
        with self.assertRaises(ValueError):
            self.blend("overlay")

    def test_a_level_of_reflectance_never_makes_a_metal(self):
        self.assertEqual(self.blend("replace", value=1.0, old=[0, 240], identity="reflectance"), [229, 229])
        self.assertEqual(self.blend("add", value=0.9, old=[200, 240], identity="reflectance"), [229, 229])

    def test_a_metal_is_written_as_its_byte_whatever_the_mode_except_none(self):
        for mode in ("replace", "add", "subtract", "multiply", "min", "max"):
            self.assertEqual(self.blend(mode, value=va.Metal(5), old=[0, 100], identity="reflectance"), [235, 235], mode)
        self.assertEqual(self.blend("none", value=va.Metal(5), old=[0, 100], identity="reflectance"), [0, 100])

    def test_a_metal_must_be_one_of_the_26(self):
        for bad in (-1, 26):
            with self.assertRaises(ValueError):
                va.Metal(bad)
        self.assertEqual(va.Metal(25).byte, 255)
        self.assertEqual(va.Metal(0).byte, 230)

    def test_a_metal_is_only_for_reflectance(self):
        with self.assertRaises(ValueError):
            self.blend("replace", value=va.Metal(0), identity="smoothness")


class StrokeTests(unittest.TestCase):
    PROFILE = va.PBR_PROFILES[LABPBR]

    def settings(self, **blends):
        return {identity: va.ChannelBlend(mode, value) for identity, (mode, value) in blends.items()}

    def stroke(self, planes, **blends):
        return va.ChannelStroke(self.PROFILE, planes, self.settings(**blends))

    def test_with_every_mode_none_the_planes_are_not_changed(self):
        planes = neutral_planes(4)
        planes[1, 2, SMOOTHNESS] = 77
        stroke = self.stroke(planes, smoothness=("none", 0.5))
        stroke.add_coverage(full_coverage((4, 4)))
        self.assertTrue(numpy.array_equal(stroke.planes, planes))
        self.assertFalse(stroke.changed)

    def test_only_the_channels_with_a_mode_change_and_only_where_the_stroke_went(self):
        planes = neutral_planes(4)
        stroke = self.stroke(planes, smoothness=("replace", 0.2), emission=("replace", 0.2))
        coverage = numpy.zeros((4, 4))
        coverage[1, 2] = 1.0
        stroke.add_coverage(coverage)
        expected = neutral_planes(4)
        expected[1, 2, SMOOTHNESS] = 51
        expected[1, 2, EMISSION] = 50
        self.assertTrue(numpy.array_equal(stroke.planes, expected))
        self.assertTrue(stroke.changed)

    def test_the_planes_given_are_not_changed_by_the_stroke(self):
        planes = neutral_planes(4)
        before = planes.copy()
        stroke = self.stroke(planes, smoothness=("replace", 1.0))
        stroke.add_coverage(full_coverage((4, 4)))
        self.assertTrue(numpy.array_equal(planes, before))
        self.assertEqual(int(stroke.planes[0, 0, SMOOTHNESS]), 255)

    def test_passing_over_a_pixel_again_adds_only_once(self):
        planes = neutral_planes(4)
        stroke = self.stroke(planes, smoothness=("add", 0.2))
        for _ in range(5):
            stroke.add_coverage(full_coverage((4, 4)))
        self.assertEqual(int(stroke.planes[0, 0, SMOOTHNESS]), 51)

    def test_a_pixel_keeps_the_strongest_coverage_any_pass_gave_it(self):
        planes = neutral_planes(4)
        stroke = self.stroke(planes, smoothness=("replace", 0.2))
        stroke.add_coverage(numpy.full((4, 4), 0.25))
        stroke.add_coverage(numpy.full((4, 4), 0.1))
        self.assertEqual(int(stroke.planes[0, 0, SMOOTHNESS]), 13)
        stroke.add_coverage(numpy.full((4, 4), 1.0))
        self.assertEqual(int(stroke.planes[0, 0, SMOOTHNESS]), 51)

    def test_two_strokes_in_a_row_add_twice(self):
        planes = neutral_planes(4)
        first = self.stroke(planes, smoothness=("add", 0.2))
        first.add_coverage(full_coverage((4, 4)))
        second = self.stroke(first.planes, smoothness=("add", 0.2))
        second.add_coverage(full_coverage((4, 4)))
        self.assertEqual(int(second.planes[0, 0, SMOOTHNESS]), 102)

    def test_a_painted_normal_longer_than_one_is_shrunk_to_one_on_both_axes(self):
        planes = neutral_planes(4)
        stroke = self.stroke(planes, normal_x=("replace", 1.0), normal_y=("replace", 1.0))
        stroke.add_coverage(full_coverage((4, 4)))
        # 1 / sqrt(2) = 0.70711 of the way out: 128 + 0.70711 * 127 = 217.8.
        self.assertEqual(stroke.planes[0, 0, NORMAL_X], 218)
        self.assertEqual(stroke.planes[0, 0, NORMAL_Y], 218)

    def test_painting_only_one_axis_can_shrink_the_other(self):
        planes = neutral_planes(4)
        planes[:, :, NORMAL_Y] = 255
        stroke = self.stroke(planes, normal_x=("replace", 1.0))
        stroke.add_coverage(full_coverage((4, 4)))
        self.assertEqual(stroke.planes[0, 0, NORMAL_X], 218)
        self.assertEqual(stroke.planes[0, 0, NORMAL_Y], 218)

    def test_a_normal_that_is_not_longer_than_one_is_not_changed(self):
        planes = neutral_planes(4)
        stroke = self.stroke(planes, normal_x=("replace", 0.4))
        stroke.add_coverage(full_coverage((4, 4)))
        self.assertEqual(stroke.planes[0, 0, NORMAL_X], 179)
        self.assertEqual(stroke.planes[0, 0, NORMAL_Y], 128)

    def test_a_pixel_the_stroke_did_not_touch_keeps_its_long_normal(self):
        planes = neutral_planes(4)
        planes[:, :, NORMAL_X] = 255
        planes[:, :, NORMAL_Y] = 255
        stroke = self.stroke(planes, smoothness=("replace", 1.0))
        coverage = numpy.zeros((4, 4))
        coverage[0, 0] = 1.0
        stroke.add_coverage(coverage)
        self.assertEqual(stroke.planes[3, 3, NORMAL_X], 255)
        self.assertEqual(stroke.planes[3, 3, NORMAL_Y], 255)
        # The pixel it touched is shortened, though the stroke painted another channel there.
        self.assertEqual(stroke.planes[0, 0, NORMAL_X], 218)

    def test_a_stroke_with_no_channel_painted_does_not_shorten_anything(self):
        planes = neutral_planes(4)
        planes[:, :, NORMAL_X] = 255
        planes[:, :, NORMAL_Y] = 255
        stroke = self.stroke(planes, smoothness=("none", 1.0))
        stroke.add_coverage(full_coverage((4, 4)))
        self.assertEqual(stroke.planes[0, 0, NORMAL_X], 255)

    def test_a_metal_of_reflectance_goes_through_a_stroke(self):
        stroke = self.stroke(neutral_planes(2), reflectance=("replace", va.Metal(3)))
        stroke.add_coverage(full_coverage((2, 2)))
        self.assertEqual(int(stroke.planes[0, 0, REFLECTANCE]), 233)

    def test_an_unknown_channel_or_mode_is_refused_at_once(self):
        with self.assertRaises(ValueError):
            self.stroke(neutral_planes(2), colour=("replace", 1.0))
        with self.assertRaises(ValueError):
            self.stroke(neutral_planes(2), smoothness=("overlay", 1.0))

    def test_a_channel_the_profile_does_not_have_is_refused_even_though_it_has_a_codec(self):
        none = va.PBR_PROFILES["none"]
        with self.assertRaises(ValueError):
            va.ChannelStroke(none, va.neutral_planes(none, 2), {"smoothness": va.ChannelBlend("replace", 1.0)})

    def test_a_stroke_under_none_has_no_planes_to_change(self):
        planes = va.neutral_planes(va.PBR_PROFILES["none"], 4)
        stroke = va.ChannelStroke(va.PBR_PROFILES["none"], planes, {})
        stroke.add_coverage(full_coverage((4, 4)))
        self.assertEqual(stroke.planes.shape, (4, 4, 0))
        self.assertFalse(stroke.changed)


class WindowedWorkTests(unittest.TestCase):
    """A stroke works out only the box around what it covers; the result must be what working out every pixel gives."""

    PROFILE = va.PBR_PROFILES[LABPBR]

    def test_the_planes_are_what_blending_each_whole_channel_gives_for_random_strokes(self):
        generator = numpy.random.default_rng(7)
        modes = ("replace", "add", "subtract", "multiply", "min", "max")
        not_normals = [c.identity for c in self.PROFILE.channels if not c.identity.startswith("normal")]
        for trial in range(30):
            planes = generator.integers(0, 256, size=(9, 11, 8), dtype=numpy.uint8)
            coverage = numpy.zeros((9, 11))
            row, column = generator.integers(0, 6), generator.integers(0, 8)
            coverage[row : row + 3, column : column + 4] = generator.random((3, 4))
            blends = {
                identity: va.ChannelBlend(str(generator.choice(modes)), float(generator.random()))
                for identity in not_normals
            }
            stroke = va.ChannelStroke(self.PROFILE, planes, blends)
            stroke.set_coverage(coverage)
            for index, channel in enumerate(self.PROFILE.channels):
                if channel.identity in blends:
                    blend = blends[channel.identity]
                    expected = va.blend_channel(channel.identity, planes[:, :, index], blend.value, blend.mode, coverage)
                else:
                    expected = planes[:, :, index]
                if channel.identity.startswith("normal"):
                    continue
                self.assertTrue(numpy.array_equal(stroke.planes[:, :, index], expected), f"trial {trial}: {channel.identity}")

    def test_a_stroke_in_the_middle_of_a_canvas_changes_nothing_outside_its_box(self):
        planes = numpy.full((9, 11, 8), 77, dtype=numpy.uint8)
        stroke = va.ChannelStroke(self.PROFILE, planes, {"smoothness": va.ChannelBlend("replace", 0.0)})
        coverage = numpy.zeros((9, 11))
        coverage[4, 5] = 1.0
        stroke.add_coverage(coverage)
        changed = numpy.argwhere(numpy.any(stroke.planes != planes, axis=2))
        self.assertEqual(changed.tolist(), [[4, 5]])


class ResetTests(unittest.TestCase):
    PROFILE = va.PBR_PROFILES[LABPBR]

    def painted(self):
        planes = neutral_planes(2)
        planes[:, :, SMOOTHNESS] = 200
        planes[:, :, EMISSION] = 100
        planes[:, :, NORMAL_X] = 200
        planes[:, :, OCCLUSION] = 40
        planes[:, :, HEIGHT] = 10
        planes[:, :, POROSITY] = 90
        return planes

    def test_full_coverage_gives_every_channel_its_neutral_byte(self):
        reset = va.reset_channels(self.PROFILE, self.painted(), full_coverage((2, 2)))
        self.assertTrue(numpy.array_equal(reset, neutral_planes(2)))

    def test_partial_coverage_goes_that_far_toward_neutral(self):
        reset = va.reset_channels(self.PROFILE, self.painted(), numpy.full((2, 2), 0.25))
        # 200 - 0.25 * 200 = 150; 100 + 0.25 * 155 = 138.75; 200 - 0.25 * 72 = 182; 40 + 0.25 * 215 = 93.75; 10 + 0.25 * 245 = 71.25.
        self.assertEqual(reset[0, 0].tolist(), [150, 0, 68, 139, 182, 128, 94, 71])

    def test_no_coverage_changes_nothing_and_the_given_planes_are_not_touched(self):
        planes = self.painted()
        before = planes.copy()
        reset = va.reset_channels(self.PROFILE, planes, numpy.zeros((2, 2)))
        self.assertTrue(numpy.array_equal(reset, before))
        reset[0, 0, 0] = 1
        self.assertTrue(numpy.array_equal(planes, before))

    def test_only_the_covered_pixels_are_reset(self):
        coverage = numpy.zeros((2, 2))
        coverage[0, 1] = 1.0
        reset = va.reset_channels(self.PROFILE, self.painted(), coverage)
        self.assertEqual(reset[0, 1].tolist(), neutral_planes(1)[0, 0].tolist())
        self.assertEqual(reset[0, 0, SMOOTHNESS], 200)


class SampleTests(unittest.TestCase):
    PROFILE = va.PBR_PROFILES[LABPBR]

    def test_the_values_of_a_neutral_pixel_are_the_neutral_meanings(self):
        values = va.sample_channel_values(self.PROFILE, neutral_planes(2), 1, 0)
        self.assertEqual(
            values,
            {"smoothness": 0.0, "reflectance": 0.0, "porosity_scattering": 0.0, "emission": 0.0,
             "normal_x": 0.0, "normal_y": 0.0, "ambient_occlusion": 0.0, "height": 1.0},
        )

    def test_a_pixel_gives_back_what_a_stroke_would_have_painted_to_get_it(self):
        planes = neutral_planes(2)
        planes[0, 1] = [51, 100, 255, 50, 179, 1, 204, 51]
        values = va.sample_channel_values(self.PROFILE, planes, 0, 1)
        self.assertEqual(values["smoothness"], 0.2)
        self.assertEqual(values["reflectance"], 100 / 255)
        self.assertEqual(values["emission"], 51 / 255)
        self.assertEqual(values["normal_y"], -1.0)
        self.assertEqual(values["ambient_occlusion"], 0.2)
        encoded = [int(va.CHANNEL_CODECS[identity].encode(numpy.array([value]))[0]) for identity, value in values.items()]
        self.assertEqual(encoded, planes[0, 1].tolist())

    def test_a_metal_byte_is_given_as_the_metal(self):
        planes = neutral_planes(2)
        planes[0, 0, REFLECTANCE] = 240
        self.assertEqual(va.sample_channel_values(self.PROFILE, planes, 0, 0)["reflectance"], va.Metal(10))

    def test_the_first_metal_byte_is_a_metal_and_the_byte_before_it_is_a_level(self):
        planes = neutral_planes(2)
        planes[0, 0, REFLECTANCE] = 229
        planes[0, 1, REFLECTANCE] = 230
        self.assertEqual(va.sample_channel_values(self.PROFILE, planes, 0, 0)["reflectance"], 229 / 255)
        self.assertEqual(va.sample_channel_values(self.PROFILE, planes, 0, 1)["reflectance"], va.Metal(0))

    def test_the_planes_of_a_profile_with_no_channels_have_no_values(self):
        planes = va.neutral_planes(va.PBR_PROFILES["none"], 2)
        self.assertEqual(va.sample_channel_values(va.PBR_PROFILES["none"], planes, 0, 0), {})


if __name__ == "__main__":
    unittest.main()
