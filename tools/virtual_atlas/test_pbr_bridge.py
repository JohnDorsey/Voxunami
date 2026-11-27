"""Tests for the channels on the bridge: the program says which PBR convention it reads in `hello` (optional `profile`), VirtualAtlas
answers with its own setting in `welcome`, and a `tile` carries the companion pictures (`companions`: {"_s": base64, "_n": base64}) when
both use the same convention. A file that would not exist (every channel in it neutral) has no key, and the program takes that as neutral.

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import base64
import sys
import unittest
from pathlib import Path

import numpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_pbr_files import EMISSION, LABPBR, NORMAL_X, SMOOTHNESS  # noqa: E402
from test_pbr_panel import PanelDriver  # noqa: E402
from test_virtual_atlas import va  # noqa: E402

CATALOGUE = {"type": "tiles", "tiles": [{"id": "first", "files": ["a"], "size": 8, "group": "game"}]}


class BridgeChannelsFixture(PanelDriver):
    def setUp(self):
        super().setUp()
        self.controller = va.BridgeController(self.project, self.editor)

    def connect(self, profile=LABPBR):
        hello = {"type": "hello", "protocol": 1, "host": "A Game"}
        if profile is not None:
            hello["profile"] = profile
        self.controller.receive(hello)
        self.controller.receive(CATALOGUE)
        return self.controller.take_outgoing()

    def paint(self, **planes):
        """Give the tile a channel value directly (as a stroke that ended would have)."""
        record = self.project.tiles[(0, 0)]
        for identity, value in planes.items():
            record.planes[2, 3, IDENTITIES.index(identity)] = value
        record.revision += 1

    def paint_pixel(self):
        record = self.project.tiles[(0, 0)]
        record.pixels[0, 0] = (1, 2, 3, 4)
        record.revision += 1

    def tiles_told(self):
        return [message for message in self.controller.take_outgoing() if message["type"] == "tile"]


IDENTITIES = [channel.identity for channel in va.PBR_PROFILES[LABPBR].channels]


class WelcomeTests(BridgeChannelsFixture):
    def test_welcome_carries_the_setting_when_it_has_channels(self):
        welcome = self.connect()[0]
        self.assertEqual(welcome["type"], "welcome")
        self.assertEqual(welcome["profile"], LABPBR)

    def test_welcome_has_no_profile_under_none_and_so_is_what_it_always_was(self):
        self.project.switch_pbr_profile("none")
        self.editor.sync()
        welcome = self.connect(profile=None)[0]
        self.assertEqual(welcome, {"type": "welcome", "protocol": 1, "tool": "VirtualAtlas", "tile_size": 8})

    def test_a_profile_that_is_not_text_is_taken_as_none_given(self):
        self.controller.receive({"type": "hello", "protocol": 1, "host": "A Game", "profile": 7})
        self.controller.receive(CATALOGUE)
        self.controller.take_outgoing()
        self.paint(smoothness=9)
        self.assertEqual(self.tiles_told(), [], "a channel alone is nothing to tell a program that gets no companions")
        self.paint_pixel()
        self.assertNotIn("companions", self.tiles_told()[0])


class CompanionsTests(BridgeChannelsFixture):
    def test_a_channel_that_changed_is_told_with_the_companion_file_that_holds_it(self):
        self.connect()
        self.paint(smoothness=51)
        told = self.tiles_told()
        self.assertEqual(len(told), 1)
        companions = told[0]["companions"]
        self.assertEqual(list(companions), ["_s"])
        picture = numpy.frombuffer(base64.b64decode(companions["_s"]), dtype=numpy.uint8).reshape(8, 8, 4)
        self.assertEqual(picture[2, 3].tolist(), [51, 0, 0, 255])
        self.assertEqual(picture[0, 0].tolist(), [0, 0, 0, 255], "the neutral pixels of the file")

    def test_the_two_files_are_told_when_both_have_data(self):
        self.connect()
        self.paint(smoothness=51, normal_x=200)
        companions = self.tiles_told()[0]["companions"]
        self.assertEqual(sorted(companions), ["_n", "_s"])
        normal = numpy.frombuffer(base64.b64decode(companions["_n"]), dtype=numpy.uint8).reshape(8, 8, 4)
        self.assertEqual(normal[2, 3].tolist(), [200, 128, 255, 255])

    def test_a_file_whose_channels_are_all_neutral_has_no_key(self):
        self.connect()
        self.paint(normal_x=200)
        self.assertEqual(list(self.tiles_told()[0]["companions"]), ["_n"])

    def test_painting_everything_back_to_neutral_tells_an_empty_set_so_the_program_clears_its_own(self):
        self.connect()
        self.paint(smoothness=51)
        self.tiles_told()
        self.paint(smoothness=0)
        told = self.tiles_told()
        self.assertEqual(len(told), 1)
        self.assertEqual(told[0]["companions"], {})

    def test_a_change_of_a_channel_alone_is_told_though_the_pixels_are_the_same(self):
        self.connect()
        self.paint(emission=50)
        told = self.tiles_told()
        self.assertEqual(len(told), 1)
        self.assertEqual(
            numpy.frombuffer(base64.b64decode(told[0]["companions"]["_s"]), dtype=numpy.uint8).reshape(8, 8, 4)[2, 3, 3], 50
        )

    def test_what_is_told_once_is_not_told_again(self):
        self.connect()
        self.paint(smoothness=51)
        self.assertEqual(len(self.tiles_told()), 1)
        self.assertEqual(self.tiles_told(), [])

    def test_the_channels_the_tile_has_when_the_program_asks_count_as_told(self):
        self.paint(smoothness=51)
        outgoing = self.connect()
        self.assertEqual([message["type"] for message in outgoing], ["welcome", "known"])
        self.assertEqual(self.tiles_told(), [])

    def test_a_stroke_that_is_not_finished_is_told_with_its_channels(self):
        self.connect()
        self.editor.controls.set_mode("smoothness", "replace")
        self.editor.controls.set_value("smoothness", "primary", 0.2)
        self.controller.receive({"type": "pointer", "tile": "first", "x": 2, "y": 2, "left": True, "right": False, "segment": 0})
        told = self.tiles_told()
        self.assertEqual(len(told), 1)
        picture = numpy.frombuffer(base64.b64decode(told[0]["companions"]["_s"]), dtype=numpy.uint8).reshape(8, 8, 4)
        self.assertEqual(int(picture[2, 2, 0]), 51)


class NoCompanionsTests(BridgeChannelsFixture):
    def test_a_program_that_did_not_say_which_convention_it_reads_gets_none(self):
        self.connect(profile=None)
        self.paint(smoothness=51)
        self.assertEqual(self.tiles_told(), [], "a channel alone is nothing to tell a program that gets no companions")
        self.paint_pixel()
        told = self.tiles_told()
        self.assertEqual(len(told), 1)
        self.assertNotIn("companions", told[0])

    def test_a_program_that_reads_another_convention_gets_none(self):
        self.connect(profile="some_other_pbr")
        self.paint(smoothness=51)
        self.assertEqual(self.tiles_told(), [])
        self.paint_pixel()
        self.assertNotIn("companions", self.tiles_told()[0])

    def test_under_none_there_are_no_companions_whatever_the_program_says(self):
        self.project.switch_pbr_profile("none")
        self.editor.sync()
        self.connect(profile=LABPBR)
        self.project.tiles[(0, 0)].pixels[0, 0] = (1, 2, 3, 4)
        self.project.tiles[(0, 0)].revision += 1
        told = self.tiles_told()
        self.assertEqual(len(told), 1)
        self.assertNotIn("companions", told[0])


class TileRecordedFormatTests(BridgeChannelsFixture):
    def test_the_bytes_of_a_companion_are_the_bytes_of_the_file_that_a_save_writes(self):
        self.connect()
        self.paint(smoothness=51, emission=50)
        companions = self.tiles_told()[0]["companions"]
        self.project.save_tile((0, 0))
        saved = va.read_image_pixels(self.folder / "a_s.png")
        self.assertEqual(base64.b64decode(companions["_s"]), saved.tobytes())


if __name__ == "__main__":
    unittest.main()
