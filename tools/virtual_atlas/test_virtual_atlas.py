"""Tests for the logic of VirtualAtlas.py (everything that needs no window).

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import base64
import importlib.util
import json
import logging
import os
import random
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import numpy
from PIL import Image

# VIRTUAL_ATLAS_PROGRAM points the tests at a copy of the program, for experiments that break the code on purpose.
# The window tests draw into memory only.
os.environ.setdefault("SDL_VIDEODRIVER", "dummy")
os.environ.setdefault("SDL_AUDIODRIVER", "dummy")

PROGRAM_PATH = Path(
    os.environ.get("VIRTUAL_ATLAS_PROGRAM")
    or Path(__file__).resolve().parents[2] / "plugins" / "base_blocks" / "textures" / "VirtualAtlas.py"
)


def load_program():
    # Importing writes no __pycache__ next to the program, which lives in an asset folder.
    sys.dont_write_bytecode = True
    specification = importlib.util.spec_from_file_location("virtual_atlas_under_test", PROGRAM_PATH)
    module = importlib.util.module_from_spec(specification)
    sys.modules[specification.name] = module
    specification.loader.exec_module(module)
    return module


va = load_program()
# Without a handler, error messages about error tiles would be printed to the screen during the tests.
va.logger.addHandler(logging.NullHandler())

TRANSPARENT = va.TRANSPARENT
RED = (255, 0, 0, 255)
GREEN = (0, 255, 0, 255)
BLUE = (0, 0, 255, 255)


def solid_tile(color, size=4):
    pixels = numpy.zeros((size, size, 4), dtype=numpy.uint8)
    pixels[...] = color
    return pixels


def random_tile(seed, size=4):
    generator = numpy.random.default_rng(seed)
    return generator.integers(0, 256, size=(size, size, 4), dtype=numpy.uint8)


def mask_from_text(*rows):
    """A boolean mask from rows of text where '#' is set, for readable expectations."""
    return numpy.array([[character == "#" for character in row] for row in rows], dtype=bool)


def mask_to_text(mask):
    return ["".join("#" if value else "." for value in row) for row in mask]


class NameTests(unittest.TestCase):
    def test_accepts_ordinary_names(self):
        for name in ["dirt.png", "Stone_2.PNG", "a b.png", "tile.0.png", "ünï.png"]:
            self.assertIsNone(va.validate_tile_name(name), name)

    def test_rejects_bad_names(self):
        for name in ["", " dirt.png", "dirt.png ", "dirt", "dirt.jpg", ".png", "a/b.png", "a\\b.png", "../x.png",
                     "a:b.png", "a?.png", "a\x01.png", "CON.png", "con.png", "lpt1.png", "x" * 252 + ".png"]:
            self.assertIsNotNone(va.validate_tile_name(name), repr(name))

    def test_name_clash_ignores_case(self):
        self.assertIsNotNone(va.find_name_problem("Dirt.png", ["dirt.png"]))
        self.assertIsNone(va.find_name_problem("dirt2.png", ["dirt.png"]))

    def test_unique_names_get_a_counter_before_the_extension(self):
        self.assertEqual(va.make_unique_name("a.png", []), "a.png")
        self.assertEqual(va.make_unique_name("a.png", ["a.png"]), "a_2.png")
        self.assertEqual(va.make_unique_name("a.png", ["A.PNG", "a_2.png"]), "a_3.png")


class CoordinateTests(unittest.TestCase):
    def test_round_trip_and_spacing(self):
        self.assertEqual(va.format_coordinate((3, 12)), "(3, 12)")
        self.assertEqual(va.parse_coordinate("(3, 12)"), (3, 12))
        self.assertEqual(va.parse_coordinate("(3,12)"), (3, 12))
        self.assertEqual(va.parse_coordinate("( 3 ,  12 )"), (3, 12))

    def test_column_comes_first(self):
        self.assertEqual(va.parse_coordinate("(5, 1)"), (5, 1))

    def test_rejects_bad_text_and_negative_numbers(self):
        for text in ["3, 12", "(3, 12", "(3; 12)", "(a, b)", "(1, 2, 3)", "(-1, 0)", "(0, -1)", ""]:
            with self.assertRaises(ValueError, msg=text):
                va.parse_coordinate(text)

    def test_reading_order_is_row_then_column(self):
        coordinates = [(1, 1), (0, 1), (2, 0), (0, 0)]
        self.assertEqual(sorted(coordinates, key=va.reading_order_key), [(0, 0), (2, 0), (0, 1), (1, 1)])


class AtlasFileTests(unittest.TestCase):
    def test_parses_the_example_from_the_specification(self):
        atlas_file = va.parse_atlas_file(
            '{"coordinates_to_names": {"(0, 0)": "dirt.png", "(0, 1)": "stone.png"}}'
        )
        self.assertIsNone(atlas_file.tile_size)
        self.assertEqual(atlas_file.coordinates_to_names, {(0, 0): "dirt.png", (0, 1): "stone.png"})
        self.assertEqual(atlas_file.coordinates_to_data, {})

    def test_empty_object_is_an_empty_atlas(self):
        atlas_file = va.parse_atlas_file("{}")
        self.assertEqual((atlas_file.tile_size, atlas_file.coordinates_to_names), (None, {}))

    def assert_refused(self, text, expected_fragment):
        with self.assertRaises(va.AtlasFileError) as context:
            va.parse_atlas_file(text)
        self.assertTrue(
            any(expected_fragment in problem for problem in context.exception.problems),
            f"{expected_fragment!r} not in {context.exception.problems}",
        )

    def test_refuses_one_file_at_two_positions(self):
        self.assert_refused(
            '{"coordinates_to_names": {"(0, 0)": "a.png", "(1, 0)": "a.png"}}', "'a.png' appears at both"
        )

    def test_refuses_one_file_at_two_positions_ignoring_case(self):
        self.assert_refused('{"coordinates_to_names": {"(0, 0)": "a.png", "(1, 0)": "A.PNG"}}', "appears at both")

    def test_refuses_two_files_at_one_position_written_two_ways(self):
        self.assert_refused(
            '{"coordinates_to_names": {"(0, 0)": "a.png", "(0,0)": "b.png"}}', "appears twice"
        )

    def test_refuses_two_files_at_one_position_written_the_same_way(self):
        self.assert_refused('{"coordinates_to_names": {"(0, 0)": "a.png", "(0, 0)": "b.png"}}', "appears twice")

    def test_refuses_negative_and_malformed_coordinates(self):
        self.assert_refused('{"coordinates_to_names": {"(-1, 0)": "a.png"}}', "negative")
        self.assert_refused('{"coordinates_to_names": {"zero": "a.png"}}', "not a coordinate")

    def test_refuses_names_that_could_escape_the_folder(self):
        self.assert_refused('{"coordinates_to_names": {"(0, 0)": "../a.png"}}', "bad name")

    def test_refuses_data_without_a_name(self):
        self.assert_refused('{"coordinates_to_data": {"(2, 2)": "AAAA"}}', "has data but no name")

    def test_refuses_bad_tile_sizes(self):
        for bad_size in ["0", "2000", "true", '"16"', "16.5", "-4"]:
            self.assert_refused(f'{{"tile_size": {bad_size}}}', "tile_size")

    def test_refuses_what_is_not_json_or_not_an_object(self):
        self.assert_refused("{", "not valid JSON")
        self.assert_refused("[]", "top level")

    def test_reports_every_problem_not_just_the_first(self):
        with self.assertRaises(va.AtlasFileError) as context:
            va.parse_atlas_file('{"tile_size": 0, "coordinates_to_names": {"(0, 0)": "a.png", "(1, 0)": "a.png"}}')
        self.assertGreaterEqual(len(context.exception.problems), 2)

    def test_serialization_is_exact_and_in_reading_order(self):
        atlas_file = va.AtlasFile(
            16, {(1, 0): "b.png", (0, 1): "c.png", (0, 0): "a.png"}, {(0, 1): "AAAA"}, {"future_setting": [1, 2]}
        )
        expected = (
            '{\n    "tile_size": 16,\n    "coordinates_to_names": {\n        "(0, 0)": "a.png",\n'
            '        "(1, 0)": "b.png",\n        "(0, 1)": "c.png"\n    },\n'
            '    "coordinates_to_data": {\n        "(0, 1)": "AAAA"\n    },\n'
            '    "future_setting": [\n        1,\n        2\n    ]\n}\n'
        )
        self.assertEqual(va.serialize_atlas_file(atlas_file), expected)

    def test_round_trip_keeps_unknown_settings_and_everything_else(self):
        atlas_file = va.AtlasFile(8, {(2, 3): "x.png"}, {(2, 3): "AAAA"}, {"other": {"nested": True}, "n": 5})
        again = va.parse_atlas_file(va.serialize_atlas_file(atlas_file))
        self.assertEqual(again, atlas_file)

    def test_a_file_without_tile_size_is_written_without_one(self):
        text = va.serialize_atlas_file(va.AtlasFile(None, {(0, 0): "a.png"}))
        self.assertNotIn("tile_size", text)

    def test_unicode_names_survive(self):
        atlas_file = va.AtlasFile(4, {(0, 0): "ünï.png"})
        self.assertEqual(va.parse_atlas_file(va.serialize_atlas_file(atlas_file)), atlas_file)


class AtomicWriteTests(unittest.TestCase):
    def test_writes_content_and_leaves_no_temporary_file(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "atlas.json"
            va.write_atlas_file(path, va.AtlasFile(4, {(0, 0): "a.png"}))
            self.assertEqual(va.read_atlas_file(path).coordinates_to_names, {(0, 0): "a.png"})
            self.assertEqual(os.listdir(folder), ["atlas.json"])

    def test_a_failed_write_keeps_the_old_file_and_cleans_up(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "atlas.json"
            va.write_atlas_file(path, va.AtlasFile(4, {(0, 0): "old.png"}))
            with mock.patch.object(va.os, "replace", side_effect=OSError("disk on fire")):
                with self.assertRaises(OSError):
                    va.write_atlas_file(path, va.AtlasFile(4, {(0, 0): "new.png"}))
            self.assertEqual(va.read_atlas_file(path).coordinates_to_names, {(0, 0): "old.png"})
            self.assertEqual(os.listdir(folder), ["atlas.json"])

    def test_missing_file_reads_as_none(self):
        with tempfile.TemporaryDirectory() as folder:
            self.assertIsNone(va.read_atlas_file(Path(folder) / "atlas.json"))

    def test_unusable_file_is_refused(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "atlas.json"
            path.write_text("{ nope", encoding="utf-8")
            with self.assertRaises(va.AtlasFileError):
                va.read_atlas_file(path)

    def test_file_with_a_byte_order_mark_is_read(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "atlas.json"
            path.write_bytes(b"\xef\xbb\xbf" + b'{"tile_size": 8}')
            self.assertEqual(va.read_atlas_file(path).tile_size, 8)


class TileDataTests(unittest.TestCase):
    def test_round_trip(self):
        pixels = random_tile(1)
        self.assertTrue(numpy.array_equal(va.decode_tile_data(va.encode_tile_data(pixels), 4), pixels))

    def test_bytes_are_rows_then_columns_then_channels(self):
        pixels = numpy.zeros((2, 2, 4), dtype=numpy.uint8)
        pixels[1, 0] = (1, 2, 3, 4)
        raw = base64.b64decode(va.encode_tile_data(pixels))
        self.assertEqual(list(raw[8:12]), [1, 2, 3, 4])

    def test_decoded_data_can_be_edited(self):
        decoded = va.decode_tile_data(va.encode_tile_data(random_tile(2)), 4)
        decoded[0, 0] = 7

    def test_rejects_wrong_length_and_bad_base64(self):
        with self.assertRaises(va.TileDataError):
            va.decode_tile_data(va.encode_tile_data(random_tile(3, 4)), 8)
        with self.assertRaises(va.TileDataError):
            va.decode_tile_data("not base64!!", 4)


class PngTests(unittest.TestCase):
    def test_round_trip_keeps_color_under_zero_alpha(self):
        pixels = random_tile(4)
        pixels[0, 0] = (12, 34, 56, 0)
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "t.png"
            va.write_png_atomically(path, pixels)
            self.assertTrue(numpy.array_equal(va.read_image_pixels(path), pixels))
            self.assertEqual(va.read_image_size(path), (4, 4))

    def test_other_png_kinds_become_rgba(self):
        with tempfile.TemporaryDirectory() as folder:
            Image.new("RGB", (3, 3), (10, 20, 30)).save(Path(folder) / "rgb.png")
            Image.new("L", (3, 3), 77).save(Path(folder) / "gray.png")
            palette = Image.new("P", (3, 3), 0)
            palette.putpalette([5, 6, 7] + [0] * 765)
            palette.save(Path(folder) / "palette.png")
            self.assertEqual(tuple(va.read_image_pixels(Path(folder) / "rgb.png")[1, 1]), (10, 20, 30, 255))
            self.assertEqual(tuple(va.read_image_pixels(Path(folder) / "gray.png")[1, 1]), (77, 77, 77, 255))
            self.assertEqual(tuple(va.read_image_pixels(Path(folder) / "palette.png")[1, 1]), (5, 6, 7, 255))

    def test_a_corrupt_file_is_an_error_not_a_crash(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "bad.png"
            path.write_bytes(b"this is not a png")
            with self.assertRaises(va.TileImageError):
                va.read_image_size(path)
            with self.assertRaises(va.TileImageError):
                va.read_image_pixels(path)

    def test_a_truncated_file_is_an_error(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "t.png"
            va.write_png_atomically(path, random_tile(5, 16))
            path.write_bytes(path.read_bytes()[: len(path.read_bytes()) // 2])
            with self.assertRaises(va.TileImageError):
                va.read_image_pixels(path)


class FolderScanTests(unittest.TestCase):
    def setUp(self):
        self.temporary_folder = tempfile.TemporaryDirectory()
        self.folder = Path(self.temporary_folder.name)

    def tearDown(self):
        self.temporary_folder.cleanup()

    def put_tile(self, name, pixels):
        va.write_png_atomically(self.folder / name, pixels)

    def scan(self, atlas_file, tile_size=4):
        return va.scan_tile_folder(self.folder, atlas_file, tile_size)

    def test_listed_good_file_is_a_clean_tile(self):
        self.put_tile("a.png", solid_tile(RED))
        scan = self.scan(va.AtlasFile(4, {(0, 0): "a.png"}))
        record = scan.tiles[(0, 0)]
        self.assertIsNone(record.error)
        self.assertFalse(record.is_modified)
        self.assertFalse(record.is_new)
        self.assertTrue(record.can_return_to_unplaced)
        self.assertEqual(scan.unplaced, [])

    def test_listed_missing_file_is_an_error_tile(self):
        scan = self.scan(va.AtlasFile(4, {(1, 2): "gone.png"}))
        self.assertIn("missing", scan.tiles[(1, 2)].error)
        self.assertIsNone(scan.tiles[(1, 2)].pixels)

    def test_listed_wrong_size_file_is_an_error_tile_and_is_logged(self):
        self.put_tile("big.png", solid_tile(RED, 8))
        with self.assertLogs("VirtualAtlas", level="ERROR") as logs:
            scan = va.scan_tile_folder(self.folder, va.AtlasFile(4, {(0, 0): "big.png"}), 4)
        self.assertIn("8x8", scan.tiles[(0, 0)].error)
        self.assertTrue(any("big.png" in line for line in logs.output))
        self.assertFalse(scan.tiles[(0, 0)].is_modified)
        self.assertFalse(scan.tiles[(0, 0)].can_return_to_unplaced)

    def test_listed_corrupt_file_is_an_error_tile(self):
        (self.folder / "bad.png").write_bytes(b"junk")
        scan = self.scan(va.AtlasFile(4, {(0, 0): "bad.png"}))
        self.assertIn("cannot be read", scan.tiles[(0, 0)].error)

    def test_unlisted_files_of_the_right_size_are_unplaced_and_sorted(self):
        self.put_tile("b.png", solid_tile(RED))
        self.put_tile("A.png", solid_tile(GREEN))
        scan = va.scan_tile_folder(self.folder, va.AtlasFile(4), 4)
        self.assertEqual([tile.name for tile in scan.unplaced], ["A.png", "b.png"])

    def test_unlisted_wrong_size_corrupt_and_non_png_files_are_ignored_silently(self):
        self.put_tile("big.png", solid_tile(RED, 8))
        (self.folder / "bad.png").write_bytes(b"junk")
        (self.folder / "notes.txt").write_text("x")
        (self.folder / ".virtualatlas-123.tmp").write_bytes(b"half a write")
        with self.assertNoLogs("VirtualAtlas", level="ERROR"):
            scan = va.scan_tile_folder(self.folder, va.AtlasFile(4), 4)
        self.assertEqual(scan.unplaced, [])
        self.assertEqual(scan.existing_names, {"big.png", "bad.png"})

    def test_png_extension_is_matched_ignoring_case(self):
        self.put_tile("Shout.PNG", solid_tile(RED))
        scan = va.scan_tile_folder(self.folder, va.AtlasFile(4), 4)
        self.assertEqual([tile.name for tile in scan.unplaced], ["Shout.PNG"])

    def test_listed_name_finds_a_file_that_differs_in_case(self):
        self.put_tile("dirt.png", solid_tile(RED))
        scan = self.scan(va.AtlasFile(4, {(0, 0): "Dirt.png"}))
        self.assertIsNone(scan.tiles[(0, 0)].error)
        self.assertEqual(scan.unplaced, [])

    def test_data_without_a_file_is_a_new_modified_tile(self):
        pixels = random_tile(6)
        scan = self.scan(va.AtlasFile(4, {(0, 0): "n.png"}, {(0, 0): va.encode_tile_data(pixels)}))
        record = scan.tiles[(0, 0)]
        self.assertTrue(record.is_new)
        self.assertTrue(record.is_modified)
        self.assertFalse(record.can_return_to_unplaced)
        self.assertTrue(numpy.array_equal(record.pixels, pixels))
        self.assertIsNone(record.disk_pixels)

    def test_data_equal_to_the_file_is_stale(self):
        pixels = random_tile(7)
        self.put_tile("a.png", pixels)
        scan = self.scan(va.AtlasFile(4, {(0, 0): "a.png"}, {(0, 0): va.encode_tile_data(pixels)}))
        self.assertEqual(scan.stale_data_coordinates, [(0, 0)])
        self.assertFalse(scan.tiles[(0, 0)].is_modified)

    def test_data_different_from_the_file_is_a_modified_tile(self):
        on_disk, current = random_tile(8), random_tile(9)
        self.put_tile("a.png", on_disk)
        scan = self.scan(va.AtlasFile(4, {(0, 0): "a.png"}, {(0, 0): va.encode_tile_data(current)}))
        record = scan.tiles[(0, 0)]
        self.assertTrue(record.is_modified)
        self.assertFalse(record.is_new)
        self.assertTrue(numpy.array_equal(record.pixels, current))
        self.assertTrue(numpy.array_equal(record.disk_pixels, on_disk))
        self.assertEqual(scan.stale_data_coordinates, [])

    def test_data_of_the_wrong_length_is_an_error_tile(self):
        scan = self.scan(va.AtlasFile(4, {(0, 0): "n.png"}, {(0, 0): va.encode_tile_data(random_tile(1, 8))}))
        self.assertIn("stored pixel data", scan.tiles[(0, 0)].error)

    def test_data_next_to_a_wrong_size_file_is_an_error_tile_so_the_file_is_not_overwritten(self):
        self.put_tile("a.png", solid_tile(RED, 8))
        scan = self.scan(va.AtlasFile(4, {(0, 0): "a.png"}, {(0, 0): va.encode_tile_data(random_tile(1))}))
        self.assertIn("8x8", scan.tiles[(0, 0)].error)

    def test_existing_names_include_ignored_files_and_listed_missing_ones_do_not_count(self):
        self.put_tile("big.png", solid_tile(RED, 8))
        scan = self.scan(va.AtlasFile(4, {(0, 0): "gone.png"}))
        self.assertEqual(scan.existing_names, {"big.png"})


class MovePlanTests(unittest.TestCase):
    def test_one_tile_onto_another_is_a_swap(self):
        moves = va.plan_group_move({(0, 0), (1, 0)}, {(0, 0)}, (0, 0), (1, 0))
        self.assertEqual(moves, {(0, 0): (1, 0), (1, 0): (0, 0)})

    def test_one_tile_onto_an_empty_place_just_moves(self):
        self.assertEqual(va.plan_group_move({(0, 0)}, {(0, 0)}, (0, 0), (3, 2)), {(0, 0): (3, 2)})

    def test_dropping_where_it_started_changes_nothing(self):
        self.assertEqual(va.plan_group_move({(0, 0), (1, 0)}, {(0, 0)}, (0, 0), (0, 0)), {})

    def test_a_row_dragged_one_step_over_a_neighbor_rotates_it_to_the_other_end(self):
        occupied = {(0, 0), (1, 0), (2, 0), (3, 0)}
        moves = va.plan_group_move(occupied, {(0, 0), (1, 0), (2, 0)}, (0, 0), (1, 0))
        self.assertEqual(moves, {(0, 0): (1, 0), (1, 0): (2, 0), (2, 0): (3, 0), (3, 0): (0, 0)})

    def test_the_grabbed_tile_decides_the_offset(self):
        occupied = {(0, 0), (1, 0), (5, 5)}
        moves = va.plan_group_move(occupied, {(0, 0), (1, 0)}, (1, 0), (1, 3))
        self.assertEqual(moves, {(0, 0): (0, 3), (1, 0): (1, 3)})

    def test_displaced_tiles_go_to_the_vacated_cells_in_reading_order(self):
        # Selected (0,0) and (1,0) move down by 1 onto A at (0,1) and B at (1,1); A and B go to the freed cells.
        occupied = {(0, 0), (1, 0), (0, 1), (1, 1)}
        moves = va.plan_group_move(occupied, {(0, 0), (1, 0)}, (0, 0), (0, 1))
        self.assertEqual(moves, {(0, 0): (0, 1), (1, 0): (1, 1), (0, 1): (0, 0), (1, 1): (1, 0)})

    def test_displaced_tiles_fill_only_cells_that_the_selection_does_not_land_on(self):
        # Selection {(0,0),(2,0)} moves right by 1 to {(1,0),(3,0)}; X at (1,0) is displaced; the free cells
        # are (0,0) and (2,0), and X takes the first one in reading order.
        occupied = {(0, 0), (1, 0), (2, 0)}
        moves = va.plan_group_move(occupied, {(0, 0), (2, 0)}, (0, 0), (1, 0))
        self.assertEqual(moves, {(0, 0): (1, 0), (2, 0): (3, 0), (1, 0): (0, 0)})

    def test_a_negative_destination_refuses_the_whole_move(self):
        occupied = {(0, 0), (1, 0), (2, 0)}
        self.assertIsNone(va.plan_group_move(occupied, {(0, 0), (1, 0)}, (1, 0), (0, 0)))
        self.assertIsNone(va.plan_group_move({(0, 0)}, {(0, 0)}, (0, 0), (-1, 0)))

    def test_misuse_is_an_error(self):
        with self.assertRaises(ValueError):
            va.plan_group_move({(0, 0), (1, 0)}, {(0, 0)}, (1, 0), (2, 0))
        with self.assertRaises(ValueError):
            va.plan_group_move({(0, 0)}, {(0, 0), (1, 0)}, (0, 0), (2, 0))

    def test_any_move_is_a_permutation_that_shifts_the_selection_rigidly(self):
        generator = random.Random(1234)
        refused = 0
        for _ in range(3000):
            cells = [(column, row) for column in range(5) for row in range(5)]
            occupied = set(generator.sample(cells, generator.randint(1, 18)))
            selected = set(generator.sample(sorted(occupied), generator.randint(1, len(occupied))))
            grabbed = generator.choice(sorted(selected))
            drop = (generator.randint(0, 7), generator.randint(0, 7))
            moves = va.plan_group_move(occupied, selected, grabbed, drop)
            if moves is None:
                refused += 1
                self.assertTrue(
                    any(column + drop[0] - grabbed[0] < 0 or row + drop[1] - grabbed[1] < 0 for column, row in selected)
                )
                continue
            offset = (drop[0] - grabbed[0], drop[1] - grabbed[1])
            self.assertTrue(set(moves) <= occupied)
            after = {moves.get(cell, cell) for cell in occupied}
            self.assertEqual(len(after), len(occupied), "two tiles ended on the same cell or one vanished")
            if offset != (0, 0):
                for column, row in selected:
                    self.assertEqual(moves[(column, row)], (column + offset[0], row + offset[1]))
            if offset != (0, 0):
                for cell in occupied - set(moves):
                    self.assertNotIn(cell, selected)
            displaced = set(moves) - selected
            leftovers = {moves[cell] for cell in displaced}
            self.assertTrue(leftovers <= selected, "a displaced tile must land where the selection used to be")
        self.assertGreater(refused, 0)

    def test_move_keys_applies_moves_to_any_dictionary(self):
        moved = va.move_keys({(0, 0): "a", (1, 0): "b", (2, 2): "c"}, {(0, 0): (1, 0), (1, 0): (0, 0)})
        self.assertEqual(moved, {(1, 0): "a", (0, 0): "b", (2, 2): "c"})

    def test_placing_an_unplaced_tile_needs_an_empty_non_negative_place(self):
        self.assertTrue(va.can_place_unplaced_tile({(0, 0)}, (1, 0)))
        self.assertFalse(va.can_place_unplaced_tile({(0, 0)}, (0, 0)))
        self.assertFalse(va.can_place_unplaced_tile({(0, 0)}, (-1, 0)))

    def test_atlas_extent_has_one_spare_column_and_row(self):
        self.assertEqual(va.atlas_extent([]), (1, 1))
        self.assertEqual(va.atlas_extent([(0, 0)]), (2, 2))
        self.assertEqual(va.atlas_extent([(3, 0), (1, 4)]), (5, 6))


class ImportTests(unittest.TestCase):
    def test_grid_description(self):
        grid = va.describe_import_grid(37, 20, 16)
        self.assertEqual((grid.full_columns, grid.full_rows, grid.remainder_width, grid.remainder_height), (2, 1, 5, 4))
        self.assertFalse(grid.divides_evenly)
        self.assertTrue(va.describe_import_grid(32, 16, 16).divides_evenly)

    def picture(self, width, height):
        return numpy.random.default_rng(width * 1000 + height).integers(0, 256, size=(height, width, 4), dtype=numpy.uint8)

    def test_even_picture_is_cut_exactly(self):
        picture = self.picture(8, 4)
        tiles = va.slice_image_into_tiles(picture, 4, "crop")
        self.assertEqual(sorted(tiles), [(0, 0), (1, 0)])
        self.assertTrue(numpy.array_equal(tiles[(1, 0)], picture[0:4, 4:8]))
        self.assertTrue(numpy.array_equal(va.slice_image_into_tiles(picture, 4, "pad")[(1, 0)], picture[0:4, 4:8]))

    def test_crop_drops_partial_tiles(self):
        tiles = va.slice_image_into_tiles(self.picture(10, 9), 4, "crop")
        self.assertEqual(sorted(tiles), [(0, 0), (0, 1), (1, 0), (1, 1)])

    def test_pad_keeps_partial_tiles_and_fills_with_transparency(self):
        picture = self.picture(10, 9)
        tiles = va.slice_image_into_tiles(picture, 4, "pad")
        self.assertEqual(len(tiles), 9)
        corner = tiles[(2, 2)]
        self.assertTrue(numpy.array_equal(corner[:1, :2], picture[8:9, 8:10]))
        self.assertTrue(numpy.all(corner[1:, :] == 0))
        self.assertTrue(numpy.all(corner[:, 2:] == 0))

    def test_a_picture_smaller_than_a_tile_cannot_be_cropped_but_can_be_padded(self):
        with self.assertRaises(ValueError):
            va.slice_image_into_tiles(self.picture(3, 3), 4, "crop")
        self.assertEqual(sorted(va.slice_image_into_tiles(self.picture(3, 3), 4, "pad")), [(0, 0)])

    def test_unknown_mode_is_an_error(self):
        with self.assertRaises(ValueError):
            va.slice_image_into_tiles(self.picture(8, 8), 4, "stretch")

    def test_default_names_use_row_then_column_zero_padded(self):
        names = va.default_import_names([(0, 0), (2, 1)], [])
        self.assertEqual(names, {(0, 0): "tile_00_00.png", (2, 1): "tile_01_02.png"})

    def test_default_names_widen_for_large_atlases(self):
        names = va.default_import_names([(0, 0), (123, 4)], [])
        self.assertEqual(names[(123, 4)], "tile_004_123.png")

    def test_default_names_avoid_existing_files_ignoring_case(self):
        names = va.default_import_names([(0, 0), (1, 0)], ["TILE_00_00.PNG"])
        self.assertEqual(names, {(0, 0): "tile_00_00_2.png", (1, 0): "tile_00_01.png"})


class CanvasTests(unittest.TestCase):
    def test_tiles_keep_their_relative_arrangement_with_empty_cells_between(self):
        tiles = {(3, 5): solid_tile(RED), (5, 5): solid_tile(GREEN), (3, 6): solid_tile(BLUE)}
        layout = va.compose_canvas(tiles, 4)
        self.assertEqual(layout.pixels.shape, (8, 12, 4))
        self.assertEqual((layout.first_column, layout.first_row), (3, 5))
        self.assertEqual(tuple(layout.pixels[0, 0]), RED)
        self.assertEqual(tuple(layout.pixels[0, 8]), GREEN)
        self.assertEqual(tuple(layout.pixels[4, 0]), BLUE)
        self.assertTrue(layout.editable_mask[0, 0] and layout.editable_mask[0, 8] and layout.editable_mask[4, 3])
        self.assertFalse(layout.editable_mask[0, 4])
        self.assertFalse(layout.editable_mask[4, 4])

    def test_extracting_unedited_canvas_gives_the_tiles_back(self):
        tiles = {(0, 0): random_tile(1), (2, 1): random_tile(2)}
        layout = va.compose_canvas(tiles, 4)
        back = va.extract_canvas_tiles(layout, layout.pixels)
        self.assertEqual(set(back), set(tiles))
        for coordinate in tiles:
            self.assertTrue(numpy.array_equal(back[coordinate], tiles[coordinate]))

    def test_a_line_across_two_tiles_lands_in_both_and_nothing_lands_in_the_gap(self):
        layout = va.compose_canvas({(0, 0): solid_tile(TRANSPARENT), (2, 0): solid_tile(TRANSPARENT)}, 4)
        canvas = layout.pixels.copy()
        mask = va.thick_line_mask(canvas.shape[:2], (1, 1), (10, 1), 1) & layout.editable_mask
        va.apply_color_to_mask(canvas, mask, RED)
        back = va.extract_canvas_tiles(layout, canvas)
        self.assertEqual(int((back[(0, 0)][..., 3] > 0).sum()), 3)
        self.assertEqual(int((back[(2, 0)][..., 3] > 0).sum()), 3)

    def test_nothing_to_compose_is_an_error(self):
        with self.assertRaises(ValueError):
            va.compose_canvas({}, 4)


class BrushMaskTests(unittest.TestCase):
    def test_odd_brush_is_centered_and_even_brush_extends_right_and_down(self):
        self.assertEqual(mask_to_text(va.rectangle_brush_mask((5, 5), 2, 2, 3, 1)), [".....", ".....", ".###.", ".....", "....."])
        self.assertEqual(mask_to_text(va.rectangle_brush_mask((5, 5), 2, 2, 2, 2)), [".....", ".##..", ".##..", ".....", "....."])
        self.assertEqual(mask_to_text(va.rectangle_brush_mask((5, 5), 2, 2, 4, 1)), [".....", ".....", "####.", ".....", "....."])

    def test_width_and_height_are_independent(self):
        mask = va.rectangle_brush_mask((9, 9), 4, 4, 5, 2)
        self.assertEqual(int(mask.sum()), 10)
        self.assertEqual(mask_to_text(mask)[3], "..#####..")
        self.assertEqual(mask_to_text(mask)[4], "..#####..")

    def test_brush_is_clipped_at_the_edges_without_error(self):
        self.assertEqual(int(va.rectangle_brush_mask((4, 4), 0, 0, 3, 3).sum()), 4)
        self.assertEqual(int(va.rectangle_brush_mask((4, 4), 100, 100, 3, 3).sum()), 0)
        self.assertEqual(int(va.rectangle_brush_mask((4, 4), -100, 2, 3, 3).sum()), 0)

    def test_every_size_paints_exactly_that_many_pixels(self):
        for size in range(1, 9):
            self.assertEqual(int(va.rectangle_brush_mask((20, 20), 10, 10, size, size).sum()), size * size)


class LinePixelTests(unittest.TestCase):
    def test_includes_both_ends_and_is_connected(self):
        generator = random.Random(7)
        for _ in range(300):
            start = (generator.randint(-5, 15), generator.randint(-5, 15))
            end = (generator.randint(-5, 15), generator.randint(-5, 15))
            points = va.line_pixels(start, end)
            self.assertEqual(points[0], start)
            self.assertEqual(points[-1], end)
            self.assertEqual(len(points), max(abs(end[0] - start[0]), abs(end[1] - start[1])) + 1)
            for (a, b), (c, d) in zip(points, points[1:]):
                self.assertLessEqual(max(abs(a - c), abs(b - d)), 1)

    def test_single_point(self):
        self.assertEqual(va.line_pixels((3, 4), (3, 4)), [(3, 4)])

    def test_swept_brush_equals_union_of_stamps_and_covers_the_line(self):
        mask = va.swept_rectangle_brush_mask((10, 10), (1, 1), (7, 4), 2, 3)
        union = numpy.zeros((10, 10), dtype=bool)
        for column, row in va.line_pixels((1, 1), (7, 4)):
            union |= va.rectangle_brush_mask((10, 10), column, row, 2, 3)
        self.assertTrue(numpy.array_equal(mask, union))


class ThickLineTests(unittest.TestCase):
    def test_width_one_is_the_bresenham_line(self):
        mask = va.thick_line_mask((10, 10), (1, 1), (7, 4), 1)
        expected = numpy.zeros((10, 10), dtype=bool)
        for column, row in va.line_pixels((1, 1), (7, 4)):
            expected[row, column] = True
        self.assertTrue(numpy.array_equal(mask, expected))

    def test_horizontal_line_of_every_width_paints_that_many_rows_with_square_ends(self):
        for width in range(1, 8):
            mask = va.thick_line_mask((20, 20), (3, 10), (12, 10), width)
            rows = numpy.flatnonzero(mask.any(axis=1))
            columns = numpy.flatnonzero(mask.any(axis=0))
            self.assertEqual(len(rows), width)
            self.assertEqual(rows[0], 10 - width // 2)
            self.assertEqual((columns[0], columns[-1]), (3, 12), f"ends of width {width}")
            self.assertEqual(int(mask.sum()), width * 10)

    def test_vertical_line_of_every_width_paints_that_many_columns(self):
        for width in range(1, 8):
            mask = va.thick_line_mask((20, 20), (10, 3), (10, 12), width)
            columns = numpy.flatnonzero(mask.any(axis=0))
            rows = numpy.flatnonzero(mask.any(axis=1))
            self.assertEqual(len(columns), width)
            self.assertEqual(columns[0], 10 - width // 2)
            self.assertEqual((rows[0], rows[-1]), (3, 12))

    def test_horizontal_width_matches_the_pencil_rule(self):
        for width in range(1, 8):
            line = va.thick_line_mask((20, 20), (5, 10), (9, 10), width)
            pencil = va.swept_rectangle_brush_mask((20, 20), (5, 10), (9, 10), 1, width)
            self.assertTrue(numpy.array_equal(line, pencil), f"width {width}")

    def test_a_zero_length_line_is_a_square_stamp(self):
        for width in (2, 3, 4):
            self.assertTrue(
                numpy.array_equal(
                    va.thick_line_mask((10, 10), (5, 5), (5, 5), width), va.rectangle_brush_mask((10, 10), 5, 5, width, width)
                )
            )

    def test_the_end_caps_are_square_to_the_line_not_to_the_axes(self):
        # A 45 degree line of width 4: the end pixel's far corners must not stick out past the cap.
        mask = va.thick_line_mask((30, 30), (10, 10), (20, 20), 4)
        self.assertFalse(mask[8, 8], "a pixel behind the start cap, on the line itself, is painted")
        self.assertFalse(mask[22, 22], "a pixel beyond the end cap is painted")
        self.assertTrue(mask[10, 10] and mask[20, 20])
        # A square brush swept along the line paints (column 8, row 11) behind the start cap; the rectangle does not.
        swept = va.swept_rectangle_brush_mask((30, 30), (10, 10), (20, 20), 4, 4)
        self.assertTrue(swept[11, 8])
        self.assertFalse(mask[11, 8])
        self.assertGreater(int(swept.sum()), int(mask.sum()))

    def test_drawing_the_line_backwards_gives_the_same_pixels(self):
        generator = random.Random(99)
        for _ in range(400):
            start = (generator.randint(0, 24), generator.randint(0, 24))
            end = (generator.randint(0, 24), generator.randint(0, 24))
            width = generator.randint(2, 7)
            forward = va.thick_line_mask((25, 25), start, end, width)
            backward = va.thick_line_mask((25, 25), end, start, width)
            self.assertTrue(numpy.array_equal(forward, backward), (start, end, width))

    def test_wide_lines_contain_the_thin_line_and_are_connected_in_size(self):
        generator = random.Random(5)
        for _ in range(300):
            start = (generator.randint(3, 40), generator.randint(3, 40))
            end = (generator.randint(3, 40), generator.randint(3, 40))
            width = generator.randint(2, 8)
            mask = va.thick_line_mask((45, 45), start, end, width)
            for column, row in va.line_pixels(start, end):
                self.assertTrue(mask[row, column], (start, end, width, (column, row)))
            length = ((end[0] - start[0]) ** 2 + (end[1] - start[1]) ** 2) ** 0.5
            self.assertLess(abs(int(mask.sum()) - width * (length + 1)), 2 * width + 2 + length * 0.4, (start, end, width))

    def test_lines_running_off_the_canvas_are_clipped(self):
        mask = va.thick_line_mask((6, 6), (-10, 2), (20, 2), 3)
        self.assertEqual(int(mask.sum()), 6 * 3)
        self.assertEqual(int(va.thick_line_mask((6, 6), (-10, -10), (-3, -3), 3).sum()), 0)


class ParallelogramTests(unittest.TestCase):
    def test_horizontal_ends_slanted_sides(self):
        mask = va.horizontal_parallelogram_mask((6, 8), (2, 0), (4, 2), 3)
        self.assertEqual(
            [list(numpy.flatnonzero(row)) for row in mask[:4]], [[1, 2, 3], [2, 3, 4], [3, 4, 5], []]
        )

    def test_even_width_follows_the_pencil_rule(self):
        mask = va.horizontal_parallelogram_mask((5, 8), (3, 0), (3, 2), 2)
        self.assertEqual([list(numpy.flatnonzero(row)) for row in mask[:3]], [[2, 3], [2, 3], [2, 3]])

    def test_a_perfectly_horizontal_drag_is_a_one_pixel_high_run_covering_both_ends(self):
        mask = va.horizontal_parallelogram_mask((3, 12), (2, 1), (6, 1), 2)
        self.assertEqual(list(numpy.flatnonzero(mask[1])), [1, 2, 3, 4, 5, 6])
        self.assertEqual(int(mask.sum()), 6)

    def test_drawing_it_backwards_gives_the_same_pixels(self):
        generator = random.Random(3)
        for _ in range(300):
            start = (generator.randint(0, 19), generator.randint(0, 19))
            end = (generator.randint(0, 19), generator.randint(0, 19))
            width = generator.randint(1, 6)
            forward = va.horizontal_parallelogram_mask((20, 20), start, end, width)
            backward = va.horizontal_parallelogram_mask((20, 20), end, start, width)
            self.assertTrue(numpy.array_equal(forward, backward), (start, end, width))

    def test_the_end_segments_are_exactly_width_pixels_wide(self):
        generator = random.Random(4)
        for _ in range(200):
            start = (generator.randint(5, 25), generator.randint(2, 10))
            end = (generator.randint(5, 25), generator.randint(12, 20))
            width = generator.randint(1, 7)
            mask = va.horizontal_parallelogram_mask((30, 30), start, end, width)
            self.assertEqual(int(mask[start[1]].sum()), width)
            self.assertEqual(int(mask[end[1]].sum()), width)
            self.assertEqual(int(mask[start[1] - 1].sum()), 0)

    def test_vertical_is_the_transpose(self):
        horizontal = va.horizontal_parallelogram_mask((8, 6), (0, 2), (2, 4), 3)
        vertical = va.vertical_parallelogram_mask((6, 8), (2, 0), (4, 2), 3)
        self.assertTrue(numpy.array_equal(vertical, horizontal.T))

    def test_vertical_ends_are_vertical(self):
        mask = va.vertical_parallelogram_mask((10, 10), (1, 2), (6, 4), 3)
        self.assertEqual(list(numpy.flatnonzero(mask[:, 1])), [1, 2, 3])
        self.assertEqual(list(numpy.flatnonzero(mask[:, 6])), [3, 4, 5])
        self.assertEqual(int(mask[:, 0].sum()), 0)

    def test_clipping_never_raises(self):
        for start, end in [((-5, -5), (3, 3)), ((2, 2), (40, 40)), ((-9, 3), (-2, 7))]:
            va.horizontal_parallelogram_mask((6, 6), start, end, 4)
            va.vertical_parallelogram_mask((6, 6), start, end, 4)


class FloodFillTests(unittest.TestCase):
    def test_fills_the_contiguous_exact_match_only(self):
        pixels = solid_tile(BLUE, 5)
        pixels[:, 2] = RED
        mask = va.flood_fill_mask(pixels, 0, 0)
        self.assertEqual(mask_to_text(mask)[0], "##...")
        self.assertEqual(int(mask.sum()), 10)

    def test_diagonal_neighbors_are_not_connected(self):
        pixels = solid_tile(BLUE, 3)
        pixels[0, 1] = RED
        pixels[1, 0] = RED
        self.assertEqual(int(va.flood_fill_mask(pixels, 0, 0).sum()), 1)

    def test_a_near_color_is_not_a_match(self):
        pixels = solid_tile(BLUE, 3)
        pixels[1, 1] = (0, 0, 254, 255)
        self.assertEqual(int(va.flood_fill_mask(pixels, 0, 0).sum()), 8)

    def test_transparent_pixels_with_different_hidden_colors_do_not_match(self):
        pixels = solid_tile((0, 0, 0, 0), 3)
        pixels[1, 1] = (9, 9, 9, 0)
        self.assertEqual(int(va.flood_fill_mask(pixels, 0, 0).sum()), 8)
        self.assertEqual(int(va.flood_fill_mask(pixels, 1, 1).sum()), 1)

    def test_alpha_counts(self):
        pixels = solid_tile(RED, 3)
        pixels[0, 0] = (255, 0, 0, 254)
        self.assertEqual(int(va.flood_fill_mask(pixels, 1, 1).sum()), 8)

    def test_a_barrier_of_non_editable_cells_stops_the_fill_and_blocks_a_start_on_it(self):
        pixels = solid_tile(BLUE, 5)
        editable = numpy.ones((5, 5), dtype=bool)
        editable[:, 2] = False
        self.assertEqual(int(va.flood_fill_mask(pixels, 0, 0, editable).sum()), 10)
        self.assertEqual(int(va.flood_fill_mask(pixels, 2, 2, editable).sum()), 0)

    def test_outside_the_canvas_fills_nothing(self):
        self.assertEqual(int(va.flood_fill_mask(solid_tile(BLUE, 3), 5, 5).sum()), 0)
        self.assertEqual(int(va.flood_fill_mask(solid_tile(BLUE, 3), -1, 0).sum()), 0)

    def test_whole_canvas_fill(self):
        self.assertEqual(int(va.flood_fill_mask(solid_tile(BLUE, 6), 3, 3).sum()), 36)


class ColorTests(unittest.TestCase):
    def blend(self, destination, source):
        return tuple(va.alpha_over(numpy.array([[destination]], dtype=numpy.uint8), numpy.array([[source]], dtype=numpy.uint8))[0, 0])

    def test_opaque_source_replaces(self):
        self.assertEqual(self.blend((1, 2, 3, 4), (9, 8, 7, 255)), (9, 8, 7, 255))

    def test_transparent_source_leaves_the_destination_exactly(self):
        self.assertEqual(self.blend((1, 2, 3, 4), (200, 200, 200, 0)), (1, 2, 3, 4))
        self.assertEqual(self.blend((1, 2, 3, 0), (200, 200, 200, 0)), (1, 2, 3, 0))

    def test_over_a_transparent_destination_gives_the_source_color_and_alpha(self):
        self.assertEqual(self.blend((77, 77, 77, 0), (10, 20, 30, 100)), (10, 20, 30, 100))

    def test_half_over_opaque_mixes(self):
        self.assertEqual(self.blend((0, 0, 0, 255), (255, 255, 255, 128)), (128, 128, 128, 255))

    def test_two_translucent_layers_follow_the_standard_formula(self):
        result = self.blend((0, 0, 255, 128), (255, 0, 0, 128))
        self.assertEqual(result[3], 192)
        self.assertAlmostEqual(result[0], 255 * 0.5 / 0.75, delta=1)
        self.assertAlmostEqual(result[2], 255 * 0.5 * 0.5 / 0.75, delta=1)

    def test_apply_color_writes_exactly_including_alpha(self):
        pixels = solid_tile(BLUE, 3)
        va.apply_color_to_mask(pixels, mask_from_text(".#.", "...", "..."), (1, 2, 3, 0))
        self.assertEqual(tuple(pixels[0, 1]), (1, 2, 3, 0))
        self.assertEqual(tuple(pixels[0, 0]), BLUE)

    def test_eraser_color_is_fully_transparent_black(self):
        self.assertEqual(va.TRANSPARENT, (0, 0, 0, 0))


class SelectionAndPasteTests(unittest.TestCase):
    def test_rectangle_between_corners_in_any_order_and_clipped(self):
        self.assertEqual(va.rectangle_from_corners((3, 1), (1, 2), (10, 10)), (1, 1, 4, 3))
        self.assertEqual(va.rectangle_from_corners((-3, -3), (1, 1), (10, 10)), (0, 0, 2, 2))
        self.assertEqual(va.rectangle_from_corners((8, 8), (20, 20), (10, 10)), (8, 8, 10, 10))
        left, top, right, bottom = va.rectangle_from_corners((20, 20), (30, 30), (10, 10))
        self.assertTrue(right <= left or bottom <= top)

    def test_crop_copies(self):
        pixels = random_tile(1, 6)
        cropped = va.crop_pixels(pixels, (1, 2, 4, 5))
        self.assertEqual(cropped.shape, (3, 3, 4))
        cropped[...] = 0
        self.assertFalse(numpy.all(pixels[2:5, 1:4] == 0))

    def test_clear_rectangle_makes_transparent_only_in_editable_cells(self):
        pixels = solid_tile(RED, 4)
        editable = numpy.ones((4, 4), dtype=bool)
        editable[:, 3] = False
        va.clear_rectangle(pixels, (1, 1, 4, 3), editable)
        self.assertEqual(tuple(pixels[1, 1]), va.TRANSPARENT)
        self.assertEqual(tuple(pixels[2, 2]), va.TRANSPARENT)
        self.assertEqual(tuple(pixels[1, 3]), RED)
        self.assertEqual(tuple(pixels[0, 1]), RED)

    def test_paste_composites_alpha_over_so_transparent_parts_keep_what_is_underneath(self):
        destination = solid_tile(RED, 4)
        source = numpy.zeros((2, 2, 4), dtype=numpy.uint8)
        source[0, 0] = BLUE
        result = va.paste_pixels(destination, source, 1, 1)
        self.assertEqual(tuple(result[1, 1]), BLUE)
        self.assertEqual(tuple(result[1, 2]), RED)
        self.assertEqual(tuple(destination[1, 1]), RED, "the destination must not be modified")

    def test_paste_is_clipped_to_the_destination_and_to_editable_cells(self):
        destination = solid_tile(RED, 4)
        source = solid_tile(BLUE, 3)
        result = va.paste_pixels(destination, source, 2, 2)
        self.assertEqual({tuple(pixel) for pixel in result.reshape(-1, 4)}, {RED, BLUE})
        self.assertEqual(int(numpy.all(result == BLUE, axis=2).sum()), 4)
        editable = numpy.ones((4, 4), dtype=bool)
        editable[:, 3] = False
        self.assertEqual(int(numpy.all(va.paste_pixels(destination, source, 2, 2, editable) == BLUE, axis=2).sum()), 2)

    def test_paste_entirely_outside_changes_nothing(self):
        destination = solid_tile(RED, 4)
        self.assertTrue(numpy.array_equal(va.paste_pixels(destination, solid_tile(BLUE, 2), 10, 10), destination))
        self.assertTrue(numpy.array_equal(va.paste_pixels(destination, solid_tile(BLUE, 2), -5, -5), destination))

    def test_paste_partly_off_the_top_left_uses_the_visible_part_of_the_source(self):
        source = numpy.zeros((3, 3, 4), dtype=numpy.uint8)
        source[2, 2] = BLUE
        result = va.paste_pixels(solid_tile(RED, 4), source, -2, -2)
        self.assertEqual(tuple(result[0, 0]), BLUE)
        self.assertEqual(int(numpy.all(result == BLUE, axis=2).sum()), 1)


class FootprintTests(unittest.TestCase):
    def offsets(self, diameter, shape):
        return {(pixel.column_offset, pixel.row_offset) for pixel in va.brush_footprint(diameter, shape)}

    def test_small_diameters(self):
        self.assertEqual(self.offsets(1, "circle"), {(0, 0)})
        self.assertEqual(self.offsets(2, "circle"), {(-1, -1), (0, -1), (-1, 0), (0, 0)})
        self.assertEqual(self.offsets(3, "circle"), {(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)})

    def test_pixel_counts_of_round_brushes_and_square_brushes(self):
        self.assertEqual({d: len(va.brush_footprint(d, "circle")) for d in range(1, 8)}, {1: 1, 2: 4, 3: 5, 4: 12, 5: 21, 6: 32, 7: 37})
        for diameter in range(1, 9):
            self.assertEqual(len(va.brush_footprint(diameter, "square")), diameter * diameter)

    def test_square_footprints_follow_the_pencil_anchor(self):
        for diameter in range(1, 8):
            expected = {
                (column - 5, row - 5) for row, column in numpy.argwhere(va.rectangle_brush_mask((11, 11), 5, 5, diameter, diameter))
            }
            self.assertEqual(self.offsets(diameter, "square"), expected)

    def test_odd_circles_are_symmetric_and_even_circles_are_symmetric_about_their_corner(self):
        for diameter in (3, 5, 7, 9):
            offsets = self.offsets(diameter, "circle")
            self.assertEqual(offsets, {(-c, r) for c, r in offsets})
            self.assertEqual(offsets, {(c, -r) for c, r in offsets})
            self.assertEqual(offsets, {(r, c) for c, r in offsets})
        for diameter in (2, 4, 6, 8):
            offsets = self.offsets(diameter, "circle")
            self.assertEqual(offsets, {(-1 - c, r) for c, r in offsets})
            self.assertEqual(offsets, {(c, -1 - r) for c, r in offsets})

    def test_listed_in_reading_order_with_distance_ratios(self):
        footprint = va.brush_footprint(3, "circle")
        self.assertEqual([(p.column_offset, p.row_offset) for p in footprint], [(0, -1), (-1, 0), (0, 0), (1, 0), (0, 1)])
        self.assertEqual(footprint[2].distance_ratio, 0.0)
        self.assertAlmostEqual(footprint[0].distance_ratio, 2 / 3)

    def test_bad_shape_is_an_error(self):
        with self.assertRaises(ValueError):
            va.brush_footprint(3, "triangle")


class ScriptedRandom:
    """Returns the given numbers, then repeats the last one."""

    def __init__(self, *numbers):
        self.numbers = list(numbers)
        self.calls = 0

    def random(self):
        self.calls += 1
        return self.numbers[min(self.calls - 1, len(self.numbers) - 1)]


class NoiseBrushTests(unittest.TestCase):
    def test_density_one_fills_the_footprint_and_density_zero_fills_nothing(self):
        pixels = solid_tile(TRANSPARENT, 9)
        va.noise_stamp(pixels, 4, 4, RED, 5, "circle", 1.0, 1.0, random.Random(1))
        self.assertEqual(int(numpy.all(pixels == RED, axis=2).sum()), 21)
        cleared = solid_tile(TRANSPARENT, 9)
        va.noise_stamp(cleared, 4, 4, RED, 5, "circle", 0.0, 1.0, random.Random(1))
        self.assertTrue(numpy.all(cleared == 0))

    def test_each_pixel_has_its_own_roll_in_reading_order(self):
        pixels = solid_tile(TRANSPARENT, 5)
        # Diameter 3 circle (a plus): rolls go to (0,-1), (-1,0), (0,0), (1,0), (0,1). Density 0.5.
        source = ScriptedRandom(0.1, 0.9, 0.4, 0.5, 0.0)
        va.noise_stamp(pixels, 2, 2, RED, 3, "circle", 0.5, 1.0, source)
        painted = {(int(c), int(r)) for r, c in numpy.argwhere(pixels[..., 3] > 0)}
        self.assertEqual(painted, {(2, 1), (2, 2), (2, 3)})

    def test_density_is_the_probability_of_a_roll_being_true(self):
        pixels = solid_tile(TRANSPARENT, 40)
        va.noise_stamp(pixels, 20, 20, RED, 30, "square", 0.3, 1.0, random.Random(42))
        fraction = int((pixels[..., 3] > 0).sum()) / 900
        self.assertAlmostEqual(fraction, 0.3, delta=0.05)

    def test_stamps_consume_the_same_rolls_at_the_edge_as_in_the_middle(self):
        for column in (-3, 0, 4):
            source = ScriptedRandom(0.0)
            va.noise_stamp(solid_tile(TRANSPARENT, 9), column, 4, RED, 5, "circle", 1.0, 1.0, source)
            self.assertEqual(source.calls, 21)

    def test_clipped_at_the_canvas_edge(self):
        pixels = solid_tile(TRANSPARENT, 4)
        va.noise_stamp(pixels, 0, 0, RED, 5, "square", 1.0, 1.0, random.Random(1))
        self.assertEqual(int((pixels[..., 3] > 0).sum()), 9)

    def test_opacity_one_writes_the_exact_color_even_over_other_pixels_and_with_alpha(self):
        pixels = solid_tile(BLUE, 3)
        va.noise_stamp(pixels, 1, 1, (10, 20, 30, 77), 3, "square", 1.0, 1.0, random.Random(1))
        self.assertTrue(numpy.all(pixels == (10, 20, 30, 77)))

    def test_lower_opacity_composites_and_repeated_hits_accumulate(self):
        pixels = solid_tile(TRANSPARENT, 3)
        alphas = []
        for _ in range(3):
            va.noise_stamp(pixels, 1, 1, RED, 1, "square", 1.0, 0.5, random.Random(1))
            alphas.append(int(pixels[1, 1, 3]))
        self.assertEqual(alphas, [128, 192, 224])
        self.assertEqual(tuple(pixels[1, 1, :3]), (255, 0, 0))
        self.assertEqual(int((pixels[..., 3] > 0).sum()), 1)

    def test_opacity_scales_the_colors_own_alpha(self):
        pixels = solid_tile(TRANSPARENT, 3)
        va.noise_stamp(pixels, 1, 1, (255, 0, 0, 128), 1, "square", 1.0, 0.5, random.Random(1))
        self.assertEqual(int(pixels[1, 1, 3]), 64)

    def test_editable_mask_protects_cells(self):
        pixels = solid_tile(TRANSPARENT, 4)
        editable = numpy.ones((4, 4), dtype=bool)
        editable[:, :2] = False
        va.noise_stamp(pixels, 2, 2, RED, 4, "square", 1.0, 1.0, random.Random(1), editable)
        self.assertTrue(numpy.all(pixels[:, :2] == 0))
        self.assertTrue(numpy.any(pixels[:, 2:] != 0))


class AirbrushTests(unittest.TestCase):
    def test_hard_full_opacity_brush_paints_the_footprint_exactly(self):
        before = solid_tile(BLUE, 9)
        stroke = va.AirbrushStroke(before, RED, 5, 1.0, 1.0)
        stroke.stamp(4, 4)
        result = stroke.result()
        self.assertEqual(int(numpy.all(result == RED, axis=2).sum()), 21)
        self.assertEqual(int(numpy.all(result == BLUE, axis=2).sum()), 81 - 21)

    def test_does_not_modify_the_pixels_it_was_given(self):
        before = solid_tile(BLUE, 5)
        stroke = va.AirbrushStroke(before, RED, 3, 0.0, 1.0)
        stroke.stamp(2, 2)
        stroke.result()
        self.assertTrue(numpy.all(before == BLUE))

    def test_soft_brush_is_strongest_in_the_center_and_fades_outward(self):
        stroke = va.AirbrushStroke(solid_tile(TRANSPARENT, 21), RED, 15, 0.0, 1.0)
        stroke.stamp(10, 10)
        row = stroke.coverage[10]
        self.assertEqual(row[10], 1.0)
        for column in range(10, 17):
            self.assertGreaterEqual(row[column], row[column + 1])
        self.assertLess(row[16], row[12])

    def test_hardness_keeps_full_strength_out_to_that_fraction_of_the_radius(self):
        stroke = va.AirbrushStroke(solid_tile(TRANSPARENT, 31), RED, 21, 0.5, 1.0)
        self.assertEqual(stroke.falloff(0.0), 1.0)
        self.assertEqual(stroke.falloff(0.5), 1.0)
        self.assertAlmostEqual(stroke.falloff(0.75), 0.5)
        self.assertEqual(stroke.falloff(1.0), 0.0)

    def test_opacity_is_the_ceiling_of_the_coverage(self):
        stroke = va.AirbrushStroke(solid_tile(TRANSPARENT, 9), RED, 5, 1.0, 0.4)
        stroke.stamp(4, 4)
        self.assertAlmostEqual(float(stroke.coverage.max()), 0.4)

    def test_passing_over_a_pixel_again_within_a_stroke_does_not_pile_up(self):
        once = va.AirbrushStroke(solid_tile(TRANSPARENT, 15), RED, 7, 0.3, 0.5)
        once.stamp(7, 7)
        many = va.AirbrushStroke(solid_tile(TRANSPARENT, 15), RED, 7, 0.3, 0.5)
        for _ in range(6):
            many.stamp(7, 7)
            many.stamp(8, 7)
            many.stamp(7, 7)
        self.assertLessEqual(float(many.coverage.max()), 0.5)
        self.assertTrue(numpy.array_equal(many.result()[7, 7], once.result()[7, 7]))

    def test_a_second_stroke_composites_over_the_first(self):
        first = va.AirbrushStroke(solid_tile(TRANSPARENT, 5), RED, 1, 1.0, 0.5)
        first.stamp(2, 2)
        after_first = first.result()
        second = va.AirbrushStroke(after_first, RED, 1, 1.0, 0.5)
        second.stamp(2, 2)
        self.assertEqual(int(after_first[2, 2, 3]), 128)
        self.assertEqual(int(second.result()[2, 2, 3]), 192)

    def test_color_over_existing_pixels_blends(self):
        stroke = va.AirbrushStroke(solid_tile((0, 0, 0, 255), 3), (255, 255, 255, 255), 1, 1.0, 0.5)
        stroke.stamp(1, 1)
        self.assertEqual(tuple(stroke.result()[1, 1]), (128, 128, 128, 255))

    def test_the_colors_own_alpha_is_multiplied_in(self):
        stroke = va.AirbrushStroke(solid_tile(TRANSPARENT, 3), (255, 0, 0, 128), 1, 1.0, 0.5)
        stroke.stamp(1, 1)
        self.assertEqual(int(stroke.result()[1, 1, 3]), 64)

    def test_path_stamps_every_pixel_between_and_edges_are_clipped(self):
        stroke = va.AirbrushStroke(solid_tile(TRANSPARENT, 12), RED, 1, 1.0, 1.0)
        stroke.stamp_path((0, 0), (11, 0))
        self.assertTrue(numpy.all(stroke.coverage[0] == 1.0))
        stroke.stamp(-50, -50)

    def test_editable_mask_protects_cells(self):
        editable = numpy.ones((6, 6), dtype=bool)
        editable[:, 3:] = False
        stroke = va.AirbrushStroke(solid_tile(TRANSPARENT, 6), RED, 5, 1.0, 1.0, editable)
        stroke.stamp(3, 3)
        self.assertTrue(numpy.all(stroke.coverage[:, 3:] == 0))
        self.assertTrue(numpy.any(stroke.coverage[:, :3] > 0))


class HistoryTests(unittest.TestCase):
    def setUp(self):
        self.history = va.EditHistory()
        self.blank = solid_tile(TRANSPARENT)
        self.red = solid_tile(RED)
        self.green = solid_tile(GREEN)
        self.blue = solid_tile(BLUE)

    def test_undo_returns_the_before_pixels_and_redo_the_after_pixels(self):
        self.history.record({"a": (self.blank, self.red)})
        undone = self.history.undo(["a"])
        self.assertTrue(numpy.array_equal(undone["a"], self.blank))
        redone = self.history.redo(["a"])
        self.assertTrue(numpy.array_equal(redone["a"], self.red))

    def test_steps_come_back_in_reverse_order(self):
        self.history.record({"a": (self.blank, self.red)})
        self.history.record({"a": (self.red, self.green)})
        self.assertTrue(numpy.array_equal(self.history.undo(["a"])["a"], self.red))
        self.assertTrue(numpy.array_equal(self.history.undo(["a"])["a"], self.blank))
        self.assertIsNone(self.history.undo(["a"]))
        self.assertTrue(numpy.array_equal(self.history.redo(["a"])["a"], self.red))
        self.assertTrue(numpy.array_equal(self.history.redo(["a"])["a"], self.green))
        self.assertIsNone(self.history.redo(["a"]))

    def test_a_new_edit_clears_redo(self):
        self.history.record({"a": (self.blank, self.red)})
        self.history.undo(["a"])
        self.assertTrue(self.history.can_redo(["a"]))
        self.history.record({"a": (self.blank, self.green)})
        self.assertFalse(self.history.can_redo(["a"]))

    def test_tiles_have_separate_histories(self):
        self.history.record({"a": (self.blank, self.red)})
        self.history.record({"b": (self.blank, self.green)})
        undone = self.history.undo(["a"])
        self.assertEqual(set(undone), {"a"})
        self.assertTrue(self.history.can_undo(["b"]))
        self.assertFalse(self.history.can_undo(["a"]))

    def test_undo_among_several_shown_tiles_takes_the_latest_edit(self):
        self.history.record({"a": (self.blank, self.red)})
        self.history.record({"b": (self.blank, self.green)})
        self.assertEqual(set(self.history.undo(["a", "b"])), {"b"})
        self.assertEqual(set(self.history.undo(["a", "b"])), {"a"})

    def test_an_edit_across_tiles_is_one_step_even_if_only_one_is_shown(self):
        self.history.record({"a": (self.blank, self.red), "b": (self.blank, self.green)})
        undone = self.history.undo(["a"])
        self.assertEqual(set(undone), {"a", "b"})
        self.assertFalse(self.history.can_undo(["a", "b"]))
        redone = self.history.redo(["b"])
        self.assertEqual(set(redone), {"a", "b"})
        self.assertTrue(numpy.array_equal(redone["b"], self.green))

    def test_a_hidden_tile_with_newer_edits_keeps_them(self):
        self.history.record({"a": (self.blank, self.red), "b": (self.blank, self.green)})
        self.history.record({"b": (self.green, self.blue)})
        undone = self.history.undo(["a"])
        self.assertEqual(set(undone), {"a"}, "b has a newer edit on top, so only a's part is stepped back")
        undone_b = self.history.undo(["b"])
        self.assertTrue(numpy.array_equal(undone_b["b"], self.green))
        undone_b_again = self.history.undo(["b"])
        self.assertTrue(numpy.array_equal(undone_b_again["b"], self.blank))

    def test_redo_steps_forward_in_the_reverse_order_of_the_undos_across_tiles(self):
        self.history.record({"a": (self.blank, self.red)})
        self.history.record({"b": (self.blank, self.green)})
        self.history.undo(["a", "b"])  # takes b's edit
        self.history.undo(["a", "b"])  # then a's edit
        self.assertEqual(set(self.history.redo(["a", "b"])), {"a"})
        self.assertEqual(set(self.history.redo(["a", "b"])), {"b"})

    def test_unchanged_tiles_are_not_recorded(self):
        self.history.record({"a": (self.red, self.red.copy())})
        self.assertFalse(self.history.can_undo(["a"]))
        self.history.record({"a": (self.red, self.green), "b": (self.red, self.red.copy())})
        self.assertEqual(set(self.history.undo(["a", "b"])), {"a"})

    def test_recorded_pixels_are_copied(self):
        before = self.blank.copy()
        after = self.red.copy()
        self.history.record({"a": (before, after)})
        before[...] = 9
        after[...] = 9
        undone = self.history.undo(["a"])["a"]
        self.assertTrue(numpy.array_equal(undone, self.blank))
        undone[...] = 7
        self.assertTrue(numpy.array_equal(self.history.redo(["a"])["a"], self.red))

    def test_depth_limit_drops_the_oldest_steps(self):
        history = va.EditHistory(depth=3)
        colors = [solid_tile((i, 0, 0, 255)) for i in range(6)]
        for older, newer in zip(colors, colors[1:]):
            history.record({"a": (older, newer)})
        steps = 0
        while history.undo(["a"]) is not None:
            steps += 1
        self.assertEqual(steps, 3)

    def test_forget_removes_a_tile(self):
        self.history.record({"a": (self.blank, self.red)})
        self.history.forget("a")
        self.assertFalse(self.history.can_undo(["a"]))
        self.assertIsNone(self.history.undo(["a"]))

    def test_nothing_shown_means_nothing_to_undo(self):
        self.history.record({"a": (self.blank, self.red)})
        self.assertIsNone(self.history.undo([]))
        self.assertIsNone(self.history.redo(["a"]))


class ProjectFixture(unittest.TestCase):
    """Builds a folder of tiles on disk, with an atlas.json, for tests of the project and the window."""

    tile_size = 4

    def setUp(self):
        self.temporary_folder = tempfile.TemporaryDirectory()
        self.folder = Path(self.temporary_folder.name)

    def tearDown(self):
        self.temporary_folder.cleanup()

    def make_project(self, placed=None, unplaced=(), data=None, write_atlas=True, other_settings=None, tile_size=None):
        """placed: {coordinate: (name, pixels or None for no file)}; unplaced: [(name, pixels)]."""
        size = tile_size or self.tile_size
        names = {}
        for coordinate, (name, pixels) in (placed or {}).items():
            names[coordinate] = name
            if pixels is not None:
                va.write_png_atomically(self.folder / name, pixels)
        for name, pixels in unplaced:
            va.write_png_atomically(self.folder / name, pixels)
        if write_atlas:
            atlas_file = va.AtlasFile(size, names, dict(data or {}), dict(other_settings or {}))
            va.write_atlas_file(self.folder / "atlas.json", atlas_file)
        return va.AtlasProject.open(self.folder)

    def saved(self):
        return va.read_atlas_file(self.folder / "atlas.json")


class ProjectTests(ProjectFixture):
    def test_a_folder_without_atlas_json_is_empty_with_a_provisional_tile_size_and_lists_unplaced_tiles(self):
        project = self.make_project(unplaced=[("a.png", solid_tile(RED, 16))], write_atlas=False)
        self.assertTrue(project.is_empty)
        self.assertEqual((project.tile_size, project.tile_size_is_saved, project.atlas_file_exists), (16, False, False))
        self.assertEqual([tile.name for tile in project.unplaced], ["a.png"])
        self.assertFalse((self.folder / "atlas.json").exists(), "opening must not create atlas.json")

    def test_placing_an_unplaced_tile_saves_atlas_json_immediately_and_does_not_select_it(self):
        project = self.make_project(unplaced=[("a.png", solid_tile(RED))], write_atlas=False)
        self.assertTrue(project.set_tile_size(4))
        self.assertTrue(project.place_unplaced_tile(0, (2, 1)))
        self.assertEqual(project.unplaced, [])
        self.assertEqual(project.selection, set())
        saved = self.saved()
        self.assertEqual((saved.tile_size, saved.coordinates_to_names), (4, {(2, 1): "a.png"}))
        self.assertEqual(saved.coordinates_to_data, {})

    def test_placing_on_an_occupied_or_negative_place_is_refused_and_changes_nothing(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED))}, [("b.png", solid_tile(GREEN))])
        before = (self.folder / "atlas.json").read_bytes()
        self.assertFalse(project.place_unplaced_tile(0, (0, 0)))
        self.assertFalse(project.place_unplaced_tile(0, (-1, 0)))
        self.assertEqual(len(project.unplaced), 1)
        self.assertEqual((self.folder / "atlas.json").read_bytes(), before)

    def test_moving_the_selection_saves_immediately(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED)), (1, 0): ("b.png", solid_tile(GREEN))})
        project.select_only((0, 0))
        self.assertTrue(project.move_selection((0, 0), (1, 0)))
        self.assertEqual(self.saved().coordinates_to_names, {(1, 0): "a.png", (0, 0): "b.png"})
        self.assertEqual(project.selection, {(1, 0)})

    def test_a_refused_move_changes_nothing(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED))})
        project.select_only((0, 0))
        before = (self.folder / "atlas.json").read_bytes()
        self.assertFalse(project.move_selection((0, 0), (-1, 0)))
        self.assertEqual(project.selection, {(0, 0)})
        self.assertEqual((self.folder / "atlas.json").read_bytes(), before)

    def test_a_failed_save_undoes_the_change_and_raises(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED)), (1, 0): ("b.png", solid_tile(GREEN))})
        project.select_only((0, 0))
        before = (self.folder / "atlas.json").read_bytes()
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                project.move_selection((0, 0), (1, 0))
        self.assertEqual(set(project.tiles), {(0, 0), (1, 0)})
        self.assertEqual(project.tiles[(0, 0)].name, "a.png")
        self.assertEqual(project.selection, {(0, 0)})
        self.assertEqual((self.folder / "atlas.json").read_bytes(), before)

    def test_new_tiles_keep_their_pixel_data_in_atlas_json_through_moves_and_have_no_file(self):
        pixels = random_tile(3)
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(pixels)})
        self.assertTrue(project.tiles[(0, 0)].is_new)
        project.select_only((0, 0))
        project.move_selection((0, 0), (3, 3))
        saved = self.saved()
        self.assertEqual(set(saved.coordinates_to_data), {(3, 3)})
        self.assertTrue(numpy.array_equal(va.decode_tile_data(saved.coordinates_to_data[(3, 3)], 4), pixels))
        self.assertFalse((self.folder / "n.png").exists())

    def test_stored_data_that_cannot_be_used_is_preserved_when_saving(self):
        project = self.make_project(
            {(0, 0): ("bad.png", None), (1, 0): ("a.png", solid_tile(RED))}, data={(0, 0): "AAAA"}
        )
        self.assertIn("stored pixel data", project.tiles[(0, 0)].error)
        project.select_only((1, 0))
        project.move_selection((1, 0), (2, 0))
        self.assertEqual(self.saved().coordinates_to_data, {(0, 0): "AAAA"})

    def test_unknown_settings_survive_saving(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED))}, other_settings={"future": {"x": 1}})
        project.select_only((0, 0))
        project.move_selection((0, 0), (1, 1))
        self.assertEqual(self.saved().other_settings, {"future": {"x": 1}})

    def test_stored_data_equal_to_its_file_is_removed_from_atlas_json_on_open(self):
        pixels = random_tile(8)
        project = self.make_project({(0, 0): ("a.png", pixels)}, data={(0, 0): va.encode_tile_data(pixels)})
        self.assertEqual(self.saved().coordinates_to_data, {})
        self.assertFalse(project.tiles[(0, 0)].is_modified)

    def test_only_unmodified_tiles_with_files_can_return_to_unplaced_and_it_is_all_or_none(self):
        project = self.make_project(
            {(0, 0): ("a.png", solid_tile(RED)), (1, 0): ("n.png", None), (2, 0): ("gone.png", None)},
            data={(1, 0): va.encode_tile_data(random_tile(1))},
        )
        self.assertFalse(project.return_to_unplaced([(1, 0)]), "a new tile has no file to return to")
        self.assertFalse(project.return_to_unplaced([(2, 0)]), "an error tile cannot return")
        self.assertFalse(project.return_to_unplaced([(0, 0), (1, 0)]))
        self.assertEqual(set(project.tiles), {(0, 0), (1, 0), (2, 0)})
        self.assertTrue(project.return_to_unplaced([(0, 0)]))
        self.assertEqual([tile.name for tile in project.unplaced], ["a.png"])
        self.assertEqual(set(self.saved().coordinates_to_names), {(1, 0), (2, 0)})

    def test_returning_deselects_and_keeps_the_unplaced_list_sorted(self):
        project = self.make_project(
            {(0, 0): ("m.png", solid_tile(RED)), (1, 0): ("b.png", solid_tile(GREEN))}, [("z.png", solid_tile(BLUE))]
        )
        project.select_only((0, 0))
        self.assertTrue(project.return_to_unplaced([(0, 0)]))
        self.assertEqual(project.selection, set())
        self.assertEqual([tile.name for tile in project.unplaced], ["m.png", "z.png"])

    def test_a_returned_tile_can_be_placed_again(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED))})
        project.return_to_unplaced([(0, 0)])
        self.assertTrue(project.place_unplaced_tile(0, (5, 5)))
        self.assertEqual(self.saved().coordinates_to_names, {(5, 5): "a.png"})

    def test_provisional_tile_size_can_change_until_it_is_saved(self):
        project = self.make_project(
            unplaced=[("small.png", solid_tile(RED, 4)), ("big.png", solid_tile(RED, 8))], write_atlas=False
        )
        self.assertEqual(project.unplaced, [])
        self.assertTrue(project.set_tile_size(8))
        self.assertEqual([tile.name for tile in project.unplaced], ["big.png"])
        self.assertTrue(project.set_tile_size(4))
        self.assertEqual([tile.name for tile in project.unplaced], ["small.png"])
        project.save_atlas_file()
        self.assertEqual(self.saved().tile_size, 4)
        self.assertFalse(project.set_tile_size(8), "a saved tile size is read-only")

    def test_tile_size_range_is_enforced(self):
        project = self.make_project(write_atlas=False)
        self.assertFalse(project.set_tile_size(0))
        self.assertFalse(project.set_tile_size(va.MAXIMUM_TILE_SIZE + 1))

    def test_tile_size_cannot_change_while_a_tile_has_unsaved_content(self):
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(random_tile(1))})
        project.tile_size_is_saved = False
        self.assertFalse(project.set_tile_size(8))

    def test_selection_helpers(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED)), (1, 0): ("b.png", solid_tile(GREEN))})
        project.select_only((0, 0))
        project.toggle_selection((1, 0))
        self.assertEqual(project.selection, {(0, 0), (1, 0)})
        project.toggle_selection((0, 0))
        self.assertEqual(project.selection, {(1, 0)})
        project.clear_selection()
        self.assertEqual(project.selection, set())

    def test_a_project_with_a_bad_atlas_file_refuses_to_open(self):
        (self.folder / "atlas.json").write_text('{"coordinates_to_names": {"(0, 0)": "a.png", "(1, 0)": "a.png"}}')
        with self.assertRaises(va.AtlasFileError):
            va.AtlasProject.open(self.folder)


class GeometryTests(unittest.TestCase):
    def setUp(self):
        self.geometry = va.GridGeometry(tile_size=16, zoom=4)  # cell 64, gap 8, pitch 72

    def test_sizes(self):
        self.assertEqual((self.geometry.cell, self.geometry.gap, self.geometry.pitch), (64, 8, 72))

    def test_tiles_are_separated_by_gaps(self):
        self.assertEqual(self.geometry.tile_origin((0, 0)), (8, 8))
        self.assertEqual(self.geometry.tile_origin((2, 1)), (8 + 144, 8 + 72))

    def test_exact_coordinate_is_none_in_gaps_and_outside(self):
        self.assertEqual(self.geometry.exact_coordinate(8, 8), (0, 0))
        self.assertEqual(self.geometry.exact_coordinate(71, 71), (0, 0))
        self.assertIsNone(self.geometry.exact_coordinate(72, 20), "the gap to the right of tile 0")
        self.assertIsNone(self.geometry.exact_coordinate(20, 75))
        self.assertIsNone(self.geometry.exact_coordinate(3, 20), "left of the first tile")
        self.assertEqual(self.geometry.exact_coordinate(80, 8), (1, 0))

    def test_nearest_coordinate_splits_the_gap_between_neighbors_and_can_be_negative(self):
        self.assertEqual(self.geometry.nearest_coordinate(8, 8), (0, 0))
        self.assertEqual(self.geometry.nearest_coordinate(75, 8), (0, 0))
        self.assertEqual(self.geometry.nearest_coordinate(76, 8), (1, 0))
        self.assertEqual(self.geometry.nearest_coordinate(4, 4), (0, 0))
        self.assertEqual(self.geometry.nearest_coordinate(3, 3), (-1, -1))
        self.assertEqual(self.geometry.nearest_coordinate(-10, -10), (-1, -1))

    def test_every_tile_point_is_nearest_to_its_own_place(self):
        for column in range(4):
            for row in range(4):
                x, y = self.geometry.tile_origin((column, row))
                for dx, dy in [(0, 0), (63, 0), (0, 63), (63, 63), (32, 32)]:
                    self.assertEqual(self.geometry.nearest_coordinate(x + dx, y + dy), (column, row))
                    self.assertEqual(self.geometry.exact_coordinate(x + dx, y + dy), (column, row))

    def test_small_cells_still_have_a_gap(self):
        self.assertEqual(va.GridGeometry(4, 1).gap, 3)

    def test_content_size(self):
        self.assertEqual(self.geometry.content_size(2, 3), (8 + 144, 8 + 216))


class PanelTests(ProjectFixture):
    def make_panels(self, placed, unplaced=()):
        project = self.make_project(placed, unplaced)
        atlas = va.AtlasPanel(project)
        unplaced_panel = va.UnplacedPanel(project)
        atlas.rect = va.pygame.Rect(0, 0, 300, 200)
        unplaced_panel.rect = va.pygame.Rect(0, 210, 300, 100)
        return project, atlas, unplaced_panel

    def test_tile_screen_rect_and_hit_test_agree_and_follow_the_panel_position(self):
        project, atlas, _ = self.make_panels({(1, 1): ("a.png", solid_tile(RED))})
        rectangle = atlas.tile_screen_rect((1, 1))
        self.assertEqual(atlas.tile_at(rectangle.center), (1, 1))
        self.assertIsNone(atlas.tile_at((rectangle.right + 1, rectangle.centery)))
        atlas.rect = va.pygame.Rect(40, 30, 300, 200)
        self.assertEqual(atlas.tile_screen_rect((1, 1)).topleft, (rectangle.x + 40, rectangle.y + 30))
        self.assertEqual(atlas.tile_at(atlas.tile_screen_rect((1, 1)).center), (1, 1))

    def test_scrolling_is_clamped_to_the_content(self):
        project, atlas, _ = self.make_panels({(0, 0): ("a.png", solid_tile(RED))})
        atlas.scroll_by(500, 500)
        self.assertEqual((atlas.scroll_x, atlas.scroll_y), (0, 0), "content smaller than the view cannot scroll")
        project, atlas, _ = self.make_panels({(20, 20): ("b.png", solid_tile(RED))})
        atlas.scroll_by(10_000, 10_000)
        width, height = atlas.geometry.content_size(*atlas.extent())
        self.assertEqual((atlas.scroll_x, atlas.scroll_y), (width - 300, height - 200))
        atlas.scroll_by(-100_000, -100_000)
        self.assertEqual((atlas.scroll_x, atlas.scroll_y), (0, 0))

    def test_scrolling_moves_what_a_position_points_at(self):
        project, atlas, _ = self.make_panels({(c, r): (f"t{c}{r}.png", solid_tile(RED)) for c in range(10) for r in range(10)})
        position = atlas.tile_screen_rect((3, 3)).center
        atlas.scroll_by(0, 72)
        self.assertEqual(atlas.tile_at(position), (3, 4))

    def test_zooming_keeps_the_content_under_the_pointer_in_place(self):
        project, atlas, _ = self.make_panels({(c, r): (f"t{c}{r}.png", solid_tile(RED)) for c in range(10) for r in range(10)})
        atlas.scroll_x, atlas.scroll_y = 100, 80
        anchor = (150, 120)
        before_place = atlas.drop_coordinate(anchor)
        atlas.zoom_by(1, anchor)
        self.assertEqual(atlas.zoom, va.default_zoom(4) + 1)
        self.assertEqual(atlas.drop_coordinate(anchor), before_place)

    def test_zoom_is_limited(self):
        project, atlas, _ = self.make_panels({(0, 0): ("a.png", solid_tile(RED))})
        atlas.zoom_by(-100, (10, 10))
        self.assertEqual(atlas.zoom, va.MINIMUM_ZOOM)
        atlas.zoom_by(100, (10, 10))
        self.assertEqual(atlas.zoom, va.MAXIMUM_ZOOM)

    def test_only_visible_places_are_listed(self):
        project, atlas, _ = self.make_panels({(30, 30): ("a.png", solid_tile(RED))})
        visible = list(atlas.visible_coordinates())
        self.assertIn((0, 0), visible)
        self.assertLess(len(visible), 100)

    def test_unplaced_tiles_wrap_into_rows(self):
        project, _, unplaced_panel = self.make_panels({}, [(f"u{i}.png", solid_tile(RED)) for i in range(10)])
        columns = unplaced_panel.columns()
        self.assertGreater(columns, 1)
        self.assertEqual(unplaced_panel.coordinate_of_index(columns), (0, 1))
        rectangle = unplaced_panel.tile_screen_rect(unplaced_panel.coordinate_of_index(7))
        self.assertEqual(unplaced_panel.tile_at(rectangle.center), 7)

    def test_unplaced_places_past_the_last_tile_are_empty(self):
        project, _, unplaced_panel = self.make_panels({}, [("u0.png", solid_tile(RED))])
        rectangle = unplaced_panel.tile_screen_rect((1, 0))
        self.assertIsNone(unplaced_panel.tile_at(rectangle.center))


class InteractionTests(ProjectFixture):
    def setUp(self):
        super().setUp()
        placed = {
            (0, 0): ("a.png", solid_tile(RED)),
            (1, 0): ("b.png", solid_tile(GREEN)),
            (2, 0): ("c.png", solid_tile(BLUE)),
            (0, 1): ("d.png", random_tile(1)),
        }
        self.project = self.make_project(placed, [("u.png", solid_tile((9, 9, 9, 255)))])
        self.atlas, self.unplaced, self.interaction = self.make_interaction(self.project)

    def make_interaction(self, project):
        atlas, unplaced = va.AtlasPanel(project), va.UnplacedPanel(project)
        atlas.rect = va.pygame.Rect(0, 0, 700, 500)
        unplaced.rect = va.pygame.Rect(0, 510, 700, 150)
        return atlas, unplaced, va.AtlasInteraction(project, atlas, unplaced)

    def center(self, coordinate):
        return self.atlas.tile_screen_rect(coordinate).center

    def names(self):
        return {coordinate: record.name for coordinate, record in self.project.tiles.items()}

    def click(self, position, control=False):
        self.interaction.mouse_down(position, control)
        self.interaction.mouse_up(position)

    def drag(self, start, end, control=False):
        self.interaction.mouse_down(start, control)
        self.interaction.mouse_move((start[0] + 20, start[1] + 20))
        self.interaction.mouse_move(end)
        self.interaction.mouse_up(end)

    def test_clicking_a_tile_selects_only_it(self):
        self.click(self.center((0, 0)))
        self.click(self.center((1, 0)))
        self.assertEqual(self.project.selection, {(1, 0)})

    def test_control_click_adds_and_removes(self):
        self.click(self.center((0, 0)))
        self.click(self.center((1, 0)), control=True)
        self.assertEqual(self.project.selection, {(0, 0), (1, 0)})
        self.click(self.center((0, 0)), control=True)
        self.assertEqual(self.project.selection, {(1, 0)})

    def test_clicking_a_tile_inside_a_multiple_selection_selects_only_it_on_release(self):
        self.project.selection = {(0, 0), (1, 0)}
        self.click(self.center((0, 0)))
        self.assertEqual(self.project.selection, {(0, 0)})

    def test_clicking_empty_space_clears_the_selection_but_control_click_does_not(self):
        self.project.selection = {(0, 0)}
        empty = self.center((5, 5))
        self.click(empty, control=True)
        self.assertEqual(self.project.selection, {(0, 0)})
        self.click(empty)
        self.assertEqual(self.project.selection, set())

    def test_clicking_a_gap_counts_as_empty_space(self):
        self.project.selection = {(0, 0)}
        rectangle = self.atlas.tile_screen_rect((0, 0))
        self.click((rectangle.right + 1, rectangle.centery))
        self.assertEqual(self.project.selection, set())

    def test_a_small_movement_is_still_a_click(self):
        self.project.selection = {(0, 0), (1, 0)}
        start = self.center((0, 0))
        self.interaction.mouse_down(start, False)
        self.interaction.mouse_move((start[0] + 2, start[1]))
        self.assertFalse(self.interaction.dragging)
        self.interaction.mouse_up((start[0] + 2, start[1]))
        self.assertEqual(self.project.selection, {(0, 0)})

    def test_dragging_a_tile_onto_another_swaps_them_and_saves(self):
        self.drag(self.center((0, 0)), self.center((1, 0)))
        self.assertEqual(self.names()[(0, 0)], "b.png")
        self.assertEqual(self.names()[(1, 0)], "a.png")
        self.assertEqual(self.saved().coordinates_to_names[(1, 0)], "a.png")

    def test_dragging_an_unselected_tile_selects_it_and_drops_the_old_selection(self):
        self.project.selection = {(1, 0), (2, 0)}
        self.drag(self.center((0, 0)), self.center((4, 4)))
        self.assertEqual(self.project.selection, {(4, 4)})
        self.assertEqual(self.names()[(4, 4)], "a.png")
        self.assertEqual(self.names()[(1, 0)], "b.png", "the old selection did not move")

    def test_dragging_a_selected_tile_moves_the_whole_selection(self):
        self.project.selection = {(0, 0), (1, 0)}
        self.drag(self.center((0, 0)), self.center((0, 3)))
        self.assertEqual(self.names()[(0, 3)], "a.png")
        self.assertEqual(self.names()[(1, 3)], "b.png")
        self.assertEqual(self.project.selection, {(0, 3), (1, 3)})

    def test_control_dragging_an_unselected_tile_adds_it_to_the_group(self):
        self.project.selection = {(0, 0)}
        self.drag(self.center((1, 0)), self.center((1, 3)), control=True)
        self.assertEqual(self.project.selection, {(0, 3), (1, 3)})

    def test_dropping_where_it_started_changes_nothing(self):
        before = (self.folder / "atlas.json").read_bytes()
        self.drag(self.center((0, 0)), self.center((0, 0)))
        self.assertEqual((self.folder / "atlas.json").read_bytes(), before)

    def test_a_drop_that_would_go_negative_is_refused_and_flagged_invalid_while_dragging(self):
        self.project.selection = {(0, 0), (1, 0)}
        start = self.center((1, 0))
        self.interaction.mouse_down(start, False)
        self.interaction.mouse_move((start[0] + 20, start[1]))
        self.interaction.mouse_move(self.center((0, 0)))
        self.assertTrue(self.interaction.dragging)
        self.assertFalse(self.interaction.drag.drop_is_valid)
        self.interaction.mouse_up(self.center((0, 0)))
        self.assertEqual(self.names()[(0, 0)], "a.png")

    def test_the_drag_reports_validity_for_a_good_drop(self):
        start = self.center((0, 0))
        self.interaction.mouse_down(start, False)
        target = (start[0] + 150, start[1])
        self.interaction.mouse_move(target)
        self.assertTrue(self.interaction.drag.drop_is_valid)
        self.assertEqual(self.interaction.drag.drop_coordinate, self.atlas.drop_coordinate(target))

    def test_escape_cancels_a_drag(self):
        start = self.center((0, 0))
        self.interaction.mouse_down(start, False)
        self.interaction.mouse_move(self.center((3, 3)))
        self.interaction.cancel()
        self.interaction.mouse_up(self.center((3, 3)))
        self.assertEqual(self.names()[(0, 0)], "a.png")

    def test_dragging_an_unplaced_tile_into_an_empty_place_places_it_without_selecting(self):
        self.project.selection = {(0, 0)}
        self.drag(self.unplaced.tile_screen_rect((0, 0)).center, self.center((3, 3)))
        self.assertEqual(self.names()[(3, 3)], "u.png")
        self.assertEqual(self.project.unplaced, [])
        self.assertEqual(self.project.selection, {(0, 0)}, "the selection is untouched")

    def test_dragging_an_unplaced_tile_onto_an_occupied_place_is_refused(self):
        self.drag(self.unplaced.tile_screen_rect((0, 0)).center, self.center((1, 0)))
        self.assertEqual(len(self.project.unplaced), 1)
        self.assertEqual(self.names()[(1, 0)], "b.png")

    def test_an_unplaced_tile_over_an_occupied_place_is_flagged_invalid_while_dragging(self):
        start = self.unplaced.tile_screen_rect((0, 0)).center
        self.interaction.mouse_down(start, False)
        self.interaction.mouse_move(self.center((1, 0)))
        self.assertFalse(self.interaction.drag.drop_is_valid)
        self.interaction.mouse_move(self.center((6, 6)))
        self.assertTrue(self.interaction.drag.drop_is_valid)

    def test_clicking_an_unplaced_tile_selects_nothing(self):
        self.project.selection = {(0, 0)}
        self.click(self.unplaced.tile_screen_rect((0, 0)).center)
        self.assertEqual(self.project.selection, {(0, 0)})

    def test_dragging_an_atlas_tile_into_the_unplaced_section_returns_it(self):
        self.drag(self.center((0, 0)), self.unplaced.rect.center)
        self.assertNotIn((0, 0), self.project.tiles)
        self.assertEqual({tile.name for tile in self.project.unplaced}, {"a.png", "u.png"})
        self.assertNotIn("a.png", self.saved().coordinates_to_names.values())

    def test_dragging_a_group_with_a_tile_that_cannot_return_returns_nothing(self):
        project = self.make_project(
            {(0, 0): ("a.png", solid_tile(RED)), (1, 0): ("n.png", None)}, data={(1, 0): va.encode_tile_data(random_tile(5))}
        )
        atlas, unplaced, interaction = self.make_interaction(project)
        project.selection = {(0, 0), (1, 0)}
        start = atlas.tile_screen_rect((0, 0)).center
        interaction.mouse_down(start, False)
        interaction.mouse_move(unplaced.rect.center)
        self.assertFalse(interaction.drag.drop_is_valid)
        interaction.mouse_up(unplaced.rect.center)
        self.assertEqual(set(project.tiles), {(0, 0), (1, 0)})

    def test_a_failed_save_during_a_drop_raises_and_leaves_the_arrangement(self):
        start, end = self.center((0, 0)), self.center((1, 0))
        self.interaction.mouse_down(start, False)
        self.interaction.mouse_move(end)
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                self.interaction.mouse_up(end)
        self.assertEqual(self.names()[(0, 0)], "a.png")
        self.assertIsNone(self.interaction.drag)


class FakeClipboard:
    def __init__(self, text=""):
        self.text = text

    def get_text(self):
        return self.text

    def set_text(self, text):
        self.text = text


class TextFieldTests(unittest.TestCase):
    CONTROL = va.pygame.KMOD_CTRL
    SHIFT = va.pygame.KMOD_SHIFT

    def field(self, text="dirt.png", **keywords):
        return va.TextField(text, clipboard=FakeClipboard(), **keywords)

    def test_typing_inserts_at_the_cursor_and_replaces_the_selection(self):
        field = self.field("abcd")
        field.select(1, 3)
        field.insert_text("XY!")
        self.assertEqual((field.text, field.cursor), ("aXY!d", 4))

    def test_arrows_home_end_and_shift_selection(self):
        field = self.field("abcd")
        field.handle_key_down(va.pygame.K_HOME, 0)
        self.assertEqual(field.cursor, 0)
        field.handle_key_down(va.pygame.K_RIGHT, self.SHIFT)
        field.handle_key_down(va.pygame.K_RIGHT, self.SHIFT)
        self.assertEqual(field.selected_text, "ab")
        field.handle_key_down(va.pygame.K_RIGHT, 0)
        self.assertEqual((field.cursor, field.has_selection), (2, False))
        field.handle_key_down(va.pygame.K_END, 0)
        self.assertEqual(field.cursor, 4)

    def test_backspace_and_delete(self):
        field = self.field("abcd")
        field.select(2, 2)
        field.handle_key_down(va.pygame.K_BACKSPACE, 0)
        self.assertEqual(field.text, "acd")
        field.handle_key_down(va.pygame.K_DELETE, 0)
        self.assertEqual(field.text, "ad")
        field.select(0, 2)
        field.handle_key_down(va.pygame.K_DELETE, 0)
        self.assertEqual(field.text, "")
        field.handle_key_down(va.pygame.K_BACKSPACE, 0)
        field.handle_key_down(va.pygame.K_DELETE, 0)
        self.assertEqual(field.text, "")

    def test_control_a_selects_everything_and_the_clipboard_keys_work(self):
        field = self.field("abc")
        field.handle_key_down(va.pygame.K_a, self.CONTROL)
        self.assertEqual(field.selected_text, "abc")
        field.handle_key_down(va.pygame.K_x, self.CONTROL)
        self.assertEqual((field.text, field.clipboard.text), ("", "abc"))
        field.handle_key_down(va.pygame.K_v, self.CONTROL)
        self.assertEqual(field.text, "abc")
        field.select(1, 2)
        field.handle_key_down(va.pygame.K_c, self.CONTROL)
        self.assertEqual(field.clipboard.text, "b")

    def test_pasting_flattens_line_breaks(self):
        field = self.field("")
        field.clipboard.text = "a\r\nb"
        field.handle_key_down(va.pygame.K_v, self.CONTROL)
        self.assertEqual(field.text, "a b")

    def test_accepted_characters_and_length_limit(self):
        field = self.field("", accepted_characters=str.isdigit, maximum_length=4)
        field.insert_text("12ab345")
        self.assertEqual(field.text, "1234")
        field.insert_text("9")
        self.assertEqual(field.text, "1234")
        field.select(0, 2)
        field.insert_text("99")
        self.assertEqual(field.text, "9934")

    def test_unrelated_keys_are_not_used(self):
        self.assertFalse(self.field().handle_key_down(va.pygame.K_F5, 0))
        self.assertFalse(self.field().handle_key_down(va.pygame.K_ESCAPE, 0))


class LayoutTests(unittest.TestCase):
    def test_the_window_is_divided_in_half_without_overlaps(self):
        for size in [(1280, 800), (900, 600), (2000, 1200)]:
            layout = va.compute_layout(size)
            half = size[0] // 2
            for rectangle in (layout.atlas_title, layout.atlas, layout.unplaced_title, layout.unplaced):
                self.assertLess(rectangle.right, half + 1, size)
            self.assertGreaterEqual(layout.editor.x, half)
            self.assertLessEqual(layout.editor.right, size[0])
            self.assertLessEqual(layout.atlas.bottom, layout.unplaced_title.y)
            self.assertLessEqual(layout.unplaced.bottom, layout.status.y)
            self.assertGreater(layout.atlas.height, 100)


class WindowTests(ProjectFixture):
    """Draws the window into memory and checks pixels."""

    @classmethod
    def setUpClass(cls):
        va.pygame.init()

    def make_application(self, *arguments, **keywords):
        project = self.make_project(*arguments, **keywords)
        surface = va.pygame.Surface((1280, 800))
        return va.Application(project, surface), project

    def draw(self, application):
        application.draw()
        return application.surface

    def press(self, application, position):
        application.handle_event(va.pygame.event.Event(va.pygame.MOUSEBUTTONDOWN, pos=position, button=1))

    def release(self, application, position):
        application.handle_event(va.pygame.event.Event(va.pygame.MOUSEBUTTONUP, pos=position, button=1))

    def move(self, application, position):
        application.handle_event(va.pygame.event.Event(va.pygame.MOUSEMOTION, pos=position, rel=(0, 0), buttons=(1, 0, 0)))

    def test_tiles_appear_in_the_atlas_where_the_geometry_says(self):
        application, project = self.make_application(
            {(0, 0): ("a.png", solid_tile(RED)), (2, 1): ("b.png", solid_tile(GREEN))}
        )
        surface = self.draw(application)
        self.assertEqual(tuple(surface.get_at(application.atlas_panel.tile_screen_rect((0, 0)).center))[:3], RED[:3])
        self.assertEqual(tuple(surface.get_at(application.atlas_panel.tile_screen_rect((2, 1)).center))[:3], GREEN[:3])
        empty = application.atlas_panel.tile_screen_rect((1, 0)).center
        self.assertEqual(tuple(surface.get_at(empty))[:3], va.Theme.PANEL)

    def test_the_space_between_tiles_looks_like_an_empty_place(self):
        application, _ = self.make_application({(0, 0): ("a.png", solid_tile(RED)), (1, 0): ("b.png", solid_tile(RED))})
        surface = self.draw(application)
        first = application.atlas_panel.tile_screen_rect((0, 0))
        second = application.atlas_panel.tile_screen_rect((1, 0))
        gap = ((first.right + second.x) // 2, first.centery)
        self.assertEqual(tuple(surface.get_at(gap))[:3], va.Theme.PANEL)

    def test_selected_tiles_get_an_outline(self):
        application, project = self.make_application({(0, 0): ("a.png", solid_tile(RED))})
        project.select_only((0, 0))
        surface = self.draw(application)
        rectangle = application.atlas_panel.tile_screen_rect((0, 0))
        self.assertEqual(tuple(surface.get_at((rectangle.centerx, rectangle.y - 2)))[:3], va.Theme.ACCENT)

    def test_unsaved_tiles_get_a_marker_and_clean_ones_do_not(self):
        pixels = random_tile(2)
        application, project = self.make_application(
            {(0, 0): ("n.png", None), (1, 0): ("a.png", solid_tile(RED))}, data={(0, 0): va.encode_tile_data(pixels)}
        )
        surface = self.draw(application)

        def marker_present(coordinate):
            rectangle = application.atlas_panel.tile_screen_rect(coordinate)
            radius = max(3, rectangle.width // 12)
            return tuple(surface.get_at((rectangle.right - radius - 2, rectangle.y + radius + 2)))[:3] == va.Theme.UNSAVED

        self.assertTrue(marker_present((0, 0)))
        self.assertFalse(marker_present((1, 0)))

    def test_error_tiles_show_the_error_color_and_no_pixels(self):
        application, _ = self.make_application({(0, 0): ("gone.png", None)})
        surface = self.draw(application)
        rectangle = application.atlas_panel.tile_screen_rect((0, 0))
        self.assertEqual(tuple(surface.get_at((rectangle.x + 1, rectangle.y + 1)))[:3], va.Theme.ERROR)

    def test_unplaced_tiles_are_drawn_in_their_section(self):
        application, _ = self.make_application({}, [("u.png", solid_tile(BLUE))], write_atlas=False)
        application.project.set_tile_size(4)
        application.atlas_panel.set_tile_size(4)
        application.unplaced_panel.set_tile_size(4)
        surface = self.draw(application)
        rectangle = application.unplaced_panel.tile_screen_rect((0, 0))
        self.assertEqual(tuple(surface.get_at(rectangle.center))[:3], BLUE[:3])

    def test_the_empty_state_offers_the_tile_size_while_it_is_not_saved_and_not_after(self):
        application, _ = self.make_application(write_atlas=False)
        self.assertIn("field", application.empty_state_rectangles())
        self.draw(application)
        saved_application, _ = self.make_application({}, write_atlas=True)
        self.assertNotIn("field", saved_application.empty_state_rectangles())
        self.draw(saved_application)

    def test_create_button_writes_atlas_json_with_the_typed_tile_size(self):
        application, project = self.make_application(write_atlas=False)
        rectangles = application.empty_state_rectangles()
        self.press(application, rectangles["field"].center)
        application.tile_size_field.select_all()
        application.handle_event(va.pygame.event.Event(va.pygame.TEXTINPUT, text="8"))
        self.press(application, rectangles["create"].center)
        self.assertEqual(self.saved().tile_size, 8)
        self.assertEqual(project.tile_size, 8)
        self.assertEqual(application.atlas_panel.tile_size, 8)

    def test_a_bad_tile_size_is_flagged_and_nothing_is_created(self):
        application, project = self.make_application(write_atlas=False)
        rectangles = application.empty_state_rectangles()
        self.press(application, rectangles["field"].center)
        application.tile_size_field.set_text("")
        self.press(application, rectangles["create"].center)
        self.assertTrue(application.tile_size_field_invalid)
        self.assertFalse((self.folder / "atlas.json").exists())
        self.draw(application)

    def test_a_failed_save_at_the_end_of_a_drag_shows_a_message_and_changes_nothing(self):
        application, project = self.make_application(
            {(0, 0): ("a.png", solid_tile(RED)), (1, 0): ("b.png", solid_tile(GREEN))}
        )
        start = application.atlas_panel.tile_screen_rect((0, 0)).center
        end = application.atlas_panel.tile_screen_rect((1, 0)).center
        self.press(application, start)
        self.move(application, end)
        self.draw(application)  # drag feedback must draw without error
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            self.release(application, end)
        self.assertIn("Could not save", application.message)
        self.assertEqual(project.tiles[(0, 0)].name, "a.png")
        self.draw(application)

    def test_a_complete_drag_through_events(self):
        application, project = self.make_application({(0, 0): ("a.png", solid_tile(RED))}, [("u.png", solid_tile(BLUE))])
        start = application.unplaced_panel.tile_screen_rect((0, 0)).center
        end = application.atlas_panel.tile_screen_rect((3, 2)).center
        self.press(application, start)
        self.move(application, end)
        self.draw(application)
        self.release(application, end)
        self.assertEqual(project.tiles[(3, 2)].name, "u.png")

    def test_wheel_scrolls_and_control_wheel_zooms_the_panel_under_the_pointer(self):
        application, _ = self.make_application({(30, 30): ("a.png", solid_tile(RED))})
        application.pointer = application.atlas_panel.rect.center
        application.handle_event(va.pygame.event.Event(va.pygame.MOUSEWHEEL, x=0, y=-1))
        self.assertGreater(application.atlas_panel.scroll_y, 0)
        zoom = application.atlas_panel.zoom
        with mock.patch.object(va.pygame.key, "get_mods", return_value=va.pygame.KMOD_CTRL):
            application.handle_event(va.pygame.event.Event(va.pygame.MOUSEWHEEL, x=0, y=1))
        self.assertEqual(application.atlas_panel.zoom, zoom + 1)

    def test_tooltips_name_the_tile_under_the_pointer(self):
        application, _ = self.make_application(
            {(0, 0): ("a.png", solid_tile(RED)), (1, 0): ("gone.png", None)}, [("u.png", solid_tile(RED))]
        )
        application.pointer = application.atlas_panel.tile_screen_rect((0, 0)).center
        self.assertEqual(application.hover_text(), "a.png")
        application.pointer = application.atlas_panel.tile_screen_rect((1, 0)).center
        self.assertIn("is missing", application.hover_text())
        application.pointer = application.unplaced_panel.tile_screen_rect((0, 0)).center
        self.assertEqual(application.hover_text(), "u.png")
        self.draw(application)

    def test_the_right_half_shows_the_selection(self):
        application, project = self.make_application({(0, 0): ("a.png", solid_tile(RED)), (1, 0): ("b.png", solid_tile(GREEN))})
        project.select_only((0, 0))
        self.draw(application)
        project.selection = {(0, 0), (1, 0)}
        self.draw(application)
        project.selection = set()
        self.draw(application)


if __name__ == "__main__":
    unittest.main()
