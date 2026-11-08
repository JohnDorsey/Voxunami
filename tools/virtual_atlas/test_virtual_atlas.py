"""Tests for the logic of VirtualAtlas.py (everything that needs no window).

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import base64
import importlib.util
import io
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


class ToolSettingsTests(unittest.TestCase):
    def test_plain_brackets_change_both_axes_of_a_brush_and_never_go_below_one(self):
        settings = va.ToolSettings()
        settings.adjust("pencil", 1, True, True, (16, 16))
        self.assertEqual(settings.brush_sizes["pencil"], (2, 2))
        settings.adjust("pencil", -1, True, True, (16, 16))
        settings.adjust("pencil", -1, True, True, (16, 16))
        self.assertEqual(settings.brush_sizes["pencil"], (1, 1))

    def test_shift_changes_only_the_width_and_alt_only_the_height(self):
        settings = va.ToolSettings()
        settings.adjust("pencil", 1, True, False, (16, 16))
        settings.adjust("pencil", 1, True, False, (16, 16))
        settings.adjust("pencil", 1, False, True, (16, 16))
        self.assertEqual(settings.brush_sizes["pencil"], (3, 2))
        settings.adjust("pencil", -1, True, False, (16, 16))
        self.assertEqual(settings.brush_sizes["pencil"], (2, 2))

    def test_every_size_is_reachable_odd_or_even(self):
        settings = va.ToolSettings()
        seen = set()
        for _ in range(10):
            settings.adjust("eraser", 1, True, True, (16, 16))
            seen.add(settings.brush_sizes["eraser"][0])
        self.assertEqual(seen, set(range(2, 12)))

    def test_a_brush_is_limited_to_the_canvas_on_each_axis(self):
        settings = va.ToolSettings()
        for _ in range(100):
            settings.adjust("pencil", 1, True, True, (8, 32))
        self.assertEqual(settings.brush_sizes["pencil"], (32, 8))

    def test_pencil_and_eraser_have_separate_sizes(self):
        settings = va.ToolSettings()
        settings.adjust("pencil", 3, True, True, (16, 16))
        self.assertEqual(settings.brush_sizes["eraser"], (1, 1))

    def test_all_line_tools_share_one_width_changed_by_plain_brackets_only(self):
        settings = va.ToolSettings()
        settings.adjust("line", 2, True, True, (16, 16))
        self.assertEqual(settings.line_width, 3)
        settings.adjust("horizontal_parallelogram", 1, True, True, (16, 16))
        self.assertEqual(settings.line_width, 4)
        settings.adjust("vertical_line", 1, True, False, (16, 16))
        settings.adjust("vertical_line", 1, False, True, (16, 16))
        self.assertEqual(settings.line_width, 4)
        settings.adjust("line", -10, True, True, (16, 16))
        self.assertEqual(settings.line_width, 1)

    def test_tools_without_a_size_ignore_the_keys(self):
        settings = va.ToolSettings()
        settings.adjust("fill", 1, True, True, (16, 16))
        settings.adjust("eyedropper", 1, True, True, (16, 16))
        self.assertEqual((settings.brush_sizes, settings.line_width), ({"pencil": (1, 1), "eraser": (1, 1)}, 1))

    def test_footprint_sizes_for_the_outline(self):
        settings = va.ToolSettings()
        settings.brush_sizes["pencil"] = (3, 2)
        settings.line_width = 5
        self.assertEqual(settings.footprint_size("pencil"), (3, 2))
        self.assertEqual(settings.footprint_size("line"), (5, 5))
        self.assertEqual(settings.footprint_size("horizontal_parallelogram"), (5, 1))
        self.assertEqual(settings.footprint_size("vertical_parallelogram"), (1, 5))
        self.assertEqual(settings.footprint_size("fill"), (1, 1))


class CanvasSessionTests(unittest.TestCase):
    def layout(self, size=8, tiles=None):
        tiles = tiles or {(0, 0): solid_tile(BLUE, size)}
        return va.compose_canvas(tiles, size)

    def start(self, layout, tool, pixel, color=RED, settings=None, **keywords):
        return va.CanvasSession(layout, tool, settings or va.ToolSettings(), color, pixel, **keywords)

    def painted(self, session):
        return {(int(c), int(r)) for r, c in numpy.argwhere(numpy.any(session.pixels != session.base, axis=2))}

    def test_pencil_writes_the_exact_color_alpha_included_and_leaves_the_layout_alone(self):
        layout = self.layout()
        session = self.start(layout, "pencil", (3, 3), (10, 20, 30, 77))
        self.assertEqual(tuple(session.pixels[3, 3]), (10, 20, 30, 77))
        self.assertTrue(numpy.all(layout.pixels == BLUE))
        self.assertTrue(session.changed)

    def test_pencil_uses_its_size_with_the_even_size_rule(self):
        settings = va.ToolSettings()
        settings.brush_sizes["pencil"] = (2, 3)
        session = self.start(self.layout(), "pencil", (4, 4), settings=settings)
        self.assertEqual(self.painted(session), {(3, 3), (4, 3), (3, 4), (4, 4), (3, 5), (4, 5)})

    def test_dragging_paints_every_pixel_between(self):
        session = self.start(self.layout(), "pencil", (0, 0))
        session.move((5, 0))
        self.assertEqual(self.painted(session), {(column, 0) for column in range(6)})
        session.move((5, 3))
        self.assertEqual(len(self.painted(session)), 9)

    def test_a_fast_diagonal_jump_leaves_no_gaps(self):
        session = self.start(self.layout(), "pencil", (0, 0))
        session.move((7, 7))
        self.assertEqual(self.painted(session), {(i, i) for i in range(8)})

    def test_the_eraser_makes_pixels_fully_transparent_whatever_the_color(self):
        session = self.start(self.layout(), "eraser", (2, 2), color=RED)
        self.assertEqual(tuple(session.pixels[2, 2]), va.TRANSPARENT)
        session.move((4, 2))
        self.assertEqual(int((session.pixels[2, :, 3] == 0).sum()), 3)

    def test_nothing_is_drawn_over_empty_places_between_tiles(self):
        layout = self.layout(4, {(0, 0): solid_tile(BLUE), (2, 0): solid_tile(BLUE)})
        session = self.start(layout, "pencil", (0, 1))
        session.move((11, 1))
        self.assertTrue(numpy.all(session.pixels[1, 4:8] == 0), "the empty place stays empty")
        self.assertTrue(numpy.all(session.pixels[1, 0:4] == RED))
        self.assertTrue(numpy.all(session.pixels[1, 8:12] == RED))

    def test_a_stroke_may_start_and_run_outside_the_canvas(self):
        session = self.start(self.layout(), "pencil", (-5, -5))
        session.move((3, 3))
        self.assertTrue(session.changed)
        outside = self.start(self.layout(), "pencil", (-5, -5))
        outside.move((-2, -9))
        self.assertFalse(outside.changed)

    def test_a_line_is_a_preview_that_follows_the_pointer_and_leaves_no_trace_of_earlier_positions(self):
        session = self.start(self.layout(), "line", (0, 0))
        session.move((7, 7))
        self.assertEqual(self.painted(session), {(i, i) for i in range(8)})
        session.move((7, 0))
        self.assertEqual(self.painted(session), {(column, 0) for column in range(8)})
        session.move((0, 0))
        self.assertEqual(self.painted(session), {(0, 0)})

    def test_a_line_uses_the_shared_line_width(self):
        settings = va.ToolSettings()
        settings.line_width = 3
        session = self.start(self.layout(), "line", (1, 4), settings=settings)
        session.move((6, 4))
        self.assertEqual(self.painted(session), {(c, r) for c in range(1, 7) for r in range(3, 6)})

    def test_horizontal_and_vertical_lines_lock_to_their_axis(self):
        horizontal = self.start(self.layout(), "horizontal_line", (1, 2))
        horizontal.move((6, 7))
        self.assertEqual(self.painted(horizontal), {(column, 2) for column in range(1, 7)})
        vertical = self.start(self.layout(), "vertical_line", (2, 1))
        vertical.move((7, 6))
        self.assertEqual(self.painted(vertical), {(2, row) for row in range(1, 7)})

    def test_parallelogram_tools_draw_their_shapes(self):
        settings = va.ToolSettings()
        settings.line_width = 3
        horizontal = self.start(self.layout(), "horizontal_parallelogram", (2, 0), settings=settings)
        horizontal.move((4, 2))
        self.assertEqual(self.painted(horizontal), {(1, 0), (2, 0), (3, 0), (2, 1), (3, 1), (4, 1), (3, 2), (4, 2), (5, 2)})
        vertical = self.start(self.layout(), "vertical_parallelogram", (0, 2), settings=settings)
        vertical.move((2, 4))
        self.assertEqual(self.painted(vertical), {(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (1, 4), (2, 3), (2, 4), (2, 5)})

    def test_fill_paints_the_clicked_region_on_press_and_ignores_movement(self):
        tile = solid_tile(BLUE, 8)
        tile[:, 4] = GREEN
        session = self.start(self.layout(8, {(0, 0): tile}), "fill", (0, 0))
        self.assertEqual(int(numpy.all(session.pixels == RED, axis=2).sum()), 32)
        session.move((7, 7))
        self.assertEqual(int(numpy.all(session.pixels == RED, axis=2).sum()), 32)

    def test_fill_with_the_color_already_there_changes_nothing(self):
        session = self.start(self.layout(), "fill", (0, 0), color=BLUE)
        self.assertFalse(session.changed)

    def test_fill_does_not_leak_through_an_empty_place_into_a_tile_on_the_other_side(self):
        # The empty place holds transparent pixels, so a fill of a transparent tile would run through it.
        layout = self.layout(4, {(0, 0): solid_tile(TRANSPARENT), (2, 0): solid_tile(TRANSPARENT)})
        session = self.start(layout, "fill", (0, 0))
        self.assertTrue(numpy.all(session.pixels[:, 0:4] == RED))
        self.assertTrue(numpy.all(session.pixels[:, 8:12] == 0), "the tile across the gap is untouched")

    def test_fill_does_not_cross_into_an_empty_place_and_does_nothing_outside(self):
        layout = self.layout(4, {(0, 0): solid_tile(BLUE), (2, 0): solid_tile(BLUE)})
        session = self.start(layout, "fill", (0, 0))
        self.assertEqual(int(numpy.all(session.pixels == RED, axis=2).sum()), 16)
        self.assertFalse(self.start(layout, "fill", (5, 1)).changed, "an empty place")
        self.assertFalse(self.start(layout, "fill", (-1, 1)).changed, "outside")

    def test_tools_that_do_not_draw_are_refused(self):
        with self.assertRaises(ValueError):
            self.start(self.layout(), "eyedropper", (0, 0))

    def test_pick_color(self):
        layout = self.layout(4, {(0, 0): random_tile(1), (2, 0): random_tile(2)})
        self.assertEqual(va.pick_color(layout, (1, 2)), tuple(int(v) for v in layout.pixels[2, 1]))
        self.assertIsNone(va.pick_color(layout, (5, 1)), "an empty place")
        self.assertIsNone(va.pick_color(layout, (-1, 0)))
        self.assertIsNone(va.pick_color(layout, (0, 99)))
        self.assertEqual(va.pick_color(layout, (9, 0)), tuple(int(v) for v in layout.pixels[0, 9]))

    # Wrap-assist: the canvas repeats in every direction and a stroke wraps around it. --------------------

    def wrapped(self, layout, tool, pixel, **keywords):
        return self.start(layout, tool, pixel, wrap=True, **keywords)

    def stroke_on_a_much_bigger_canvas(self, tool, points, settings):
        """Where a stroke lands on a canvas eight times as big as the 4 x 4 one, away from its edges, folded back
        by plain modulo: an independent way to find what a wrapped stroke should paint."""
        shift = 12
        big = va.compose_canvas({(0, 0): solid_tile(BLUE, 32)}, 32)
        session = self.start(big, tool, (points[0][0] + shift, points[0][1] + shift), settings=settings)
        for column, row in points[1:]:
            session.move((column + shift, row + shift))
        return {((column - shift) % 4, (row - shift) % 4) for column, row in self.painted(session)}

    def test_without_wrap_a_pixel_outside_the_canvas_paints_nothing(self):
        session = self.start(self.layout(4), "pencil", (5, 6))
        self.assertEqual(self.painted(session), set())

    def test_with_wrap_a_pixel_in_any_copy_of_the_canvas_paints_the_same_pixel_of_it(self):
        for pixel in [(1, 2), (5, 6), (-3, -2), (9, 14), (-7, 10)]:
            self.assertEqual(self.painted(self.wrapped(self.layout(4), "pencil", pixel)), {(1, 2)}, pixel)

    def test_a_stroke_leaving_one_side_comes_back_in_on_the_other(self):
        session = self.wrapped(self.layout(4), "pencil", (3, 1))
        session.move((5, 1))
        self.assertEqual(self.painted(session), {(3, 1), (0, 1), (1, 1)})
        session = self.wrapped(self.layout(4), "pencil", (1, 2))
        session.move((1, -1))
        self.assertEqual(self.painted(session), {(1, 2), (1, 1), (1, 0), (1, 3)})

    def test_a_diagonal_stroke_through_a_corner_has_no_gaps(self):
        session = self.wrapped(self.layout(4), "pencil", (2, 2))
        session.move((6, 6))
        self.assertEqual(self.painted(session), {(2, 2), (3, 3), (0, 0), (1, 1)})

    def test_a_brush_on_a_corner_paints_all_four_corners(self):
        settings = va.ToolSettings()
        settings.brush_sizes["pencil"] = (2, 2)
        session = self.wrapped(self.layout(4), "pencil", (0, 0), settings=settings)
        self.assertEqual(self.painted(session), {(3, 3), (0, 3), (3, 0), (0, 0)})

    def test_a_brush_wider_than_the_canvas_covers_the_whole_row(self):
        settings = va.ToolSettings()
        settings.brush_sizes["pencil"] = (6, 1)
        session = self.wrapped(self.layout(4), "pencil", (1, 1), settings=settings)
        self.assertEqual(self.painted(session), {(column, 1) for column in range(4)})

    def test_the_eraser_wraps_too(self):
        session = self.wrapped(self.layout(4), "eraser", (3, 0))
        session.move((4, 0))
        self.assertEqual(tuple(session.pixels[0, 0]), va.TRANSPARENT)
        self.assertEqual(tuple(session.pixels[0, 3]), va.TRANSPARENT)
        self.assertEqual(self.painted(session), {(3, 0), (0, 0)})

    def test_a_line_leaving_the_canvas_wraps(self):
        session = self.wrapped(self.layout(4), "line", (2, 1))
        session.move((6, 1))
        self.assertEqual(self.painted(session), {(column, 1) for column in range(4)})
        session = self.wrapped(self.layout(4), "line", (3, 3))
        session.move((5, 5))
        self.assertEqual(self.painted(session), {(3, 3), (0, 0), (1, 1)})

    def test_every_stroke_tool_paints_what_the_same_stroke_paints_on_a_much_bigger_canvas_folded_back(self):
        strokes = [
            [(1, 1), (6, 2)],
            [(-5, -3), (9, 6)],
            [(3, 3), (3, -9)],
            [(0, 0), (-1, 0), (2, 5), (9, -4)],
            [(7, 2), (-6, 2)],
        ]
        for tool in va.BRUSH_TOOLS + va.LINE_TOOLS:
            for width in (1, 2, 3):
                settings = va.ToolSettings()
                settings.line_width = width
                settings.brush_sizes["pencil"] = settings.brush_sizes["eraser"] = (width, 4 - width)
                for points in strokes:
                    session = self.wrapped(self.layout(4), tool, points[0], settings=settings)
                    for point in points[1:]:
                        session.move(point)
                    expected = self.stroke_on_a_much_bigger_canvas(tool, points, settings)
                    self.assertEqual(self.painted(session), expected, f"{tool} width {width} {points}")

    def test_moving_a_whole_stroke_by_whole_canvases_changes_nothing_for_any_tool(self):
        for tool in ("pencil", "line", "horizontal_parallelogram", "noise_brush", "airbrush"):
            results = []
            for shift_column, shift_row in [(0, 0), (4, 0), (-8, 12), (20, -4)]:
                session = self.wrapped(
                    self.layout(4), tool, (1 + shift_column, 2 + shift_row), random_source=random.Random(5)
                )
                session.move((6 + shift_column, 4 + shift_row))
                session.move((-2 + shift_column, 1 + shift_row))
                results.append(session.pixels.copy())
            for other in results[1:]:
                self.assertTrue(numpy.array_equal(results[0], other), tool)

    def test_pixels_of_empty_places_are_never_painted_in_any_copy(self):
        layout = self.layout(4, {(0, 0): solid_tile(BLUE), (2, 0): solid_tile(BLUE)})
        session = self.wrapped(layout, "pencil", (3, 1))
        session.move((4, 1))
        self.assertEqual(self.painted(session), {(3, 1)}, "column 4 is an empty place")
        session = self.wrapped(layout, "pencil", (11, 1))
        session.move((13, 1))
        self.assertEqual(self.painted(session), {(11, 1), (0, 1), (1, 1)}, "12 is column 0 of the next copy")
        session = self.wrapped(layout, "pencil", (17, 1))
        self.assertFalse(session.changed, "column 17 is column 5 of the canvas, an empty place")

    def test_the_fill_bucket_fills_around_the_edges(self):
        pixels = solid_tile(TRANSPARENT, 4)
        pixels[:, 0] = RED
        pixels[:, 3] = RED
        layout = va.compose_canvas({(0, 0): pixels}, 4)
        plain = self.start(layout, "fill", (0, 0), color=GREEN)
        self.assertEqual(self.painted(plain), {(0, row) for row in range(4)})
        around = self.wrapped(layout, "fill", (0, 0), color=GREEN)
        self.assertEqual(self.painted(around), {(column, row) for column in (0, 3) for row in range(4)})
        elsewhere = self.wrapped(layout, "fill", (16, -4), color=GREEN)
        self.assertTrue(numpy.array_equal(elsewhere.pixels, around.pixels), "a press in any copy fills the same region")
        from_the_other_side = self.wrapped(layout, "fill", (3, 3), color=GREEN)
        self.assertTrue(numpy.array_equal(from_the_other_side.pixels, around.pixels))

    def test_the_noise_brush_wraps(self):
        settings = va.ToolSettings()
        settings.diameters["noise_brush"] = 3
        settings.fractions["noise_density"] = 1.0
        session = self.wrapped(self.layout(5), "noise_brush", (0, 0), settings=settings)
        self.assertEqual(self.painted(session), {(0, 0), (1, 0), (4, 0), (0, 1), (0, 4)})

    def test_the_air_brush_wraps(self):
        settings = va.ToolSettings()
        settings.diameters["airbrush"] = 3
        settings.fractions["airbrush_hardness"] = 1.0
        settings.fractions["airbrush_opacity"] = 1.0
        session = self.wrapped(self.layout(4), "airbrush", (0, 0), settings=settings)
        self.assertEqual(self.painted(session), {(0, 0), (1, 0), (3, 0), (0, 1), (0, 3)})

    def test_pick_color_with_wrap_reads_the_pixel_in_any_copy(self):
        layout = self.layout(4, {(0, 0): random_tile(1), (2, 0): random_tile(2)})
        self.assertEqual(va.pick_color(layout, (1 + 12, 2 - 4), wrap=True), va.pick_color(layout, (1, 2)))
        self.assertEqual(va.pick_color(layout, (-3, 9), wrap=True), va.pick_color(layout, (9, 1)))
        self.assertIsNone(va.pick_color(layout, (5 + 12, 1), wrap=True), "an empty place in another copy")
        self.assertIsNone(va.pick_color(layout, (13, 1)), "without wrap the other copies do not exist")


class WrapLogicTests(unittest.TestCase):
    def test_wrap_pixel_puts_any_pixel_into_the_canvas(self):
        self.assertEqual(va.wrap_pixel((-1, -1), (4, 6)), (5, 3))
        self.assertEqual(va.wrap_pixel((6, 4), (4, 6)), (0, 0))
        self.assertEqual(va.wrap_pixel((13, 9), (4, 6)), (1, 1))
        self.assertEqual(va.wrap_pixel((2, 3), (4, 6)), (2, 3))

    def test_fold_mask_moves_a_window_into_the_canvas_by_whole_canvases(self):
        window = numpy.zeros((3, 3), dtype=bool)
        window[0, 0] = True
        for left, top, expected in [(3, 3, (3, 3)), (4, 4, (0, 0)), (-1, -1, (3, 3)), (-3, -3, (1, 1)), (10, 2, (2, 2))]:
            folded = va.fold_mask(window, left, top, (4, 4))
            self.assertEqual({(int(c), int(r)) for r, c in numpy.argwhere(folded)}, {expected}, (left, top))

    def test_fold_mask_joins_the_parts_of_a_window_that_cross_an_edge(self):
        folded = va.fold_mask(numpy.ones((3, 3), dtype=bool), 3, 0, (4, 4))
        self.assertEqual(mask_to_text(folded), ["##.#", "##.#", "##.#", "...."])

    def test_fold_mask_of_a_window_bigger_than_the_canvas_overlaps_itself(self):
        window = numpy.zeros((2, 9), dtype=bool)
        window[0, 1] = window[1, 8] = True
        self.assertEqual(mask_to_text(va.fold_mask(window, 0, 0, (3, 4))), [".#..", "#...", "...."])

    def test_flood_fill_with_wrap_connects_the_opposite_edges_only_when_asked(self):
        pixels = solid_tile(TRANSPARENT, 4)
        pixels[1, :] = RED
        self.assertEqual(mask_to_text(va.flood_fill_mask(pixels, 0, 0)), ["####", "....", "....", "...."])
        self.assertEqual(mask_to_text(va.flood_fill_mask(pixels, 0, 0, wrap=True)), ["####", "....", "####", "####"])

    def test_flood_fill_with_wrap_crosses_the_far_edges_too_not_only_the_near_ones(self):
        rows = solid_tile(TRANSPARENT, 4)
        rows[1, :] = RED
        self.assertEqual(mask_to_text(va.flood_fill_mask(rows, 0, 2, wrap=True)), ["####", "....", "####", "####"])
        self.assertEqual(mask_to_text(va.flood_fill_mask(rows, 3, 3, wrap=True)), ["####", "....", "####", "####"])
        columns = solid_tile(TRANSPARENT, 4)
        columns[:, 1] = RED
        self.assertEqual(mask_to_text(va.flood_fill_mask(columns, 2, 0, wrap=True)), ["#.##"] * 4)
        self.assertEqual(mask_to_text(va.flood_fill_mask(columns, 3, 3, wrap=True)), ["#.##"] * 4)

    def test_flood_fill_does_not_leak_diagonally(self):
        pixels = solid_tile(TRANSPARENT, 3)
        pixels[0, 1] = pixels[1, 0] = pixels[1, 2] = pixels[2, 1] = RED
        around = va.flood_fill_mask(pixels, 0, 0, wrap=True)
        self.assertEqual(mask_to_text(around), ["#.#", "...", "#.#"], "the middle only touches the corners at their corners")

    def test_the_noise_stamp_with_wrap_reaches_the_other_side(self):
        pixels = solid_tile(TRANSPARENT, 5)
        va.noise_stamp(pixels, 0, 0, RED, 3, "circle", 1.0, 1.0, random.Random(1), wrap=True)
        painted = {(int(c), int(r)) for r, c in numpy.argwhere(pixels[..., 3] > 0)}
        self.assertEqual(painted, {(0, 0), (1, 0), (4, 0), (0, 1), (0, 4)})

    def test_the_air_brush_with_wrap_covers_the_other_side_as_strongly_as_this_one(self):
        stroke = va.AirbrushStroke(solid_tile(TRANSPARENT, 4), RED, 3, 0.5, 1.0, wrap=True)
        stroke.stamp(0, 0)
        self.assertGreater(stroke.coverage[0, 3], 0.0)
        self.assertEqual(stroke.coverage[0, 3], stroke.coverage[0, 1])
        self.assertEqual(stroke.coverage[3, 0], stroke.coverage[1, 0])


class HexColorTests(unittest.TestCase):
    def test_parsing(self):
        self.assertEqual(va.parse_hex_color("#ff8000"), (255, 128, 0, 255))
        self.assertEqual(va.parse_hex_color("ff800040"), (255, 128, 0, 64))
        self.assertEqual(va.parse_hex_color("#f80"), (255, 136, 0, 255))
        self.assertEqual(va.parse_hex_color("#f808"), (255, 136, 0, 136))
        self.assertEqual(va.parse_hex_color("  #FFFFFF "), (255, 255, 255, 255))

    def test_rejects_what_is_not_a_color(self):
        for text in ["", "#", "#ff", "#fffff", "#ggg000", "#ff80000", "red"]:
            self.assertIsNone(va.parse_hex_color(text), text)

    def test_formatting_round_trips_and_hides_full_alpha(self):
        self.assertEqual(va.format_hex_color((255, 128, 0, 255)), "#ff8000")
        self.assertEqual(va.format_hex_color((255, 128, 0, 64)), "#ff800040")
        for color in [(0, 0, 0, 0), (1, 2, 3, 4), (255, 255, 255, 255)]:
            self.assertEqual(va.parse_hex_color(va.format_hex_color(color)), color)


class ColorStateTests(unittest.TestCase):
    def test_starts_with_black_and_white_and_edits_the_primary(self):
        state = va.ColorState()
        self.assertEqual((state.get("primary"), state.get("secondary"), state.active_slot), ((0, 0, 0, 255), (255, 255, 255, 255), "primary"))

    def test_set_color_affects_only_that_slot(self):
        state = va.ColorState()
        state.set_color((10, 20, 30, 40), "secondary")
        self.assertEqual(state.get("secondary"), (10, 20, 30, 40))
        self.assertEqual(state.get("primary"), (0, 0, 0, 255))
        state.set_color((1, 2, 3, 4))
        self.assertEqual(state.get(), (1, 2, 3, 4))

    def test_hsv_from_rgb(self):
        state = va.ColorState()
        state.set_color((255, 0, 0, 255))
        self.assertEqual(state.get_hsva()[:3], (0.0, 1.0, 1.0))
        state.set_color((0, 255, 0, 128))
        hue, saturation, value, alpha = state.get_hsva()
        self.assertAlmostEqual(hue, 1 / 3)
        self.assertAlmostEqual(alpha, 128 / 255)

    def test_a_grey_keeps_the_hue_it_had_and_black_keeps_hue_and_saturation(self):
        state = va.ColorState()
        state.set_hsva(0.6, 0.8, 0.9, 1.0)
        state.set_color((128, 128, 128, 255))
        self.assertAlmostEqual(state.get_hsva()[0], 0.6)
        state.set_hsva(0.3, 0.7, 0.5, 1.0)
        state.set_color((0, 0, 0, 255))
        hue, saturation, value, _ = state.get_hsva()
        self.assertEqual((round(hue, 3), round(saturation, 3), value), (0.3, 0.7, 0.0))

    def test_dragging_to_black_in_the_picker_does_not_lose_hue_or_saturation(self):
        state = va.ColorState()
        state.set_hsva(0.5, 0.6, 0.0, 1.0)
        self.assertEqual(state.get(), (0, 0, 0, 255))
        self.assertEqual(state.get_hsva()[:3], (0.5, 0.6, 0.0))

    def test_set_hsva_makes_rgb_and_clamps(self):
        state = va.ColorState()
        state.set_hsva(0.0, 1.0, 1.0, 0.5)
        self.assertEqual(state.get(), (255, 0, 0, 128))
        state.set_hsva(5.0, -3.0, 9.0, 2.0)
        self.assertEqual(state.get(), (255, 255, 255, 255))

    def test_history_puts_the_newest_first_without_repeats_and_is_capped(self):
        state = va.ColorState()
        state.remember((1, 1, 1, 255))
        state.remember((2, 2, 2, 255))
        state.remember((1, 1, 1, 255))
        self.assertEqual(state.history, [(1, 1, 1, 255), (2, 2, 2, 255)])
        for value in range(40):
            state.remember((value, 0, 0, 255))
        self.assertEqual(len(state.history), va.COLOR_HISTORY_LENGTH)
        self.assertEqual(state.history[0], (39, 0, 0, 255))


class CanvasViewTests(unittest.TestCase):
    def setUp(self):
        self.view = va.CanvasView()
        self.view.rect = va.pygame.Rect(100, 50, 400, 300)

    def test_fit_uses_the_largest_whole_zoom_and_centers(self):
        self.view.fit(16, 16)
        self.assertEqual(self.view.zoom, 18)  # 300 // 16
        self.assertEqual(self.view.pixel_rectangle(0, 0, 16, 16).center, (300, 200))

    def test_fit_never_goes_below_one_for_a_huge_canvas(self):
        self.view.fit(1000, 1000)
        self.assertEqual(self.view.zoom, 1)

    def test_pixel_at_and_pixel_rectangle_agree(self):
        self.view.fit(16, 16)
        for column, row in [(0, 0), (5, 9), (15, 15)]:
            rectangle = self.view.pixel_rectangle(column, row)
            self.assertEqual(self.view.pixel_at(rectangle.topleft), (column, row))
            self.assertEqual(self.view.pixel_at((rectangle.right - 1, rectangle.bottom - 1)), (column, row))
        self.assertEqual(self.view.pixel_at((self.view.origin_x - 1, self.view.origin_y)), (-1, 0))

    def test_zoom_keeps_the_pixel_under_the_pointer(self):
        self.view.fit(16, 16)
        anchor = (250, 160)
        before = self.view.pixel_at(anchor)
        self.view.zoom_by(3, anchor)
        self.assertEqual(self.view.zoom, 21)
        self.assertEqual(self.view.pixel_at(anchor), before)
        self.view.zoom_by(-100, anchor)
        self.assertEqual(self.view.zoom, 1)
        self.view.zoom_by(1000, anchor)
        self.assertEqual(self.view.zoom, va.MAXIMUM_CANVAS_ZOOM)

    def test_panning_moves_the_picture(self):
        self.view.fit(16, 16)
        rectangle = self.view.pixel_rectangle(0, 0)
        self.view.pan_by(10, -5)
        self.assertEqual(self.view.pixel_rectangle(0, 0).topleft, (rectangle.x + 10, rectangle.y - 5))

    def test_only_the_visible_pixels_are_listed(self):
        self.view.zoom = 10
        self.view.origin_x, self.view.origin_y = 100, 50
        self.assertEqual(self.view.visible_pixels(100, 100), (0, 0, 40, 30))
        self.view.origin_x = -95
        first_column, _, last_column, _ = self.view.visible_pixels(100, 100)
        self.assertEqual(first_column, 19)
        self.assertEqual(last_column, 60)
        self.assertEqual(self.view.visible_pixels(5, 5)[2:], (5, 5))


class ProjectEditingTests(ProjectFixture):
    def edit(self, project, change, coordinates=None):
        """Select tiles, draw `change(canvas_pixels)` on their canvas and commit it."""
        project.selection = set(coordinates or project.tiles)
        layout = va.compose_canvas({c: project.tiles[c].pixels for c in project.selection}, project.tile_size)
        canvas = layout.pixels.copy()
        change(canvas)
        return project.apply_canvas_edit(layout, canvas)

    def paint(self, row, column, color=RED):
        def change(canvas):
            canvas[row, column] = color

        return change

    def test_an_edit_changes_the_tile_marks_it_modified_and_refreshes_its_picture_revision(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        record = project.tiles[(0, 0)]
        revision = record.revision
        self.assertTrue(self.edit(project, self.paint(1, 2)))
        self.assertEqual(tuple(record.pixels[1, 2]), RED)
        self.assertTrue(record.is_modified)
        self.assertGreater(record.revision, revision)
        self.assertEqual(tuple(record.disk_pixels[1, 2]), BLUE, "the file's pixels are untouched")

    def test_an_edit_that_changes_nothing_is_not_recorded(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        self.assertFalse(self.edit(project, lambda canvas: None))
        self.assertFalse(project.history.can_undo(project.shown_records()))

    def test_undo_and_redo_restore_pixels_and_the_modified_mark_follows_the_file(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        record = project.tiles[(0, 0)]
        self.edit(project, self.paint(0, 0))
        self.edit(project, self.paint(1, 1, GREEN))
        self.assertTrue(project.undo())
        self.assertEqual(tuple(record.pixels[1, 1]), BLUE)
        self.assertEqual(tuple(record.pixels[0, 0]), RED)
        self.assertTrue(project.undo())
        self.assertFalse(record.is_modified, "back to what the file has")
        self.assertFalse(project.undo())
        self.assertTrue(project.redo())
        self.assertTrue(record.is_modified)
        self.assertTrue(project.redo())
        self.assertEqual(tuple(record.pixels[1, 1]), GREEN)
        self.assertFalse(project.redo())

    def test_an_edit_across_two_tiles_is_one_undo_step_and_history_survives_switching_tiles(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE)), (1, 0): ("b.png", solid_tile(BLUE))})

        def across(canvas):
            canvas[0, 3] = RED
            canvas[0, 4] = RED

        self.edit(project, across)
        project.select_only((1, 0))
        self.assertTrue(project.undo())
        self.assertEqual(tuple(project.tiles[(0, 0)].pixels[0, 3]), BLUE)
        self.assertTrue(numpy.all(project.tiles[(0, 0)].pixels == BLUE))
        self.assertTrue(numpy.all(project.tiles[(1, 0)].pixels == BLUE))

    def test_undo_looks_only_at_the_shown_tiles(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE)), (1, 0): ("b.png", solid_tile(BLUE))})
        self.edit(project, self.paint(0, 0), [(0, 0)])
        self.edit(project, self.paint(0, 0), [(1, 0)])
        project.select_only((0, 0))
        project.undo()
        self.assertTrue(numpy.all(project.tiles[(0, 0)].pixels == BLUE))
        self.assertEqual(tuple(project.tiles[(1, 0)].pixels[0, 0]), RED)

    def test_error_tiles_are_not_shown_and_cannot_be_edited(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE)), (1, 0): ("gone.png", None)})
        project.selection = {(0, 0), (1, 0)}
        self.assertEqual([record.name for record in project.shown_records()], ["a.png"])

    def test_new_tiles_are_written_to_atlas_json_on_every_edit_and_every_undo(self):
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(solid_tile(BLUE))})
        self.edit(project, self.paint(2, 2))
        stored = va.decode_tile_data(self.saved().coordinates_to_data[(0, 0)], 4)
        self.assertEqual(tuple(stored[2, 2]), RED)
        project.undo()
        stored = va.decode_tile_data(self.saved().coordinates_to_data[(0, 0)], 4)
        self.assertEqual(tuple(stored[2, 2]), BLUE)
        project.redo()
        self.assertEqual(tuple(va.decode_tile_data(self.saved().coordinates_to_data[(0, 0)], 4)[2, 2]), RED)

    def test_a_failed_save_of_a_new_tile_edit_leaves_the_old_pixels_and_no_history(self):
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(solid_tile(BLUE))})
        record = project.tiles[(0, 0)]
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                self.edit(project, self.paint(0, 0))
        self.assertTrue(numpy.all(record.pixels == BLUE))
        self.assertFalse(project.history.can_undo([record]))

    def test_a_failed_save_during_undo_keeps_the_pixels_and_the_step_still_available(self):
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(solid_tile(BLUE))})
        record = project.tiles[(0, 0)]
        self.edit(project, self.paint(0, 0))
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                project.undo()
        self.assertEqual(tuple(record.pixels[0, 0]), RED)
        self.assertTrue(project.undo(), "the step was not lost")
        self.assertEqual(tuple(record.pixels[0, 0]), BLUE)

    def test_saving_writes_the_png_and_clears_the_modified_mark(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        self.edit(project, self.paint(1, 1))
        self.assertEqual(project.unsaved_tiles(), [(0, 0)])
        project.save_tile((0, 0))
        self.assertFalse(project.tiles[(0, 0)].is_modified)
        self.assertEqual(project.unsaved_tiles(), [])
        self.assertEqual(tuple(va.read_image_pixels(self.folder / "a.png")[1, 1]), RED)

    def test_saving_a_new_tile_creates_its_file_and_removes_its_stored_data(self):
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(solid_tile(GREEN))})
        project.save_tile((0, 0))
        self.assertTrue((self.folder / "n.png").exists())
        self.assertEqual(self.saved().coordinates_to_data, {})
        self.assertEqual(self.saved().coordinates_to_names, {(0, 0): "n.png"})
        self.assertFalse(project.tiles[(0, 0)].is_new)

    def test_saving_a_new_tile_never_overwrites_an_existing_file_even_one_the_program_ignores(self):
        va.write_png_atomically(self.folder / "n.png", solid_tile(RED, 8))
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(solid_tile(GREEN))}, write_atlas=True)
        # the 8x8 file makes the tile an error tile (stored data next to a wrong-size file), so build a clash differently
        self.assertIsNotNone(project.tiles[(0, 0)].error)
        with self.assertRaises(ValueError):
            project.save_tile((0, 0))
        self.assertEqual(va.read_image_size(self.folder / "n.png"), (8, 8))

    def test_a_name_that_appeared_in_the_folder_after_loading_blocks_saving_a_new_tile(self):
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(solid_tile(GREEN))})
        va.write_png_atomically(self.folder / "n.png", solid_tile(RED, 8))
        with self.assertRaises(FileExistsError):
            project.save_tile((0, 0))
        self.assertEqual(va.read_image_size(self.folder / "n.png"), (8, 8))
        self.assertTrue(project.tiles[(0, 0)].is_new)

    def test_save_tiles_skips_clean_and_error_tiles_and_reports_failures(self):
        project = self.make_project(
            {(0, 0): ("a.png", solid_tile(BLUE)), (1, 0): ("b.png", solid_tile(BLUE)), (2, 0): ("gone.png", None)}
        )
        self.edit(project, self.paint(0, 0), [(0, 0)])
        saved, failures = project.save_tiles([(0, 0), (1, 0), (2, 0)])
        self.assertEqual((saved, failures), (1, []))
        self.edit(project, self.paint(1, 1), [(0, 0)])
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            saved, failures = project.save_tiles([(0, 0)])
        self.assertEqual(saved, 0)
        self.assertIn("a.png", failures[0])
        self.assertTrue(project.tiles[(0, 0)].is_modified)

    def test_save_all_saves_every_modified_tile(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE)), (1, 0): ("b.png", solid_tile(BLUE))})
        self.edit(project, self.paint(0, 0), [(0, 0)])
        self.edit(project, self.paint(0, 0), [(1, 0)])
        project.selection = set()
        self.assertEqual(project.save_all_tiles(), (2, []))
        self.assertEqual(project.unsaved_tiles(), [])

    def test_returning_a_tile_forgets_its_history(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        record = project.tiles[(0, 0)]
        self.edit(project, self.paint(0, 0))
        project.save_tile((0, 0))
        project.return_to_unplaced([(0, 0)])
        self.assertFalse(project.history.can_undo([record]))


class EditorLayoutTests(unittest.TestCase):
    def test_every_part_fits_inside_the_editor_and_nothing_overlaps(self):
        for size in [(640, 780), (450, 600), (900, 900)]:
            rect = va.pygame.Rect(10, 10, *size)
            areas = va.compute_editor_areas(rect, list(va.TOOL_LABELS))
            parts = {"header": areas.header, "canvas": areas.canvas, "sidebar": areas.sidebar, "options": areas.options, "controls": areas.controls, "square": areas.square,
                     "hue": areas.hue_bar, "alpha": areas.alpha_bar, "primary": areas.primary_swatch,
                     "secondary": areas.secondary_swatch, "hex": areas.hex_field}
            parts.update({f"tool {tool}": rectangle for tool, rectangle in areas.tool_buttons.items()})
            parts.update({f"history {index}": rectangle for index, rectangle in enumerate(areas.history)})
            for name, rectangle in parts.items():
                self.assertTrue(rect.contains(rectangle), f"{name} {rectangle} outside {rect}")
            names = list(parts)
            for index, first in enumerate(names):
                for second in names[index + 1:]:
                    self.assertFalse(parts[first].colliderect(parts[second]), f"{first} overlaps {second} at {size}")
            self.assertGreater(areas.canvas.height, 100, size)

    def test_the_sidebar_is_a_thin_strip_right_of_the_canvas_and_holds_its_buttons(self):
        for size in [(640, 780), (450, 600), (900, 900)]:
            areas = va.compute_editor_areas(va.pygame.Rect(10, 10, *size), list(va.TOOL_LABELS))
            self.assertGreaterEqual(areas.sidebar.left, areas.canvas.right, size)
            self.assertEqual((areas.sidebar.top, areas.sidebar.bottom), (areas.canvas.top, areas.canvas.bottom))
            self.assertLess(areas.sidebar.width, areas.canvas.width // 4, "thin")
            self.assertEqual(list(areas.sidebar_buttons), ["wrap"])
            for name, rectangle in areas.sidebar_buttons.items():
                self.assertTrue(areas.sidebar.contains(rectangle), name)
                self.assertGreaterEqual(min(rectangle.size), va.scaled(24), "big enough to click")

    def test_all_sixteen_recent_colors_have_room_when_the_editor_is_wide_enough(self):
        areas = va.compute_editor_areas(va.pygame.Rect(0, 0, 640, 780), list(va.TOOL_LABELS))
        self.assertEqual(len(areas.history), va.COLOR_HISTORY_LENGTH)


class CyclingRandom:
    """Returns the given numbers over and over, one per call."""

    def __init__(self, *numbers):
        self.numbers = list(numbers)
        self.calls = 0

    def random(self):
        self.calls += 1
        return self.numbers[(self.calls - 1) % len(self.numbers)]


class FakeImageClipboard:
    """Stands in for the system clipboard, so the tests never touch the real one."""

    def __init__(self, image=None, can_write=True):
        self.image = image
        self.can_write = can_write
        self.writes = []

    def get_image(self):
        return None if self.image is None else self.image.copy()

    def set_image(self, pixels):
        if not self.can_write:
            return False
        self.image = pixels.copy()
        self.writes.append(pixels.copy())
        return True


class AdvancedToolSettingsTests(unittest.TestCase):
    def test_the_noise_brush_and_the_air_brush_start_with_these_settings(self):
        settings = va.ToolSettings()
        self.assertEqual(settings.diameters, {"noise_brush": 5, "airbrush": 9})
        self.assertEqual(settings.noise_shape, "circle")
        self.assertEqual(
            settings.fractions,
            {"noise_density": 0.5, "noise_opacity": 1.0, "airbrush_hardness": 0.3, "airbrush_opacity": 1.0},
        )

    def test_plain_brackets_change_the_diameter_of_the_current_brush_only(self):
        settings = va.ToolSettings()
        settings.adjust("noise_brush", 2, True, True, (16, 16))
        self.assertEqual(settings.diameters, {"noise_brush": 7, "airbrush": 9})
        settings.adjust("airbrush", -3, True, True, (16, 16))
        self.assertEqual(settings.diameters, {"noise_brush": 7, "airbrush": 6})

    def test_shift_or_alt_alone_do_not_change_a_diameter(self):
        settings = va.ToolSettings()
        settings.adjust("noise_brush", 1, True, False, (16, 16))
        settings.adjust("airbrush", 1, False, True, (16, 16))
        self.assertEqual(settings.diameters, {"noise_brush": 5, "airbrush": 9})

    def test_a_diameter_stays_between_one_and_the_longer_side_of_the_canvas(self):
        settings = va.ToolSettings()
        settings.adjust("noise_brush", -100, True, True, (8, 32))
        self.assertEqual(settings.diameters["noise_brush"], 1)
        settings.adjust("noise_brush", 100, True, True, (8, 32))
        self.assertEqual(settings.diameters["noise_brush"], 32)

    def test_the_outline_of_both_brushes_is_a_square_of_the_diameter(self):
        settings = va.ToolSettings()
        self.assertEqual(settings.footprint_size("noise_brush"), (5, 5))
        self.assertEqual(settings.footprint_size("airbrush"), (9, 9))

    def test_a_fraction_is_clamped_and_kept_to_whole_percents(self):
        settings = va.ToolSettings()
        settings.set_fraction("noise_density", 1.7)
        settings.set_fraction("noise_opacity", -0.2)
        settings.set_fraction("airbrush_hardness", 0.456)
        self.assertEqual(settings.fractions["noise_density"], 1.0)
        self.assertEqual(settings.fractions["noise_opacity"], 0.0)
        self.assertEqual(settings.fractions["airbrush_hardness"], 0.46)
        with self.assertRaises(KeyError):
            settings.set_fraction("pencil_hardness", 0.5)

    def test_every_slider_names_a_fraction_that_exists(self):
        settings = va.ToolSettings()
        self.assertEqual(set(va.SLIDERS), {"noise_brush", "airbrush"})
        for tool, sliders in va.SLIDERS.items():
            for name, label in sliders:
                self.assertIn(name, settings.fractions, tool)
                self.assertTrue(label)


class SliderTests(unittest.TestCase):
    def test_the_fraction_follows_the_pointer_along_the_track_and_is_clamped(self):
        rectangle = va.pygame.Rect(100, 10, 101, 20)
        self.assertEqual(va.slider_fraction(rectangle, 100), 0.0)
        self.assertEqual(va.slider_fraction(rectangle, 200), 1.0)
        self.assertEqual(va.slider_fraction(rectangle, 150), 0.5)
        self.assertEqual(va.slider_fraction(rectangle, 125), 0.25)
        self.assertEqual(va.slider_fraction(rectangle, 0), 0.0)
        self.assertEqual(va.slider_fraction(rectangle, 5000), 1.0)

    def test_the_fraction_is_rounded_to_a_whole_percent(self):
        rectangle = va.pygame.Rect(0, 0, 1001, 20)
        self.assertEqual(va.slider_fraction(rectangle, 333), 0.33)


class AdvancedSessionTests(unittest.TestCase):
    def layout(self, size=9, tiles=None, color=BLUE):
        tiles = tiles or {(0, 0): solid_tile(color, size)}
        return va.compose_canvas(tiles, size)

    def start(self, layout, tool, pixel, color=RED, settings=None, random_source=None):
        return va.CanvasSession(layout, tool, settings or va.ToolSettings(), color, pixel, random_source)

    def painted(self, session):
        return {(int(c), int(r)) for r, c in numpy.argwhere(numpy.any(session.pixels != session.base, axis=2))}

    def noise_settings(self, diameter=5, shape="circle", density=0.5, opacity=1.0):
        settings = va.ToolSettings()
        settings.diameters["noise_brush"] = diameter
        settings.noise_shape = shape
        settings.fractions["noise_density"] = density
        settings.fractions["noise_opacity"] = opacity
        return settings

    def air_settings(self, diameter=5, hardness=0.5, opacity=1.0):
        settings = va.ToolSettings()
        settings.diameters["airbrush"] = diameter
        settings.fractions["airbrush_hardness"] = hardness
        settings.fractions["airbrush_opacity"] = opacity
        return settings

    def test_the_new_tools_draw(self):
        self.assertIn("noise_brush", va.STROKE_TOOLS)
        self.assertIn("airbrush", va.STROKE_TOOLS)
        self.assertNotIn("select", va.STROKE_TOOLS)
        with self.assertRaises(ValueError):
            self.start(self.layout(), "select", (1, 1))

    def test_a_noise_press_stamps_the_footprint_where_the_rolls_are_true_and_leaves_the_layout_alone(self):
        layout = self.layout()
        settings = self.noise_settings(5, "circle", 0.5)
        session = self.start(layout, "noise_brush", (4, 4), settings=settings, random_source=ScriptedRandom(0.0))
        expected = {(4 + footprint.column_offset, 4 + footprint.row_offset) for footprint in va.brush_footprint(5, "circle")}
        self.assertEqual(self.painted(session), expected)
        self.assertEqual(len(expected), 21)
        self.assertTrue(numpy.all(layout.pixels == BLUE))
        for column, row in expected:
            self.assertEqual(tuple(session.pixels[row, column]), RED)

    def test_density_zero_changes_nothing(self):
        session = self.start(self.layout(), "noise_brush", (4, 4), settings=self.noise_settings(density=0.0), random_source=random.Random(1))
        session.move((8, 8))
        self.assertFalse(session.changed)

    def test_every_pixel_of_movement_is_one_stamp_with_its_own_rolls(self):
        settings = self.noise_settings(1, "square", 0.5)
        source = CyclingRandom(0.1, 0.9)
        session = self.start(self.layout(), "noise_brush", (0, 0), settings=settings, random_source=source)
        session.move((3, 0))
        self.assertEqual(source.calls, 4, "the pressed pixel and each pixel moved over, not more")
        self.assertEqual(self.painted(session), {(0, 0), (2, 0)})

    def test_a_fast_jump_stamps_every_pixel_on_the_way(self):
        settings = self.noise_settings(1, "square", 1.0)
        session = self.start(self.layout(8), "noise_brush", (0, 0), settings=settings, random_source=random.Random(1))
        session.move((7, 7))
        self.assertEqual(self.painted(session), {(i, i) for i in range(8)})

    def test_moving_to_the_same_pixel_stamps_nothing_more(self):
        source = ScriptedRandom(0.0)
        session = self.start(self.layout(), "noise_brush", (4, 4), settings=self.noise_settings(3, "square", 1.0), random_source=source)
        calls = source.calls
        session.move((4, 4))
        self.assertEqual(source.calls, calls)

    def test_hits_on_the_same_pixel_add_up_within_a_stroke_below_full_opacity(self):
        layout = self.layout(8, color=TRANSPARENT)
        settings = self.noise_settings(3, "square", 1.0, 0.5)
        session = self.start(layout, "noise_brush", (2, 2), settings=settings, random_source=random.Random(1))
        session.move((3, 2))
        alphas = [int(session.pixels[2, column, 3]) for column in range(0, 6)]
        # Stamps cover columns 1-3 and 2-4: columns 2 and 3 are hit twice.
        self.assertEqual(alphas, [0, 128, 192, 192, 128, 0])

    def test_the_stroke_matches_stamping_directly(self):
        layout = self.layout()
        settings = self.noise_settings(5, "circle", 0.4, 0.7)
        session = self.start(layout, "noise_brush", (2, 2), settings=settings, random_source=random.Random(7))
        session.move((6, 5))
        reference = layout.pixels.copy()
        source = random.Random(7)
        for column, row in va.line_pixels((2, 2), (6, 5)):
            va.noise_stamp(reference, column, row, RED, 5, "circle", 0.4, 0.7, source, layout.editable_mask)
        self.assertTrue(numpy.array_equal(session.pixels, reference))

    def test_the_square_shape_is_used_when_chosen(self):
        session = self.start(self.layout(), "noise_brush", (4, 4), settings=self.noise_settings(4, "square", 1.0), random_source=ScriptedRandom(0.0))
        self.assertEqual(len(self.painted(session)), 16)

    def test_nothing_is_drawn_over_empty_places_between_tiles(self):
        layout = self.layout(4, {(0, 0): solid_tile(BLUE), (2, 0): solid_tile(BLUE)})
        session = self.start(layout, "noise_brush", (0, 2), settings=self.noise_settings(3, "square", 1.0), random_source=random.Random(1))
        session.move((11, 2))
        self.assertTrue(numpy.all(session.pixels[:, 4:8] == 0))
        self.assertTrue(numpy.any(session.pixels[:, 0:4] == RED))
        self.assertTrue(numpy.any(session.pixels[:, 8:12] == RED))

    def test_without_a_given_random_source_the_noise_brush_still_works(self):
        session = self.start(self.layout(), "noise_brush", (4, 4), settings=self.noise_settings(3, "square", 1.0))
        self.assertEqual(len(self.painted(session)), 9)

    def test_an_air_brush_press_equals_one_stamp_of_the_stroke_it_is_built_on(self):
        layout = self.layout()
        settings = self.air_settings(5, 0.5, 0.8)
        session = self.start(layout, "airbrush", (4, 4), settings=settings)
        stroke = va.AirbrushStroke(layout.pixels, RED, 5, 0.5, 0.8, layout.editable_mask)
        stroke.stamp(4, 4)
        self.assertTrue(numpy.array_equal(session.pixels, stroke.result()))
        self.assertTrue(session.changed)
        self.assertTrue(numpy.all(layout.pixels == BLUE))

    def test_passing_over_the_same_pixels_again_within_a_stroke_does_not_pile_up(self):
        settings = self.air_settings(5, 0.0, 0.5)
        once = self.start(self.layout(), "airbrush", (0, 4), settings=settings)
        once.move((8, 4))
        twice = self.start(self.layout(), "airbrush", (0, 4), settings=settings)
        twice.move((8, 4))
        twice.move((0, 4))
        twice.move((8, 4))
        self.assertTrue(numpy.array_equal(once.pixels, twice.pixels))

    def test_a_separate_stroke_composites_over_the_earlier_one(self):
        settings = self.air_settings(1, 1.0, 0.5)
        first = self.start(self.layout(8, color=TRANSPARENT), "airbrush", (3, 3), settings=settings)
        second = va.compose_canvas({(0, 0): first.pixels}, 8)
        second_session = self.start(second, "airbrush", (3, 3), settings=settings)
        self.assertEqual(int(first.pixels[3, 3, 3]), 128)
        self.assertEqual(int(second_session.pixels[3, 3, 3]), 192)

    def test_an_air_brush_at_zero_opacity_changes_nothing(self):
        session = self.start(self.layout(), "airbrush", (4, 4), settings=self.air_settings(5, 0.5, 0.0))
        self.assertFalse(session.changed)

    def test_the_air_brush_leaves_empty_places_alone_and_may_overhang_the_edge(self):
        layout = self.layout(4, {(0, 0): solid_tile(BLUE), (2, 0): solid_tile(BLUE)})
        session = self.start(layout, "airbrush", (0, 0), settings=self.air_settings(9, 1.0, 1.0))
        session.move((11, 3))
        self.assertTrue(numpy.all(session.pixels[:, 4:8] == 0))
        self.assertTrue(numpy.any(session.pixels[:, 8:12] == RED))


class ImageClipboardTests(unittest.TestCase):
    def test_a_picture_survives_the_clipboard_format_to_the_exact_pixel_alpha_included(self):
        for seed, size in [(1, (1, 1)), (2, (5, 3)), (3, (7, 2)), (4, (16, 16)), (5, (3, 9))]:
            pixels = numpy.random.default_rng(seed).integers(0, 256, size=(*size, 4), dtype=numpy.uint8)
            decoded = va.decode_dib(va.encode_dib(pixels))
            self.assertTrue(numpy.array_equal(decoded, pixels), size)

    def test_an_image_without_alpha_is_read_as_opaque(self):
        picture = Image.new("RGB", (3, 2), (10, 20, 30))
        picture.putpixel((2, 1), (200, 100, 50))
        buffer = io.BytesIO()
        picture.save(buffer, format="BMP")
        device_independent_bitmap = buffer.getvalue()[14:]  # a BMP file is a bitmap with a 14 byte file header
        decoded = va.decode_dib(device_independent_bitmap)
        self.assertEqual(decoded.shape, (2, 3, 4))
        self.assertEqual(tuple(decoded[0, 0]), (10, 20, 30, 255))
        self.assertEqual(tuple(decoded[1, 2]), (200, 100, 50, 255))

    def test_a_bitmap_as_windows_made_it_from_a_dot_net_bitmap_is_read_in_place(self):
        # Copied from the Windows clipboard (CF_DIBV5) after .NET put a 4 x 3 bitmap there: a 124 byte header, twelve
        # more bytes of color masks, then the rows from the bottom one up. Transparent pixels came out light gray.
        data = bytes.fromhex(
            "7c0000000400000003000000010020000300000030000000000000000000000000000000000000000000ff0000ff0000"
            "ff0000000000000042475273000000000000000000000000000000000000000000000000000000000000000000000000"
            "000000000000000000000000040000000000000000000000000000000000ff0000ff0000ff000000d3d3d300ff0000ff"
            "d3d3d30000ffffffd3d3d300d3d3d3001e140affd3d3d3000000ffffd3d3d300d3d3d30000ff00ff"
        )
        self.assertEqual(len(data), 184)
        gray = (211, 211, 211, 255)
        expected = [
            [(255, 0, 0, 255), gray, gray, (0, 255, 0, 255)],
            [gray, gray, (10, 20, 30, 255), gray],
            [gray, (0, 0, 255, 255), gray, (255, 255, 0, 255)],
        ]
        decoded = va.decode_dib(data)
        self.assertEqual([[tuple(int(v) for v in pixel) for pixel in row] for row in decoded], expected)

    def test_masks_after_a_version_5_header_do_not_shift_the_pixels(self):
        pixels = numpy.random.default_rng(3).integers(0, 256, size=(3, 4, 4), dtype=numpy.uint8)
        encoded = va.encode_dib(pixels)
        with_masks = encoded[:124] + bytes(12) + encoded[124:]
        self.assertTrue(numpy.array_equal(va.decode_dib(with_masks), pixels))

    def test_a_bitmap_cut_short_or_with_an_impossible_header_is_not_a_picture(self):
        encoded = va.encode_dib(solid_tile(RED, 4))
        self.assertIsNone(va.decode_dib(encoded[:60]))
        self.assertIsNone(va.decode_dib(bytes(60)))

    def test_a_png_is_read_with_its_alpha(self):
        pixels = numpy.array([[[255, 0, 0, 128], [0, 255, 0, 0]]], dtype=numpy.uint8)
        self.assertTrue(numpy.array_equal(va.decode_image_bytes(va.encode_png(pixels)), pixels))

    def test_data_that_is_not_a_picture_gives_none(self):
        self.assertIsNone(va.decode_dib(b"not a bitmap"))
        self.assertIsNone(va.decode_image_bytes(b"not a picture"))
        self.assertIsNone(va.decode_dib(b""))

    def test_other_systems_have_no_system_image_clipboard(self):
        with mock.patch.object(va.sys, "platform", "linux"):
            clipboard = va.SystemImageClipboard()
            self.assertIsNone(clipboard.get_image())
            self.assertFalse(clipboard.set_image(solid_tile(RED, 2)))

    @unittest.skipUnless(
        sys.platform == "win32" and os.environ.get("VIRTUAL_ATLAS_TEST_REAL_CLIPBOARD") == "1",
        "writes to the real clipboard: set VIRTUAL_ATLAS_TEST_REAL_CLIPBOARD=1 on Windows to run it",
    )
    def test_the_real_windows_clipboard_round_trips_a_picture_with_alpha(self):
        pixels = numpy.random.default_rng(9).integers(0, 256, size=(6, 5, 4), dtype=numpy.uint8)
        clipboard = va.SystemImageClipboard()
        self.assertTrue(clipboard.set_image(pixels))
        self.assertTrue(numpy.array_equal(clipboard.get_image(), pixels))


class ProjectRenameTests(ProjectFixture):
    def paint_tile(self, project, coordinate, row=0, column=0, color=RED):
        record = project.tiles[coordinate]
        layout = va.compose_canvas({coordinate: record.pixels}, self.tile_size)
        canvas = layout.pixels.copy()
        canvas[row, column] = color
        return project.apply_canvas_edit(layout, canvas)

    def test_renaming_a_tile_with_a_file_renames_the_file_and_atlas_json_and_keeps_the_pixels(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        self.assertTrue(project.rename_tile((0, 0), "stone.png"))
        self.assertFalse((self.folder / "a.png").exists())
        self.assertTrue(numpy.array_equal(va.read_image_pixels(self.folder / "stone.png"), solid_tile(BLUE)))
        self.assertEqual(self.saved().coordinates_to_names, {(0, 0): "stone.png"})
        self.assertEqual(project.tiles[(0, 0)].name, "stone.png")
        self.assertFalse(project.tiles[(0, 0)].is_modified)

    def test_renaming_keeps_unsaved_edits_and_their_history(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        record = project.tiles[(0, 0)]
        self.paint_tile(project, (0, 0))
        project.rename_tile((0, 0), "b.png")
        self.assertEqual(tuple(record.pixels[0, 0]), RED)
        self.assertTrue(record.is_modified)
        self.assertEqual(tuple(va.read_image_pixels(self.folder / "b.png")[0, 0]), BLUE, "the file is the old picture")
        project.select_only((0, 0))
        self.assertTrue(project.undo())
        self.assertFalse(record.is_modified)

    def test_a_new_tile_is_renamed_in_atlas_json_and_gets_no_file(self):
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(solid_tile(GREEN))})
        self.assertTrue(project.rename_tile((0, 0), "grass.png"))
        self.assertEqual(self.saved().coordinates_to_names, {(0, 0): "grass.png"})
        self.assertEqual(list(self.saved().coordinates_to_data), [(0, 0)])
        self.assertEqual([entry.name for entry in self.folder.iterdir() if entry.suffix == ".png"], [])
        project.save_tile((0, 0))
        self.assertTrue((self.folder / "grass.png").exists())

    def test_the_old_name_becomes_free_and_the_new_one_taken(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE)), (1, 0): ("c.png", solid_tile(RED))})
        project.rename_tile((0, 0), "b.png")
        self.assertIsNone(project.rename_problem((1, 0), "a.png"))
        self.assertIsNotNone(project.rename_problem((1, 0), "b.png"))

    def test_the_same_name_changes_nothing(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        before = (self.folder / "atlas.json").read_bytes()
        self.assertFalse(project.rename_tile((0, 0), "a.png"))
        self.assertEqual((self.folder / "atlas.json").read_bytes(), before)

    def test_only_the_letter_case_can_change(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        self.assertTrue(project.rename_tile((0, 0), "A.png"))
        self.assertEqual([entry.name for entry in self.folder.iterdir() if entry.suffix == ".png"], ["A.png"])
        self.assertEqual(self.saved().coordinates_to_names, {(0, 0): "A.png"})

    def test_a_bad_name_is_refused_with_the_reason_and_nothing_changes(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        for name in ["", "a", "a.jpg", "x/y.png", "CON.png", " a.png"]:
            self.assertIsNotNone(project.rename_problem((0, 0), name), name)
            with self.assertRaises(ValueError):
                project.rename_tile((0, 0), name)
        self.assertTrue((self.folder / "a.png").exists())
        self.assertEqual(project.tiles[(0, 0)].name, "a.png")

    def test_a_name_used_by_another_tile_an_unplaced_file_or_an_ignored_file_is_refused(self):
        va.write_png_atomically(self.folder / "wrong_size.png", solid_tile(RED, 8))
        project = self.make_project(
            {(0, 0): ("a.png", solid_tile(BLUE)), (1, 0): ("b.png", solid_tile(RED)), (2, 0): ("n.png", None)},
            unplaced=[("loose.png", solid_tile(GREEN))],
            data={(2, 0): va.encode_tile_data(solid_tile(GREEN))},
        )
        for name in ["b.png", "B.PNG", "loose.png", "wrong_size.png", "n.png", "N.png"]:
            self.assertIn("already used", project.rename_problem((0, 0), name), name)
        self.assertIn("already used", project.rename_problem((2, 0), "a.png"))
        self.assertIn("already used", project.rename_problem((2, 0), "wrong_size.png"))
        with self.assertRaises(ValueError):
            project.rename_tile((0, 0), "wrong_size.png")
        self.assertEqual(va.read_image_size(self.folder / "wrong_size.png"), (8, 8))

    def test_a_file_that_appeared_in_the_folder_since_loading_is_never_overwritten(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        va.write_png_atomically(self.folder / "new.png", solid_tile(RED, 8))
        with self.assertRaises(OSError):
            project.rename_tile((0, 0), "new.png")
        self.assertEqual(va.read_image_size(self.folder / "new.png"), (8, 8))
        self.assertTrue((self.folder / "a.png").exists())
        self.assertEqual(project.tiles[(0, 0)].name, "a.png")

    def test_a_file_is_never_renamed_over_another_even_where_the_system_would_allow_it(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        va.write_png_atomically(self.folder / "new.png", solid_tile(RED, 8))
        with mock.patch.object(va.os, "rename") as rename:
            with self.assertRaises(FileExistsError):
                project.rename_tile((0, 0), "new.png")
        rename.assert_not_called()

    def test_an_error_tile_cannot_be_renamed(self):
        project = self.make_project({(0, 0): ("gone.png", None)})
        self.assertIn("cannot be renamed", project.rename_problem((0, 0), "other.png"))
        with self.assertRaises(ValueError):
            project.rename_tile((0, 0), "other.png")

    def test_when_atlas_json_cannot_be_written_the_file_goes_back_to_its_name(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        before = (self.folder / "atlas.json").read_bytes()
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                project.rename_tile((0, 0), "b.png")
        self.assertTrue((self.folder / "a.png").exists())
        self.assertFalse((self.folder / "b.png").exists())
        self.assertEqual(project.tiles[(0, 0)].name, "a.png")
        self.assertEqual((self.folder / "atlas.json").read_bytes(), before)
        self.assertIsNone(project.rename_problem((0, 0), "c.png"))

    def test_when_the_file_cannot_be_renamed_nothing_changes(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        before = (self.folder / "atlas.json").read_bytes()
        with mock.patch.object(va.os, "rename", side_effect=PermissionError("in use")):
            with self.assertRaises(OSError):
                project.rename_tile((0, 0), "b.png")
        self.assertEqual(project.tiles[(0, 0)].name, "a.png")
        self.assertEqual((self.folder / "atlas.json").read_bytes(), before)

    def test_a_new_tile_whose_atlas_json_write_fails_keeps_its_name(self):
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(solid_tile(GREEN))})
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                project.rename_tile((0, 0), "m.png")
        self.assertEqual(project.tiles[(0, 0)].name, "n.png")
        self.assertEqual(self.saved().coordinates_to_names, {(0, 0): "n.png"})


class ProjectPurgeTests(ProjectFixture):
    def leftover_temporary_files(self):
        return [entry.name for entry in self.folder.iterdir() if entry.name.startswith(va.TEMPORARY_FILE_PREFIX)]

    def test_purging_a_tile_deletes_its_file_and_its_entry_in_atlas_json_and_nothing_else(self):
        project = self.make_project(
            {(0, 0): ("a.png", solid_tile(RED)), (1, 0): ("b.png", solid_tile(GREEN))},
            unplaced=[("loose.png", solid_tile(BLUE))],
            other_settings={"note": [1, 2]},
        )
        project.select_only((0, 0))
        project.purge_tile((0, 0))
        self.assertFalse((self.folder / "a.png").exists())
        self.assertTrue((self.folder / "b.png").exists())
        self.assertTrue((self.folder / "loose.png").exists())
        saved = self.saved()
        self.assertEqual(saved.coordinates_to_names, {(1, 0): "b.png"})
        self.assertEqual(saved.coordinates_to_data, {})
        self.assertEqual((saved.tile_size, saved.other_settings), (4, {"note": [1, 2]}))
        self.assertEqual(list(project.tiles), [(1, 0)])
        self.assertEqual(project.selection, set())
        self.assertEqual([tile.name for tile in project.unplaced], ["loose.png"], "a purged tile does not become unplaced")
        self.assertNotIn("a.png", project.existing_names)
        self.assertEqual(self.leftover_temporary_files(), [])

    def test_purging_keeps_the_selection_of_the_other_tiles(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED)), (1, 0): ("b.png", solid_tile(GREEN))})
        project.selection = {(0, 0), (1, 0)}
        project.purge_tile((0, 0))
        self.assertEqual(project.selection, {(1, 0)})

    def test_purging_a_tile_that_has_no_file_yet_deletes_its_stored_pixels_too(self):
        project = self.make_project(
            {(0, 0): ("n.png", None), (1, 0): ("b.png", solid_tile(GREEN))},
            data={(0, 0): va.encode_tile_data(solid_tile(RED))},
        )
        project.purge_tile((0, 0))
        saved = self.saved()
        self.assertEqual(saved.coordinates_to_names, {(1, 0): "b.png"})
        self.assertEqual(saved.coordinates_to_data, {})
        self.assertNotIn("n.png", (self.folder / "atlas.json").read_text())

    def test_purging_a_tile_with_a_file_and_stored_pixels_deletes_both_entries(self):
        project = self.make_project(
            {(0, 0): ("a.png", solid_tile(RED))}, data={(0, 0): va.encode_tile_data(solid_tile(GREEN))}
        )
        self.assertTrue(project.tiles[(0, 0)].is_modified)
        project.purge_tile((0, 0))
        self.assertFalse((self.folder / "a.png").exists())
        saved = self.saved()
        self.assertEqual((saved.coordinates_to_names, saved.coordinates_to_data), ({}, {}))

    def test_purging_a_tile_with_unsaved_edits_deletes_the_file_as_it_is_on_disk(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        record = project.tiles[(0, 0)]
        layout = va.compose_canvas({(0, 0): record.pixels}, self.tile_size)
        canvas = layout.pixels.copy()
        canvas[0, 0] = RED
        project.apply_canvas_edit(layout, canvas)
        self.assertTrue(project.history.can_undo([record]))
        project.purge_tile((0, 0))
        self.assertFalse((self.folder / "a.png").exists())
        self.assertFalse(project.history.can_undo([record]), "the history of a purged tile is forgotten")

    def test_an_error_tile_can_be_purged_whether_or_not_its_file_exists(self):
        va.write_png_atomically(self.folder / "wrong_size.png", solid_tile(RED, 8))
        project = self.make_project(
            {(0, 0): ("gone.png", None), (1, 0): ("wrong_size.png", None), (2, 0): ("bad_data.png", None)},
            data={(2, 0): "this is not base64!"},
        )
        self.assertTrue(all(record.error is not None for record in project.tiles.values()))
        for coordinate in [(0, 0), (1, 0), (2, 0)]:
            project.purge_tile(coordinate)
        self.assertFalse((self.folder / "wrong_size.png").exists())
        saved = self.saved()
        self.assertEqual((saved.coordinates_to_names, saved.coordinates_to_data), ({}, {}))

    def test_the_file_is_found_when_its_letter_case_differs_from_the_name_in_atlas_json(self):
        va.write_png_atomically(self.folder / "A.png", solid_tile(RED))
        project = self.make_project({(0, 0): ("a.png", None)})
        self.assertIsNone(project.tiles[(0, 0)].error, "the scan matches names ignoring case")
        project.purge_tile((0, 0))
        self.assertEqual([entry.name for entry in self.folder.iterdir() if entry.suffix == ".png"], [])

    def test_a_purged_name_can_be_used_again(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED)), (1, 0): ("b.png", solid_tile(GREEN))})
        project.purge_tile((0, 0))
        self.assertIsNone(project.rename_problem((1, 0), "a.png"))

    def test_an_empty_place_is_refused_and_changes_nothing(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED))})
        before = (self.folder / "atlas.json").read_bytes()
        with self.assertRaises(KeyError):
            project.purge_tile((5, 5))
        self.assertEqual((self.folder / "atlas.json").read_bytes(), before)
        self.assertTrue((self.folder / "a.png").exists())

    def test_when_atlas_json_cannot_be_written_the_file_and_the_tile_come_back(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED)), (1, 0): ("b.png", solid_tile(GREEN))})
        project.select_only((0, 0))
        before = (self.folder / "atlas.json").read_bytes()
        file_before = (self.folder / "a.png").read_bytes()
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                project.purge_tile((0, 0))
        self.assertEqual((self.folder / "a.png").read_bytes(), file_before)
        self.assertEqual((self.folder / "atlas.json").read_bytes(), before)
        self.assertEqual(sorted(project.tiles), [(0, 0), (1, 0)])
        self.assertEqual(project.selection, {(0, 0)})
        self.assertIn("a.png", project.existing_names)
        self.assertEqual(self.leftover_temporary_files(), [])
        project.purge_tile((0, 0))
        self.assertFalse((self.folder / "a.png").exists(), "the tile can be purged once the disk works again")

    def test_when_the_file_cannot_be_moved_away_nothing_changes(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED))})
        before = (self.folder / "atlas.json").read_bytes()
        with mock.patch.object(va.os, "rename", side_effect=PermissionError("in use")):
            with self.assertRaises(OSError):
                project.purge_tile((0, 0))
        self.assertTrue((self.folder / "a.png").exists())
        self.assertEqual((self.folder / "atlas.json").read_bytes(), before)
        self.assertIn((0, 0), project.tiles)

    def test_a_tile_whose_atlas_json_write_fails_and_that_has_no_file_stays(self):
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(solid_tile(GREEN))})
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                project.purge_tile((0, 0))
        self.assertIn((0, 0), project.tiles)
        self.assertEqual(list(self.saved().coordinates_to_data), [(0, 0)])

    def test_a_leftover_that_cannot_be_deleted_does_not_undo_the_purge_but_is_reported(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED))})
        with mock.patch.object(va.os, "unlink", side_effect=PermissionError("in use")):
            with self.assertLogs(va.logger, level="WARNING"):
                project.purge_tile((0, 0))
        self.assertEqual(self.saved().coordinates_to_names, {})
        self.assertFalse((self.folder / "a.png").exists())
        self.assertTrue(project.is_empty)
        self.assertEqual(len(self.leftover_temporary_files()), 1)
        self.assertEqual(project.unplaced, [], "the leftover is not a tile image, so the folder scan ignores it")


class PurgeDialogTests(ProjectFixture):
    def test_the_warning_is_short_and_says_the_file_and_the_entries_go_and_that_it_cannot_be_undone(self):
        project = self.make_project({(0, 0): ("stone.png", solid_tile(RED))})
        dialog = va.PurgeDialog(project, (0, 0))
        self.assertEqual(dialog.title, "Purge stone.png?")
        self.assertIn("file", dialog.message)
        self.assertIn("atlas.json", dialog.message)
        self.assertIn("cannot be undone", dialog.message)
        self.assertNotIn("unsaved", dialog.message)
        self.assertLess(len(dialog.message), 120, "concise")

    def test_the_warning_mentions_unsaved_changes_only_for_a_tile_that_has_them(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE)), (1, 0): ("n.png", None)}, data={(1, 0): va.encode_tile_data(solid_tile(RED))})
        record = project.tiles[(0, 0)]
        layout = va.compose_canvas({(0, 0): record.pixels}, self.tile_size)
        canvas = layout.pixels.copy()
        canvas[0, 0] = RED
        project.apply_canvas_edit(layout, canvas)
        self.assertIn("unsaved", va.PurgeDialog(project, (0, 0)).message)
        message = va.PurgeDialog(project, (1, 0)).message
        self.assertIn("unsaved", message)
        self.assertNotIn("the file", message, "a tile with no file has none to delete")

    def test_confirming_purges_and_reports_done(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED))})
        dialog = va.PurgeDialog(project, (0, 0))
        self.assertEqual(dialog.confirm(), "done")
        self.assertTrue(project.is_empty)
        self.assertFalse((self.folder / "a.png").exists())

    def test_cancelling_changes_nothing(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED))})
        dialog = va.PurgeDialog(project, (0, 0))
        self.assertEqual(dialog.cancel(), "cancel")
        self.assertTrue((self.folder / "a.png").exists())
        self.assertIn((0, 0), project.tiles)

    def test_a_disk_error_keeps_the_dialog_open_with_the_reason(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(RED))})
        dialog = va.PurgeDialog(project, (0, 0))
        with mock.patch.object(va.os, "rename", side_effect=PermissionError("in use")):
            self.assertEqual(dialog.confirm(), "ask")
        self.assertIn("a.png", dialog.error)
        self.assertIn("in use", dialog.error)
        self.assertTrue((self.folder / "a.png").exists())
        self.assertEqual(dialog.confirm(), "done", "it can be tried again")


class UnsavedTilesTests(ProjectFixture):
    def test_the_unsaved_tiles_are_the_modified_ones_including_new_tiles_in_reading_order(self):
        project = self.make_project(
            {(1, 0): ("b.png", solid_tile(BLUE)), (0, 1): ("n.png", None), (0, 0): ("a.png", solid_tile(BLUE)), (2, 0): ("gone.png", None)},
            data={(0, 1): va.encode_tile_data(solid_tile(GREEN))},
        )
        self.assertEqual(project.unsaved_tiles(), [(0, 1)])
        for coordinate in [(1, 0), (0, 0)]:
            record = project.tiles[coordinate]
            layout = va.compose_canvas({coordinate: record.pixels}, 4)
            canvas = layout.pixels.copy()
            canvas[0, 0] = RED
            project.apply_canvas_edit(layout, canvas)
        self.assertEqual(project.unsaved_tiles(), [(0, 0), (1, 0), (0, 1)])
        project.save_all_tiles()
        self.assertEqual(project.unsaved_tiles(), [])


class QuitDialogTests(ProjectFixture):
    def make_two(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE)), (1, 0): ("b.png", solid_tile(GREEN))})
        for coordinate in [(0, 0), (1, 0)]:
            record = project.tiles[coordinate]
            layout = va.compose_canvas({coordinate: record.pixels}, 4)
            canvas = layout.pixels.copy()
            canvas[0, 0] = RED
            project.apply_canvas_edit(layout, canvas)
        return project

    def disk(self, name, row=0, column=0):
        return tuple(int(v) for v in va.read_image_pixels(self.folder / name)[row, column])

    def test_it_asks_about_each_unsaved_tile_in_reading_order(self):
        project = self.make_two()
        dialog = va.QuitDialog(project)
        self.assertEqual((dialog.record.name, dialog.remaining), ("a.png", 2))
        self.assertEqual(dialog.discard(), "ask")
        self.assertEqual((dialog.record.name, dialog.remaining), ("b.png", 1))
        self.assertEqual(dialog.discard(), "quit")

    def test_save_writes_the_file_and_asks_about_the_next_tile(self):
        project = self.make_two()
        dialog = va.QuitDialog(project)
        self.assertEqual(dialog.save(), "ask")
        self.assertEqual(self.disk("a.png"), RED)
        self.assertEqual(self.disk("b.png"), GREEN)
        self.assertEqual(dialog.save(), "quit")
        self.assertEqual(self.disk("b.png"), RED)

    def test_dont_save_leaves_the_file_and_the_unsaved_pixels_alone(self):
        project = self.make_two()
        dialog = va.QuitDialog(project)
        dialog.discard()
        dialog.discard()
        self.assertEqual(self.disk("a.png"), BLUE)
        self.assertTrue(project.tiles[(0, 0)].is_modified)

    def test_cancel_changes_nothing_and_tiles_already_saved_stay_saved(self):
        project = self.make_two()
        dialog = va.QuitDialog(project)
        dialog.save()
        self.assertEqual(dialog.cancel(), "cancel")
        self.assertEqual(self.disk("a.png"), RED)
        self.assertEqual(self.disk("b.png"), GREEN)

    def test_do_this_for_all_saves_every_remaining_tile(self):
        project = self.make_two()
        dialog = va.QuitDialog(project)
        dialog.toggle_apply_to_all()
        self.assertEqual(dialog.save(), "quit")
        self.assertEqual((self.disk("a.png"), self.disk("b.png")), (RED, RED))

    def test_do_this_for_all_can_skip_every_remaining_tile(self):
        project = self.make_two()
        dialog = va.QuitDialog(project)
        dialog.discard()
        dialog.toggle_apply_to_all()
        self.assertEqual(dialog.discard(), "quit")
        self.assertEqual((self.disk("a.png"), self.disk("b.png")), (BLUE, GREEN))

    def test_the_checkbox_only_counts_for_the_tiles_still_to_come(self):
        project = self.make_two()
        dialog = va.QuitDialog(project)
        dialog.save()
        dialog.toggle_apply_to_all()
        self.assertEqual(dialog.remaining, 1)
        self.assertEqual(dialog.save(), "quit")

    def test_a_failed_save_stays_on_that_tile_with_the_reason_and_saves_nothing_after_it(self):
        project = self.make_two()
        dialog = va.QuitDialog(project)
        dialog.toggle_apply_to_all()
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            self.assertEqual(dialog.save(), "ask")
        self.assertIn("a.png", dialog.error)
        self.assertIn("disk full", dialog.error)
        self.assertEqual(dialog.record.name, "a.png")
        self.assertEqual(dialog.discard(), "quit", "with the box still ticked, every tile is skipped")
        self.assertEqual(self.disk("b.png"), GREEN)

    def test_an_error_goes_away_when_the_next_answer_works(self):
        project = self.make_two()
        dialog = va.QuitDialog(project)
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            dialog.save()
        self.assertTrue(dialog.error)
        self.assertEqual(dialog.save(), "ask")
        self.assertEqual(dialog.error, "")

    def test_a_tile_that_has_no_file_yet_shows_nothing_on_the_disk_side_and_is_written_by_save(self):
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(solid_tile(GREEN))})
        dialog = va.QuitDialog(project)
        self.assertIsNone(dialog.disk_pixels)
        self.assertTrue(numpy.array_equal(dialog.current_pixels, solid_tile(GREEN)))
        self.assertEqual(dialog.save(), "quit")
        self.assertEqual(self.disk("n.png"), GREEN)

    def test_dont_save_on_a_tile_with_no_file_leaves_it_in_atlas_json(self):
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(solid_tile(GREEN))})
        va.QuitDialog(project).discard()
        self.assertFalse((self.folder / "n.png").exists())
        self.assertEqual(list(self.saved().coordinates_to_data), [(0, 0)])

    def test_the_two_pictures_are_the_current_pixels_and_what_the_file_has(self):
        project = self.make_two()
        dialog = va.QuitDialog(project)
        self.assertEqual(tuple(dialog.current_pixels[0, 0]), RED)
        self.assertEqual(tuple(dialog.disk_pixels[0, 0]), BLUE)

    def test_a_tile_is_drawn_at_a_whole_number_of_screen_pixels_per_pixel_when_it_fits(self):
        box = va.pygame.Rect(100, 50, 220, 220)
        picture = va.fit_picture_rectangle(box, 8)
        self.assertEqual(picture.size, (216, 216))
        self.assertEqual(picture.center, box.center)
        self.assertEqual(va.fit_picture_rectangle(box, 16).size, (208, 208))
        self.assertEqual(va.fit_picture_rectangle(box, 220).size, (220, 220))
        self.assertEqual(va.fit_picture_rectangle(box, 1000).size, (220, 220), "a big tile is shrunk to the box")

    def test_the_parts_fit_inside_the_panel_and_the_window_without_overlapping(self):
        for size in [(1280, 800), (900, 600), (640, 480)]:
            for checkbox in (True, False):
                areas = va.quit_dialog_areas(size, checkbox)
                window = va.pygame.Rect(0, 0, *size)
                self.assertTrue(window.contains(areas["panel"]), size)
                parts = {name: rectangle for name, rectangle in areas.items() if name != "panel"}
                if not checkbox:
                    self.assertNotIn("checkbox", parts)
                for name, rectangle in parts.items():
                    self.assertTrue(areas["panel"].contains(rectangle), (name, size))
                names = list(parts)
                for index, first in enumerate(names):
                    for second in names[index + 1:]:
                        self.assertFalse(parts[first].colliderect(parts[second]), (first, second, size))


def striped_picture(width, height):
    """A picture whose every pixel tells where it is: red is the column, green the row (both modulo 256)."""
    pixels = numpy.zeros((height, width, 4), dtype=numpy.uint8)
    pixels[..., 0] = numpy.arange(width)[None, :] % 256
    pixels[..., 1] = numpy.arange(height)[:, None] % 256
    pixels[..., 3] = 255
    return pixels


class ProjectImportTests(ProjectFixture):
    def test_the_tiles_of_a_picture_become_new_tiles_at_their_places_with_their_pixels_in_atlas_json(self):
        project = self.make_project(write_atlas=False, tile_size=4)
        project.set_tile_size(4)
        picture = striped_picture(8, 8)
        tiles = va.slice_image_into_tiles(picture, 4, "crop")
        names = project.import_tiles(tiles)
        self.assertEqual(names[(1, 0)], "tile_00_01.png")
        self.assertEqual(sorted(project.tiles, key=va.reading_order_key), [(0, 0), (1, 0), (0, 1), (1, 1)])
        self.assertTrue(numpy.array_equal(project.tiles[(1, 1)].pixels, picture[4:8, 4:8]))
        self.assertTrue(all(record.is_new for record in project.tiles.values()))
        saved = self.saved()
        self.assertEqual(saved.tile_size, 4)
        self.assertEqual(saved.coordinates_to_names[(0, 1)], "tile_01_00.png")
        self.assertEqual(sorted(saved.coordinates_to_data), sorted(project.tiles))
        self.assertEqual([entry.name for entry in self.folder.iterdir() if entry.suffix == ".png"], [])
        self.assertEqual(project.unsaved_tiles(), sorted(project.tiles, key=va.reading_order_key))

    def test_saving_the_imported_tiles_writes_their_files_and_empties_the_stored_data(self):
        project = self.make_project(write_atlas=False, tile_size=4)
        project.set_tile_size(4)
        project.import_tiles(va.slice_image_into_tiles(striped_picture(8, 4), 4, "crop"))
        self.assertEqual(project.save_all_tiles(), (2, []))
        self.assertEqual(sorted(entry.name for entry in self.folder.iterdir() if entry.suffix == ".png"), ["tile_00_00.png", "tile_00_01.png"])
        self.assertEqual(self.saved().coordinates_to_data, {})

    def test_names_avoid_every_png_in_the_folder_even_ones_that_are_not_tiles(self):
        va.write_png_atomically(self.folder / "tile_00_00.png", solid_tile(RED, 8))
        project = self.make_project(unplaced=[("Tile_00_01.png", solid_tile(GREEN))], write_atlas=False, tile_size=4)
        project.set_tile_size(4)
        names = project.import_tiles(va.slice_image_into_tiles(striped_picture(8, 4), 4, "crop"))
        self.assertEqual(names, {(0, 0): "tile_00_00_2.png", (1, 0): "tile_00_01_2.png"})

    def test_an_atlas_that_already_has_tiles_refuses_an_import_and_stays_as_it_was(self):
        project = self.make_project({(0, 0): ("a.png", solid_tile(BLUE))})
        before = (self.folder / "atlas.json").read_bytes()
        with self.assertRaises(ValueError):
            project.import_tiles({(0, 0): solid_tile(RED)})
        self.assertEqual((self.folder / "atlas.json").read_bytes(), before)

    def test_nothing_to_import_and_tiles_of_the_wrong_size_are_refused(self):
        project = self.make_project(write_atlas=False, tile_size=4)
        project.set_tile_size(4)
        with self.assertRaises(ValueError):
            project.import_tiles({})
        with self.assertRaises(ValueError):
            project.import_tiles({(0, 0): solid_tile(RED, 5)})
        self.assertTrue(project.is_empty)

    def test_when_atlas_json_cannot_be_written_nothing_is_imported_and_the_tile_size_stays_open(self):
        project = self.make_project(write_atlas=False, tile_size=4)
        project.set_tile_size(4)
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                project.import_tiles({(0, 0): solid_tile(RED)})
        self.assertTrue(project.is_empty)
        self.assertFalse(project.tile_size_is_saved)
        self.assertFalse((self.folder / "atlas.json").exists())

    def test_an_atlas_json_that_has_a_tile_size_but_no_tiles_can_be_imported_into(self):
        project = self.make_project(tile_size=4)
        self.assertTrue(project.tile_size_is_saved)
        project.import_tiles({(0, 0): solid_tile(RED), (2, 1): solid_tile(GREEN)})
        self.assertEqual(sorted(self.saved().coordinates_to_names), [(0, 0), (2, 1)])


class ImportDialogTests(unittest.TestCase):
    def test_an_even_picture_has_nothing_to_choose(self):
        dialog = va.ImportDialog(striped_picture(32, 16), 16, "p.png")
        self.assertFalse(dialog.can_choose_mode)
        self.assertEqual(dialog.tile_count, 2)
        self.assertEqual(sorted(dialog.tiles()), [(0, 0), (1, 0)])

    def test_a_picture_that_does_not_divide_evenly_offers_pad_and_crop_and_pad_is_the_start(self):
        dialog = va.ImportDialog(striped_picture(37, 20), 16, "p.png")
        self.assertTrue(dialog.can_choose_mode)
        self.assertEqual(dialog.mode, "pad")
        self.assertEqual(dialog.tile_count, 6)
        dialog.set_mode("crop")
        self.assertEqual((dialog.mode, dialog.tile_count), ("crop", 2))
        self.assertEqual(sorted(dialog.tiles()), [(0, 0), (1, 0)])
        dialog.set_mode("pad")
        self.assertEqual(len(dialog.tiles()), 6)

    def test_a_picture_smaller_than_a_tile_can_only_be_padded(self):
        dialog = va.ImportDialog(striped_picture(10, 10), 16, "p.png")
        self.assertFalse(dialog.can_choose_mode)
        dialog.set_mode("crop")
        self.assertEqual((dialog.mode, dialog.tile_count), ("pad", 1))

    def test_a_picture_that_would_make_too_many_tiles_is_flagged_with_the_numbers(self):
        dialog = va.ImportDialog(striped_picture(64, 64), 4, "p.png")
        self.assertEqual(dialog.tile_count, 256)
        self.assertIsNone(dialog.problem())
        with mock.patch.object(va, "MAXIMUM_IMPORT_TILES", 100):
            self.assertIn("256", dialog.problem())
            self.assertIn("100", dialog.problem())

    def test_the_mode_cannot_be_something_else(self):
        dialog = va.ImportDialog(striped_picture(37, 20), 16, "p.png")
        dialog.set_mode("stretch")
        self.assertEqual(dialog.mode, "pad")

    def test_the_parts_fit_inside_the_panel_and_the_window_without_overlapping(self):
        for size in [(1280, 800), (900, 600), (700, 520)]:
            areas = va.import_dialog_areas(size)
            window = va.pygame.Rect(0, 0, *size)
            self.assertTrue(window.contains(areas["panel"]), size)
            parts = {name: rectangle for name, rectangle in areas.items() if name != "panel"}
            for name, rectangle in parts.items():
                self.assertTrue(areas["panel"].contains(rectangle), (name, size))
            names = list(parts)
            for index, first in enumerate(names):
                for second in names[index + 1:]:
                    self.assertFalse(parts[first].colliderect(parts[second]), (first, second, size))


class FilePickerTests(unittest.TestCase):
    def pick(self, **dialog_behaviour):
        import tkinter
        from tkinter import filedialog

        with mock.patch.object(tkinter, "Tk") as window, mock.patch.object(filedialog, "askopenfilename", **dialog_behaviour) as ask:
            result = va.SystemFilePicker().choose_picture()
        window.return_value.destroy.assert_called_once()
        return result, ask

    def test_the_chosen_file_is_returned_as_a_path(self):
        result, ask = self.pick(return_value="C:/art/sheet.png")
        self.assertEqual(result, Path("C:/art/sheet.png"))
        self.assertIn("png", str(ask.call_args.kwargs["filetypes"][0][1]))

    def test_giving_up_in_the_dialog_returns_none(self):
        self.assertIsNone(self.pick(return_value="")[0])

    def test_a_dialog_that_fails_returns_none_and_still_closes_its_window(self):
        self.assertIsNone(self.pick(side_effect=RuntimeError("no display"))[0])


class FakeFilePicker:
    def __init__(self, path=None):
        self.path = path
        self.calls = 0

    def choose_picture(self):
        self.calls += 1
        return self.path


class ImportWindowTests(ProjectFixture):
    @classmethod
    def setUpClass(cls):
        va.pygame.init()

    def setUp(self):
        super().setUp()
        self.picture_folder = tempfile.TemporaryDirectory()
        self.picture_path = Path(self.picture_folder.name) / "sheet.png"
        va.write_png_atomically(self.picture_path, striped_picture(40, 24))
        self.picker = FakeFilePicker(self.picture_path)
        self.project = self.make_project(write_atlas=False, tile_size=16)
        self.application = va.Application(self.project, va.pygame.Surface((1280, 800)))
        self.application.file_picker = self.picker

    def tearDown(self):
        self.picture_folder.cleanup()
        super().tearDown()

    def send(self, kind, position, button=1):
        event_type = {"down": va.pygame.MOUSEBUTTONDOWN, "up": va.pygame.MOUSEBUTTONUP}[kind]
        return self.application.handle_event(va.pygame.event.Event(event_type, pos=position, button=button))

    def click(self, position):
        self.send("down", position)
        return self.send("up", position)

    def key(self, key):
        return self.application.handle_event(va.pygame.event.Event(va.pygame.KEYDOWN, key=key, mod=0, unicode="", scancode=0))

    def click_import_button(self):
        self.application.draw()
        self.click(self.application.empty_state_rectangles()["import"].center)

    def press(self, name):
        return self.click(self.application.import_dialog_areas()[name].center)

    def test_the_button_asks_for_a_picture_and_shows_what_will_be_made_of_it(self):
        self.click_import_button()
        self.assertEqual(self.picker.calls, 1)
        dialog = self.application.import_dialog
        self.assertIsNotNone(dialog)
        self.assertEqual((dialog.pixels.shape, dialog.tile_size, dialog.source_name), ((24, 40, 4), 16, "sheet.png"))
        self.application.draw()

    def test_importing_fills_the_atlas_with_unsaved_tiles_and_closes_the_dialog(self):
        self.click_import_button()
        self.press("import")
        self.assertIsNone(self.application.import_dialog)
        self.assertEqual(sorted(self.project.tiles, key=va.reading_order_key), [(column, row) for row in range(2) for column in range(3)])
        self.assertEqual(self.project.tiles[(2, 1)].name, "tile_01_02.png")
        self.assertEqual(self.project.unsaved_tiles(), sorted(self.project.tiles, key=va.reading_order_key))
        self.assertIn("Imported 6 tiles", self.application.message)
        self.assertEqual(sorted(self.saved().coordinates_to_names), sorted(self.project.tiles))
        padded = self.project.tiles[(2, 1)].pixels
        self.assertTrue(numpy.all(padded[8:] == 0), "the part below the picture is transparent")
        self.assertEqual(tuple(padded[0, 0]), (32, 16, 0, 255))
        self.application.draw()

    def test_crop_leaves_the_partial_tiles_out(self):
        self.click_import_button()
        self.press("crop")
        self.assertEqual(self.application.import_dialog.mode, "crop")
        self.application.draw()
        self.press("import")
        self.assertEqual(sorted(self.project.tiles), [(0, 0), (1, 0)])

    def test_pad_can_be_chosen_again_after_crop(self):
        self.click_import_button()
        self.press("crop")
        self.press("pad")
        self.assertEqual(self.application.import_dialog.mode, "pad")

    def test_cancelling_the_file_picker_changes_nothing(self):
        self.picker.path = None
        self.click_import_button()
        self.assertIsNone(self.application.import_dialog)
        self.assertTrue(self.project.is_empty)

    def test_a_file_that_is_not_a_picture_is_reported_and_no_dialog_opens(self):
        broken = Path(self.picture_folder.name) / "broken.png"
        broken.write_bytes(b"this is not a picture")
        self.picker.path = broken
        self.click_import_button()
        self.assertIsNone(self.application.import_dialog)
        self.assertIn("Could not read", self.application.message)
        self.assertIn("broken.png", self.application.message)

    def test_the_tile_size_typed_in_the_field_is_the_one_used_and_is_saved_with_the_import(self):
        self.application.tile_size_field.set_text("8")
        self.click_import_button()
        self.assertEqual(self.application.import_dialog.tile_size, 8)
        self.press("import")
        self.assertEqual(self.saved().tile_size, 8)
        self.assertEqual(len(self.project.tiles), 15)

    def test_a_tile_size_that_is_not_allowed_stops_the_import_before_the_picker_opens(self):
        self.application.tile_size_field.set_text("0")
        self.click_import_button()
        self.assertEqual(self.picker.calls, 0)
        self.assertIsNone(self.application.import_dialog)
        self.assertTrue(self.application.tile_size_field_invalid)

    def test_escape_cancels_and_enter_imports(self):
        self.click_import_button()
        self.key(va.pygame.K_ESCAPE)
        self.assertIsNone(self.application.import_dialog)
        self.assertTrue(self.project.is_empty)
        self.click_import_button()
        self.key(va.pygame.K_RETURN)
        self.assertEqual(len(self.project.tiles), 6)

    def test_the_cancel_button_closes_the_dialog(self):
        self.click_import_button()
        self.press("cancel")
        self.assertIsNone(self.application.import_dialog)
        self.assertTrue(self.project.is_empty)

    def test_nothing_behind_the_dialog_reacts(self):
        self.click_import_button()
        self.click(self.application.empty_state_rectangles()["import"].center)
        self.assertEqual(self.picker.calls, 1, "the button behind the dialog was not pressed again")
        self.application.tile_size_field_focused = False
        self.click(self.application.empty_state_rectangles()["field"].center)
        self.assertFalse(self.application.tile_size_field_focused)

    def test_a_picture_that_would_make_too_many_tiles_cannot_be_imported(self):
        with mock.patch.object(va, "MAXIMUM_IMPORT_TILES", 4):
            self.click_import_button()
            self.application.draw()
            self.press("import")
            self.assertIsNotNone(self.application.import_dialog)
            self.assertIn("6", self.application.import_dialog.problem())
            self.assertTrue(self.project.is_empty)
            self.press("crop")
            self.assertEqual(self.application.import_dialog.error, "", "choosing again clears the old complaint")
            self.press("import")
        self.assertIsNone(self.application.import_dialog)
        self.assertEqual(len(self.project.tiles), 2)

    def test_a_picture_that_makes_exactly_the_most_tiles_is_allowed(self):
        with mock.patch.object(va, "MAXIMUM_IMPORT_TILES", 6):
            self.click_import_button()
            self.assertIsNone(self.application.import_dialog.problem())
            self.press("import")
        self.assertEqual(len(self.project.tiles), 6)

    def test_when_atlas_json_cannot_be_written_the_dialog_stays_open_and_says_so(self):
        self.click_import_button()
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            self.press("import")
        self.assertIsNotNone(self.application.import_dialog)
        self.assertIn("disk full", self.application.import_dialog.error)
        self.assertTrue(self.project.is_empty)
        self.application.draw()
        self.press("import")
        self.assertEqual(len(self.project.tiles), 6)

    def test_closing_the_window_while_the_dialog_is_open_closes_it_without_importing(self):
        self.click_import_button()
        self.assertFalse(self.application.handle_event(va.pygame.event.Event(va.pygame.QUIT)))

    def test_closing_after_an_import_asks_about_every_new_tile(self):
        self.click_import_button()
        self.press("import")
        self.assertTrue(self.application.handle_event(va.pygame.event.Event(va.pygame.QUIT)))
        self.assertEqual(self.application.quit_dialog.remaining, 6)

    def test_the_dialog_draws_in_both_modes_and_in_a_small_window(self):
        self.click_import_button()
        self.application.draw()
        self.press("crop")
        self.application.draw()
        small = va.Application(self.project, va.pygame.Surface((700, 520)))
        small.file_picker = self.picker
        small.import_dialog = va.ImportDialog(striped_picture(40, 24), 16, "sheet.png")
        small.draw()
        even = va.ImportDialog(striped_picture(32, 32), 16, "even.png")
        small.import_dialog = even
        small.draw()
        small.import_dialog = va.ImportDialog(striped_picture(5, 5), 16, "tiny.png")
        small.draw()

    def test_the_button_hint_says_what_it_does(self):
        self.application.pointer = self.application.empty_state_rectangles()["import"].center
        self.assertIn("picture", self.application.hover_text())
        self.assertNotIn("not built", self.application.hover_text())


class EditorDriver(ProjectFixture):
    """Drives the editor with mouse and key events the way a user would; the tests are in the classes below."""

    @classmethod
    def setUpClass(cls):
        va.pygame.init()

    def setUp(self):
        super().setUp()
        placed = {(0, 0): ("a.png", solid_tile(BLUE, 8)), (1, 0): ("b.png", solid_tile(GREEN, 8)), (3, 3): ("c.png", solid_tile(RED, 8))}
        self.tile_size = 8
        self.project = self.make_project(placed, tile_size=8)
        self.application = va.Application(self.project, va.pygame.Surface((1280, 800)))
        self.editor = self.application.editor
        self.clipboard = FakeImageClipboard()
        self.editor.clipboard = self.clipboard

    def select(self, *coordinates):
        self.project.selection = set(coordinates)
        self.editor.sync()

    def position(self, column, row):
        return self.editor.view.pixel_rectangle(column, row).center

    def send(self, kind, position, button=1):
        event_type = {"down": va.pygame.MOUSEBUTTONDOWN, "up": va.pygame.MOUSEBUTTONUP}[kind]
        self.application.handle_event(va.pygame.event.Event(event_type, pos=position, button=button))

    def move(self, position):
        self.application.handle_event(va.pygame.event.Event(va.pygame.MOUSEMOTION, pos=position, rel=(0, 0), buttons=(1, 0, 0)))

    def drag(self, start, end, button=1):
        self.send("down", start, button)
        self.move(end)
        self.send("up", end, button)

    def key(self, key, modifiers=0):
        self.application.handle_event(va.pygame.event.Event(va.pygame.KEYDOWN, key=key, mod=modifiers, unicode="", scancode=0))

    def click_tool(self, tool):
        self.send("down", self.editor.areas.tool_buttons[tool].center)
        self.send("up", self.editor.areas.tool_buttons[tool].center)

    def pixel(self, coordinate, column, row):
        return tuple(int(v) for v in self.project.tiles[coordinate].pixels[row, column])


class EditorTests(EditorDriver):
    """Checks the editor through mouse and key events."""

    def test_the_canvas_shows_the_selected_tiles_and_the_name_above_it(self):
        self.select((0, 0))
        self.assertEqual(self.editor.header_text(), "a.png")
        self.select((0, 0), (1, 0))
        self.assertEqual(self.editor.header_text(), "(multiple files)")
        self.assertEqual(self.editor.layout.pixels.shape, (8, 16, 4))
        self.select()
        self.assertEqual(self.editor.header_text(), "")
        self.assertIsNone(self.editor.layout)

    def test_the_name_gets_an_asterisk_when_any_shown_tile_is_modified(self):
        self.select((0, 0), (1, 0))
        self.drag(self.position(1, 1), self.position(2, 1))
        self.assertEqual(self.editor.header_text(), "(multiple files)*")
        self.select((1, 0))
        self.assertEqual(self.editor.header_text(), "b.png")

    def test_drawing_with_the_pencil_changes_the_tile_and_its_atlas_picture_and_marks_it_unsaved(self):
        self.select((0, 0))
        self.editor.colors.set_color(RED, "primary")
        self.drag(self.position(1, 1), self.position(5, 1))
        self.assertEqual(self.pixel((0, 0), 3, 1), RED)
        self.assertEqual(self.pixel((0, 0), 0, 1), BLUE)
        self.assertTrue(self.project.tiles[(0, 0)].is_modified)
        self.application.draw()
        atlas_rectangle = self.application.atlas_panel.tile_screen_rect((0, 0))
        cell = atlas_rectangle.width // 8
        sample = (atlas_rectangle.x + 3 * cell + cell // 2, atlas_rectangle.y + 1 * cell + cell // 2)
        self.assertEqual(tuple(self.application.surface.get_at(sample))[:3], RED[:3], "the thumbnail shows the edit")

    def test_the_right_button_draws_with_the_secondary_color(self):
        self.select((0, 0))
        self.editor.colors.set_color(RED, "primary")
        self.editor.colors.set_color(GREEN, "secondary")
        self.send("down", self.position(2, 2), 3)
        self.send("up", self.position(2, 2), 3)
        self.assertEqual(self.pixel((0, 0), 2, 2), GREEN)

    def test_a_stroke_is_one_undo_step_and_undo_redo_keys_work(self):
        self.select((0, 0))
        self.drag(self.position(0, 0), self.position(7, 0))
        self.assertEqual(self.pixel((0, 0), 7, 0), (0, 0, 0, 255))
        self.key(va.pygame.K_z, va.pygame.KMOD_CTRL)
        self.assertEqual(self.pixel((0, 0), 7, 0), BLUE)
        self.assertEqual(self.pixel((0, 0), 0, 0), BLUE, "the whole stroke went in one step")
        self.assertFalse(self.project.tiles[(0, 0)].is_modified)
        self.key(va.pygame.K_y, va.pygame.KMOD_CTRL)
        self.assertEqual(self.pixel((0, 0), 7, 0), (0, 0, 0, 255))
        self.key(va.pygame.K_z, va.pygame.KMOD_CTRL)
        self.key(va.pygame.K_z, va.pygame.KMOD_CTRL | va.pygame.KMOD_SHIFT)
        self.assertEqual(self.pixel((0, 0), 7, 0), (0, 0, 0, 255), "Ctrl+Shift+Z redoes")

    def test_the_eraser_makes_transparent_and_does_not_enter_the_color_history(self):
        self.select((0, 0))
        self.click_tool("eraser")
        self.drag(self.position(0, 3), self.position(3, 3))
        self.assertEqual(self.pixel((0, 0), 2, 3), va.TRANSPARENT)
        self.assertEqual(self.editor.colors.history, [])

    def test_a_used_color_enters_the_history(self):
        self.select((0, 0))
        self.editor.colors.set_color(RED, "primary")
        self.drag(self.position(0, 0), self.position(1, 0))
        self.assertEqual(self.editor.colors.history, [RED])

    def test_a_stroke_that_changes_nothing_adds_no_history_entry_and_no_undo_step(self):
        self.select((0, 0))
        self.editor.colors.set_color(BLUE, "primary")
        self.drag(self.position(0, 0), self.position(3, 0))
        self.assertEqual(self.editor.colors.history, [])
        self.assertFalse(self.project.history.can_undo(self.project.shown_records()))

    def test_clicking_a_history_swatch_selects_that_color_and_right_click_sets_the_secondary(self):
        self.editor.colors.remember(RED)
        self.editor.colors.remember(GREEN)
        swatches = self.editor.areas.history
        self.send("down", swatches[1].center)
        self.send("up", swatches[1].center)
        self.assertEqual(self.editor.colors.get("primary"), RED)
        self.send("down", swatches[0].center, 3)
        self.send("up", swatches[0].center, 3)
        self.assertEqual(self.editor.colors.get("secondary"), GREEN)

    def test_brackets_resize_the_brush_and_shift_and_alt_choose_the_axis(self):
        self.select((0, 0))
        self.key(va.pygame.K_RIGHTBRACKET)
        self.assertEqual(self.editor.settings.brush_sizes["pencil"], (2, 2))
        self.key(va.pygame.K_RIGHTBRACKET, va.pygame.KMOD_SHIFT)
        self.assertEqual(self.editor.settings.brush_sizes["pencil"], (3, 2))
        self.key(va.pygame.K_RIGHTBRACKET, va.pygame.KMOD_ALT)
        self.assertEqual(self.editor.settings.brush_sizes["pencil"], (3, 3))
        self.key(va.pygame.K_LEFTBRACKET, va.pygame.KMOD_SHIFT)
        self.key(va.pygame.K_LEFTBRACKET, va.pygame.KMOD_ALT)
        self.assertEqual(self.editor.settings.brush_sizes["pencil"], (2, 2))
        self.key(va.pygame.K_LEFTBRACKET)
        self.key(va.pygame.K_LEFTBRACKET)
        self.assertEqual(self.editor.settings.brush_sizes["pencil"], (1, 1))

    def test_a_bigger_brush_paints_a_bigger_area_with_the_even_size_rule(self):
        self.select((0, 0))
        self.editor.settings.brush_sizes["pencil"] = (2, 3)
        self.editor.colors.set_color(RED, "primary")
        self.send("down", self.position(4, 4))
        self.send("up", self.position(4, 4))
        painted = {(c, r) for r in range(8) for c in range(8) if self.pixel((0, 0), c, r) == RED}
        self.assertEqual(painted, {(3, 3), (4, 3), (3, 4), (4, 4), (3, 5), (4, 5)})

    def test_line_width_changes_with_brackets_for_line_tools_only(self):
        self.select((0, 0))
        self.click_tool("line")
        self.key(va.pygame.K_RIGHTBRACKET)
        self.key(va.pygame.K_RIGHTBRACKET, va.pygame.KMOD_SHIFT)
        self.assertEqual(self.editor.settings.line_width, 2)
        self.assertEqual(self.editor.settings.brush_sizes["pencil"], (1, 1))

    def test_a_line_is_drawn_on_release_between_the_press_and_release_points(self):
        self.select((0, 0))
        self.click_tool("horizontal_line")
        self.editor.colors.set_color(RED, "primary")
        self.send("down", self.position(1, 4))
        self.move(self.position(6, 7))
        self.assertEqual(self.pixel((0, 0), 3, 4), BLUE, "a preview is not yet an edit")
        self.send("up", self.position(6, 7))
        self.assertEqual({c for c in range(8) if self.pixel((0, 0), c, 4) == RED}, set(range(1, 7)))
        self.assertEqual(self.pixel((0, 0), 3, 7), BLUE)

    def test_escape_cancels_a_stroke_in_progress(self):
        self.select((0, 0))
        self.send("down", self.position(1, 1))
        self.move(self.position(5, 1))
        self.key(va.pygame.K_ESCAPE)
        self.send("up", self.position(5, 1))
        self.assertFalse(self.project.tiles[(0, 0)].is_modified)

    def test_fill_bucket_fills_the_clicked_region(self):
        self.select((0, 0))
        self.drag(self.position(4, 0), self.position(4, 7))  # a black column splits nothing yet
        self.click_tool("fill")
        self.editor.colors.set_color(RED, "primary")
        self.send("down", self.position(0, 0))
        self.send("up", self.position(0, 0))
        self.assertEqual(self.pixel((0, 0), 0, 0), RED)
        self.assertEqual(self.pixel((0, 0), 3, 7), RED)
        self.assertEqual(self.pixel((0, 0), 4, 3), (0, 0, 0, 255))
        self.assertEqual(self.pixel((0, 0), 5, 3), BLUE)

    def test_eye_dropper_picks_into_the_slot_of_the_button_and_follows_a_drag(self):
        self.select((0, 0), (1, 0))
        self.click_tool("eyedropper")
        self.send("down", self.position(1, 1))
        self.assertEqual(self.editor.colors.get("primary"), BLUE)
        self.move(self.position(10, 1))
        self.assertEqual(self.editor.colors.get("primary"), GREEN)
        self.send("up", self.position(10, 1))
        self.send("down", self.position(2, 2), 3)
        self.send("up", self.position(2, 2), 3)
        self.assertEqual(self.editor.colors.get("secondary"), BLUE)
        self.assertFalse(self.project.tiles[(0, 0)].is_modified)

    def test_a_stroke_across_two_tiles_edits_both_and_is_undone_together(self):
        self.select((0, 0), (1, 0))
        self.editor.colors.set_color(RED, "primary")
        self.drag(self.position(5, 2), self.position(10, 2))
        self.assertEqual(self.pixel((0, 0), 6, 2), RED)
        self.assertEqual(self.pixel((1, 0), 1, 2), RED)
        self.key(va.pygame.K_z, va.pygame.KMOD_CTRL)
        self.assertFalse(self.project.tiles[(0, 0)].is_modified or self.project.tiles[(1, 0)].is_modified)

    def test_tiles_far_apart_show_an_empty_place_between_that_cannot_be_drawn_on(self):
        self.select((0, 0), (3, 3))
        self.assertEqual(self.editor.layout.pixels.shape, (32, 32, 4))
        self.editor.colors.set_color(RED, "primary")
        self.drag(self.position(0, 0), self.position(31, 31))
        self.assertEqual(self.pixel((0, 0), 5, 5), RED)
        self.assertEqual(self.pixel((3, 3), 7, 7), RED)
        self.assertEqual(self.project.tiles[(0, 0)].pixels.shape, (8, 8, 4))
        self.application.draw()

    def test_a_tile_that_cannot_be_edited_leaves_the_canvas_empty_with_the_reason(self):
        project = self.make_project({(0, 0): ("gone.png", None)}, tile_size=8)
        application = va.Application(project, va.pygame.Surface((1280, 800)))
        project.select_only((0, 0))
        application.draw()
        self.assertIsNone(application.editor.layout)
        self.assertIn("missing", application.editor.message_for_empty_canvas())

    def test_the_wheel_zooms_around_the_pointer_and_the_middle_button_pans(self):
        self.select((0, 0))
        position = self.position(3, 3)
        zoom = self.editor.view.zoom
        self.application.pointer = position
        self.application.handle_event(va.pygame.event.Event(va.pygame.MOUSEWHEEL, x=0, y=1))
        self.assertEqual(self.editor.view.zoom, zoom + 1)
        self.assertEqual(self.editor.view.pixel_at(position), (3, 3))
        origin = (self.editor.view.origin_x, self.editor.view.origin_y)
        self.send("down", position, 2)
        self.move((position[0] + 30, position[1] + 10))
        self.send("up", (position[0] + 30, position[1] + 10), 2)
        self.assertEqual((self.editor.view.origin_x, self.editor.view.origin_y), (origin[0] + 30, origin[1] + 10))
        self.assertFalse(self.project.tiles[(0, 0)].is_modified)

    def test_the_view_is_fitted_again_only_when_the_selection_changes(self):
        self.select((0, 0))
        self.editor.view.zoom_by(2, self.position(0, 0))
        zoomed = self.editor.view.zoom
        self.drag(self.position(1, 1), self.position(2, 1))
        self.assertEqual(self.editor.view.zoom, zoomed, "editing keeps the zoom")
        self.select((1, 0))
        self.assertNotEqual(self.editor.view.zoom, zoomed)

    def test_dragging_in_the_color_square_hue_bar_and_alpha_bar_sets_the_color(self):
        areas = self.editor.areas
        self.send("down", (areas.hue_bar.centerx, areas.hue_bar.y + areas.hue_bar.height // 3))
        self.send("up", areas.hue_bar.center)
        hue = self.editor.colors.get_hsva()[0]
        self.assertAlmostEqual(hue, (areas.hue_bar.height // 3) / (areas.hue_bar.height - 1), places=2)
        self.send("down", (areas.square.right - 1, areas.square.y))
        self.assertEqual(self.editor.colors.get_hsva()[1:3], (1.0, 1.0))
        self.move((areas.square.x, areas.square.bottom - 1))
        self.assertEqual(self.editor.colors.get_hsva()[1:3], (0.0, 0.0))
        self.send("up", (areas.square.x, areas.square.bottom - 1))
        self.send("down", (areas.alpha_bar.centerx, areas.alpha_bar.centery))
        self.send("up", (areas.alpha_bar.centerx, areas.alpha_bar.centery))
        self.assertAlmostEqual(self.editor.colors.get()[3] / 255, 0.5, delta=0.03)

    def test_the_alpha_of_the_color_is_written_to_the_tile(self):
        self.select((0, 0))
        self.editor.colors.set_color((10, 20, 30, 40), "primary")
        self.send("down", self.position(2, 2))
        self.send("up", self.position(2, 2))
        self.assertEqual(self.pixel((0, 0), 2, 2), (10, 20, 30, 40))

    def test_typing_a_hex_color_sets_the_active_slot(self):
        areas = self.editor.areas
        self.send("down", areas.secondary_swatch.center)
        self.send("up", areas.secondary_swatch.center)
        self.send("down", areas.hex_field.center)
        self.assertTrue(self.editor.hex_focused)
        self.editor.hex_field.select_all()
        self.application.handle_event(va.pygame.event.Event(va.pygame.TEXTINPUT, text="#ff8000"))
        self.key(va.pygame.K_RETURN)
        self.assertEqual(self.editor.colors.get("secondary"), (255, 128, 0, 255))
        self.assertFalse(self.editor.hex_focused)

    def test_a_bad_hex_color_is_flagged_and_keeps_the_field_open(self):
        self.send("down", self.editor.areas.hex_field.center)
        self.editor.hex_field.set_text("#zz")
        self.key(va.pygame.K_RETURN)
        self.assertTrue(self.editor.hex_invalid)
        self.assertTrue(self.editor.hex_focused)
        self.application.draw()
        self.key(va.pygame.K_ESCAPE)
        self.assertFalse(self.editor.hex_focused)

    def test_clicking_away_applies_a_typed_color(self):
        self.send("down", self.editor.areas.hex_field.center)
        self.editor.hex_field.set_text("#102030")
        self.send("down", self.application.atlas_panel.rect.center)
        self.assertEqual(self.editor.colors.get(), (16, 32, 48, 255))

    def test_ctrl_s_saves_the_shown_modified_tiles_and_ctrl_shift_s_saves_all(self):
        self.select((0, 0))
        self.drag(self.position(0, 0), self.position(1, 0))
        self.select((1, 0))
        self.drag(self.position(0, 0), self.position(1, 0))
        self.select((0, 0))
        self.key(va.pygame.K_s, va.pygame.KMOD_CTRL)
        self.assertEqual(va.read_image_pixels(self.folder / "a.png")[0, 0].tolist(), [0, 0, 0, 255])
        self.assertEqual(va.read_image_pixels(self.folder / "b.png")[0, 0].tolist(), list(GREEN))
        self.assertIn("Saved 1 tile", self.application.message)
        self.key(va.pygame.K_s, va.pygame.KMOD_CTRL | va.pygame.KMOD_SHIFT)
        self.assertEqual(va.read_image_pixels(self.folder / "b.png")[0, 0].tolist(), [0, 0, 0, 255])
        self.assertEqual(self.project.unsaved_tiles(), [])

    def test_saving_with_nothing_to_save_says_so(self):
        self.select((0, 0))
        self.key(va.pygame.K_s, va.pygame.KMOD_CTRL)
        self.assertEqual(self.application.message, "Nothing to save.")

    def test_editing_a_new_tile_keeps_it_in_atlas_json_and_a_failed_write_is_reported(self):
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(solid_tile(BLUE, 8))}, tile_size=8)
        application = va.Application(project, va.pygame.Surface((1280, 800)))
        editor = application.editor
        project.select_only((0, 0))
        editor.sync()
        position = editor.view.pixel_rectangle(2, 2).center
        application.handle_event(va.pygame.event.Event(va.pygame.MOUSEBUTTONDOWN, pos=position, button=1))
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            application.handle_event(va.pygame.event.Event(va.pygame.MOUSEBUTTONUP, pos=position, button=1))
        self.assertIn("Could not save", application.message)
        self.assertTrue(numpy.all(project.tiles[(0, 0)].pixels == BLUE))
        application.handle_event(va.pygame.event.Event(va.pygame.MOUSEBUTTONDOWN, pos=position, button=1))
        application.handle_event(va.pygame.event.Event(va.pygame.MOUSEBUTTONUP, pos=position, button=1))
        stored = va.decode_tile_data(va.read_atlas_file(self.folder / "atlas.json").coordinates_to_data[(0, 0)], 8)
        self.assertEqual(tuple(stored[2, 2]), (0, 0, 0, 255))

    def test_every_tool_draws_its_outline_and_the_window_draws_without_error(self):
        self.select((0, 0), (1, 0))
        self.editor.pointer = self.position(3, 3)
        for tool in va.TOOL_LABELS:
            self.click_tool(tool)
            self.application.draw()
        self.editor.pointer = (-5, -5)
        self.application.draw()

    def test_the_canvas_picture_shows_the_pixels_and_hatches_empty_places(self):
        self.select((0, 0), (3, 3))
        self.application.draw()
        surface = self.application.surface
        view = self.editor.view
        self.assertEqual(tuple(surface.get_at(view.pixel_rectangle(2, 2).center))[:3], BLUE[:3])
        self.assertEqual(tuple(surface.get_at(view.pixel_rectangle(26, 26).center))[:3], RED[:3])
        hatched = surface.get_at(view.pixel_rectangle(20, 4).center)
        self.assertLess(sum(tuple(hatched)[:3]), 200, "an empty place is dark, not a tile")
        self.assertNotEqual(tuple(hatched)[:3], BLUE[:3])


    # Phase 4: the noise brush, the air brush and the rectangle selection -------------------------------------

    def region(self, coordinate, left, top, right, bottom):
        return self.project.tiles[coordinate].pixels[top:bottom, left:right].copy()

    def select_rectangle(self, first, second):
        self.click_tool("select")
        self.drag(self.position(*first), self.position(*second))

    def paste_picture(self, image):
        self.clipboard.image = image
        self.key(va.pygame.K_v, va.pygame.KMOD_CTRL)

    def test_the_noise_brush_draws_with_the_chosen_color_and_leaves_the_rest(self):
        self.select((0, 0))
        self.editor.random_source = ScriptedRandom(0.0)
        self.click_tool("noise_brush")
        self.editor.colors.set_color(RED, "primary")
        self.send("down", self.position(4, 4))
        self.send("up", self.position(4, 4))
        painted = {(c, r) for r in range(8) for c in range(8) if self.pixel((0, 0), c, r) == RED}
        self.assertEqual(painted, {(4 + f.column_offset, 4 + f.row_offset) for f in va.brush_footprint(5, "circle")})
        self.assertEqual(self.editor.colors.history, [RED])

    def test_the_air_brush_draws_a_soft_spot_in_one_undo_step(self):
        self.select((0, 0))
        self.click_tool("airbrush")
        self.editor.colors.set_color(RED, "primary")
        self.editor.settings.diameters["airbrush"] = 5
        self.editor.settings.fractions["airbrush_hardness"] = 0.0
        self.drag(self.position(2, 4), self.position(5, 4))
        self.assertEqual(self.pixel((0, 0), 4, 4), RED)
        self.assertNotIn(self.pixel((0, 0), 4, 2), (RED, BLUE), "the edge is a mix")
        self.key(va.pygame.K_z, va.pygame.KMOD_CTRL)
        self.assertFalse(self.project.tiles[(0, 0)].is_modified)

    def test_brackets_change_the_diameter_of_the_brush_that_is_active(self):
        self.select((0, 0))
        self.click_tool("noise_brush")
        self.key(va.pygame.K_RIGHTBRACKET)
        self.key(va.pygame.K_RIGHTBRACKET, va.pygame.KMOD_SHIFT)
        self.assertEqual(self.editor.settings.diameters, {"noise_brush": 6, "airbrush": 9})
        self.click_tool("airbrush")
        self.key(va.pygame.K_LEFTBRACKET)
        self.assertEqual(self.editor.settings.diameters, {"noise_brush": 6, "airbrush": 8})

    def test_only_the_two_brushes_have_controls_below_the_tools(self):
        for tool in va.TOOL_LABELS:
            self.click_tool(tool)
            expected = {"noise_brush": 3, "airbrush": 2}.get(tool, 0)
            self.assertEqual(len(self.editor.control_rectangles()), expected, tool)

    def test_the_controls_fit_inside_the_row_without_overlapping(self):
        for tool in ("noise_brush", "airbrush"):
            self.click_tool(tool)
            rectangles = list(self.editor.control_rectangles().values())
            for rectangle in rectangles:
                self.assertTrue(self.editor.areas.controls.contains(rectangle), tool)
            for index, first in enumerate(rectangles):
                for second in rectangles[index + 1:]:
                    self.assertFalse(first.colliderect(second), tool)

    def test_dragging_in_a_slider_sets_its_value_and_does_not_draw(self):
        self.select((0, 0))
        self.click_tool("noise_brush")
        track = self.editor.control_rectangles()["noise_density"]
        self.send("down", (track.x + (track.width - 1) // 4, track.centery))
        self.assertAlmostEqual(self.editor.settings.fractions["noise_density"], 0.25, delta=0.02)
        self.move((track.right + 50, track.centery))
        self.assertEqual(self.editor.settings.fractions["noise_density"], 1.0)
        self.move((track.x - 50, track.centery))
        self.assertEqual(self.editor.settings.fractions["noise_density"], 0.0)
        self.send("up", (track.x - 50, track.centery))
        self.move((track.centerx, track.centery))
        self.assertEqual(self.editor.settings.fractions["noise_density"], 0.0, "released: the slider no longer follows")
        self.assertFalse(self.project.tiles[(0, 0)].is_modified)
        self.assertEqual(self.editor.settings.fractions["noise_opacity"], 1.0)

    def test_the_air_brush_sliders_are_hardness_and_opacity(self):
        self.click_tool("airbrush")
        rectangles = self.editor.control_rectangles()
        self.assertEqual(list(rectangles), ["airbrush_hardness", "airbrush_opacity"])
        track = rectangles["airbrush_opacity"]
        self.send("down", (track.x + (track.width - 1) // 2, track.centery))
        self.send("up", (track.x + (track.width - 1) // 2, track.centery))
        self.assertAlmostEqual(self.editor.settings.fractions["airbrush_opacity"], 0.5, delta=0.02)

    def test_the_shape_dropdown_opens_offers_both_shapes_and_applies_the_choice(self):
        self.select((0, 0))
        self.click_tool("noise_brush")
        dropdown = self.editor.control_rectangles()["noise_shape"]
        self.send("down", dropdown.center)
        self.send("up", dropdown.center)
        self.assertTrue(self.editor.shape_dropdown_open)
        options = self.editor.shape_option_rectangles()
        self.assertEqual(list(options), list(va.BRUSH_SHAPES))
        self.send("down", options["square"].center)
        self.send("up", options["square"].center)
        self.assertEqual(self.editor.settings.noise_shape, "square")
        self.assertFalse(self.editor.shape_dropdown_open)
        self.assertFalse(self.project.tiles[(0, 0)].is_modified, "the click chose a shape, it did not draw")
        self.application.draw()

    def test_clicking_elsewhere_closes_the_dropdown_without_a_change_or_a_stroke(self):
        self.select((0, 0))
        self.click_tool("noise_brush")
        dropdown = self.editor.control_rectangles()["noise_shape"]
        self.send("down", dropdown.center)
        self.send("up", dropdown.center)
        self.send("down", self.position(3, 3))
        self.send("up", self.position(3, 3))
        self.assertFalse(self.editor.shape_dropdown_open)
        self.assertEqual(self.editor.settings.noise_shape, "circle")
        self.assertFalse(self.project.tiles[(0, 0)].is_modified)

    def test_the_dropdown_closes_when_another_tool_is_chosen(self):
        self.click_tool("noise_brush")
        dropdown = self.editor.control_rectangles()["noise_shape"]
        self.send("down", dropdown.center)
        self.send("up", dropdown.center)
        self.click_tool("pencil")
        self.assertFalse(self.editor.shape_dropdown_open)
        self.assertEqual(self.editor.tool, "pencil")

    def test_dragging_with_the_select_tool_selects_the_rectangle_between_both_pixels(self):
        self.select((0, 0))
        self.select_rectangle((1, 2), (4, 5))
        self.assertEqual(self.editor.selection_rectangle, (1, 2, 5, 6))
        self.drag(self.position(4, 5), self.position(1, 2))
        self.assertEqual(self.editor.selection_rectangle, (1, 2, 5, 6))
        self.assertFalse(self.project.tiles[(0, 0)].is_modified)
        self.assertFalse(self.project.history.can_undo(self.project.shown_records()))

    def test_a_selection_dragged_past_the_canvas_is_clipped_to_it(self):
        self.select((0, 0))
        self.select_rectangle((2, 2), (12, 3))
        self.assertEqual(self.editor.selection_rectangle, (2, 2, 8, 4))

    def test_a_click_without_dragging_clears_the_selection(self):
        self.select((0, 0))
        self.select_rectangle((1, 1), (3, 3))
        self.send("down", self.position(5, 5))
        self.send("up", self.position(5, 5))
        self.assertIsNone(self.editor.selection_rectangle)

    def test_the_selection_is_shown_while_it_is_dragged(self):
        self.select((0, 0))
        self.click_tool("select")
        self.send("down", self.position(1, 1))
        self.move(self.position(3, 3))
        self.assertEqual(self.editor.selection_rectangle, (1, 1, 4, 4))
        self.application.draw()
        self.send("up", self.position(3, 3))

    def test_the_right_button_does_not_select(self):
        self.select((0, 0))
        self.click_tool("select")
        self.drag(self.position(1, 1), self.position(3, 3), button=3)
        self.assertIsNone(self.editor.selection_rectangle)

    def test_ctrl_a_selects_the_whole_canvas_and_switches_to_the_select_tool(self):
        self.select((0, 0), (1, 0))
        self.assertEqual(self.editor.tool, "pencil")
        self.key(va.pygame.K_a, va.pygame.KMOD_CTRL)
        self.assertEqual(self.editor.tool, "select")
        self.assertEqual(self.editor.selection_rectangle, (0, 0, 16, 8))

    def test_ctrl_a_with_no_tile_shown_selects_nothing(self):
        self.key(va.pygame.K_a, va.pygame.KMOD_CTRL)
        self.assertIsNone(self.editor.selection_rectangle)

    def test_ctrl_c_copies_the_selected_pixels_exactly_and_changes_no_tile(self):
        self.select((0, 0))
        self.editor.colors.set_color((10, 20, 30, 40), "primary")
        self.send("down", self.position(2, 2))
        self.send("up", self.position(2, 2))
        before = self.project.tiles[(0, 0)].pixels.copy()
        self.select_rectangle((1, 1), (3, 3))
        self.key(va.pygame.K_c, va.pygame.KMOD_CTRL)
        self.assertEqual(len(self.clipboard.writes), 1)
        copied = self.clipboard.image
        self.assertEqual(copied.shape, (3, 3, 4))
        self.assertTrue(numpy.array_equal(copied, before[1:4, 1:4]))
        self.assertEqual(tuple(copied[1, 1]), (10, 20, 30, 40))
        self.assertTrue(numpy.array_equal(self.project.tiles[(0, 0)].pixels, before))
        self.assertIn("Copied", self.application.message)

    def test_ctrl_c_with_no_selection_copies_nothing(self):
        self.select((0, 0))
        sentinel = solid_tile(GREEN, 2)
        self.clipboard.image = sentinel
        self.key(va.pygame.K_c, va.pygame.KMOD_CTRL)
        self.assertEqual(self.clipboard.writes, [])
        self.assertTrue(numpy.array_equal(self.clipboard.image, sentinel))
        self.assertIsNone(self.editor.internal_image)

    def test_ctrl_x_copies_then_clears_in_one_undo_step(self):
        self.select((0, 0))
        self.select_rectangle((2, 1), (4, 2))
        self.key(va.pygame.K_x, va.pygame.KMOD_CTRL)
        self.assertEqual(self.clipboard.image.shape, (2, 3, 4))
        self.assertTrue(numpy.all(self.clipboard.image == BLUE))
        self.assertTrue(numpy.all(self.region((0, 0), 2, 1, 5, 3) == 0))
        self.assertEqual(self.pixel((0, 0), 1, 1), BLUE)
        self.assertEqual(self.pixel((0, 0), 5, 1), BLUE)
        self.key(va.pygame.K_z, va.pygame.KMOD_CTRL)
        self.assertFalse(self.project.tiles[(0, 0)].is_modified)

    def test_delete_clears_the_selection_and_leaves_the_clipboard_alone(self):
        self.select((0, 0))
        self.select_rectangle((0, 0), (1, 1))
        self.key(va.pygame.K_DELETE)
        self.assertTrue(numpy.all(self.region((0, 0), 0, 0, 2, 2) == 0))
        self.assertEqual(self.pixel((0, 0), 2, 2), BLUE)
        self.assertEqual(self.clipboard.writes, [])
        self.assertIsNone(self.clipboard.image)

    def test_delete_without_a_selection_does_nothing(self):
        self.select((0, 0))
        self.key(va.pygame.K_DELETE)
        self.assertFalse(self.project.tiles[(0, 0)].is_modified)

    def test_cutting_the_whole_canvas_keeps_the_empty_places_between_tiles_empty(self):
        self.select((0, 0), (3, 3))
        self.key(va.pygame.K_a, va.pygame.KMOD_CTRL)
        self.key(va.pygame.K_x, va.pygame.KMOD_CTRL)
        self.assertTrue(numpy.all(self.project.tiles[(0, 0)].pixels == 0))
        self.assertTrue(numpy.all(self.project.tiles[(3, 3)].pixels == 0))
        self.assertEqual(self.project.tiles[(3, 3)].pixels.shape, (8, 8, 4))
        self.assertEqual(self.clipboard.image.shape, (32, 32, 4))
        self.assertEqual(set(self.project.tiles), {(0, 0), (1, 0), (3, 3)})

    def test_a_paste_floats_over_the_canvas_until_it_is_committed(self):
        self.select((0, 0))
        picture = solid_tile((255, 0, 0, 128), 2)
        self.paste_picture(picture)
        self.assertEqual(self.editor.tool, "select")
        self.assertIsNotNone(self.editor.floating)
        self.assertEqual((self.editor.floating.column, self.editor.floating.row), (0, 0))
        self.assertFalse(self.project.tiles[(0, 0)].is_modified, "a floating paste is not yet an edit")
        self.application.draw()
        self.key(va.pygame.K_RETURN)
        expected = va.alpha_over(solid_tile(BLUE, 2), picture)
        self.assertTrue(numpy.array_equal(self.region((0, 0), 0, 0, 2, 2), expected))
        self.assertEqual(self.pixel((0, 0), 2, 0), BLUE)
        self.assertIsNone(self.editor.floating)
        self.assertTrue(self.project.tiles[(0, 0)].is_modified)

    def test_a_paste_lands_on_the_top_left_of_the_selection(self):
        self.select((0, 0))
        self.select_rectangle((3, 2), (5, 4))
        self.paste_picture(solid_tile(GREEN, 2))
        self.assertEqual((self.editor.floating.column, self.editor.floating.row), (3, 2))

    def test_a_floating_paste_is_dragged_by_the_mouse(self):
        self.select((0, 0))
        self.paste_picture(solid_tile(GREEN, 2))
        self.drag(self.position(0, 0), self.position(3, 4))
        self.assertEqual((self.editor.floating.column, self.editor.floating.row), (3, 4))
        self.key(va.pygame.K_RETURN)
        self.assertEqual(self.pixel((0, 0), 3, 4), GREEN)
        self.assertEqual(self.pixel((0, 0), 4, 5), GREEN)
        self.assertEqual(self.pixel((0, 0), 0, 0), BLUE)

    def test_escape_throws_a_floating_paste_away(self):
        self.select((0, 0))
        self.paste_picture(solid_tile(GREEN, 2))
        self.key(va.pygame.K_ESCAPE)
        self.assertIsNone(self.editor.floating)
        self.assertFalse(self.project.tiles[(0, 0)].is_modified)
        self.assertFalse(self.project.history.can_undo(self.project.shown_records()))

    def test_clicking_outside_the_floating_paste_commits_it_and_starts_a_new_selection(self):
        self.select((0, 0))
        self.paste_picture(solid_tile(GREEN, 2))
        self.send("down", self.position(6, 6))
        self.assertIsNone(self.editor.floating)
        self.assertEqual(self.pixel((0, 0), 1, 1), GREEN)
        self.move(self.position(7, 7))
        self.send("up", self.position(7, 7))
        self.assertEqual(self.editor.selection_rectangle, (6, 6, 8, 8))

    def test_choosing_another_tool_commits_the_paste_and_drops_the_selection(self):
        self.select((0, 0))
        self.paste_picture(solid_tile(GREEN, 2))
        self.click_tool("pencil")
        self.assertEqual(self.pixel((0, 0), 0, 0), GREEN)
        self.assertIsNone(self.editor.floating)
        self.assertIsNone(self.editor.selection_rectangle)

    def test_choosing_other_tiles_in_the_atlas_commits_the_paste_to_the_old_tiles_first(self):
        self.select((0, 0))
        self.paste_picture(solid_tile(RED, 2))
        position = self.application.atlas_panel.tile_screen_rect((1, 0)).center
        self.send("down", position)
        self.send("up", position)
        self.assertEqual(self.pixel((0, 0), 0, 0), RED)
        self.assertEqual(self.pixel((1, 0), 0, 0), GREEN, "the tile that was chosen afterwards is not painted")
        self.assertEqual(self.project.selection, {(1, 0)})
        self.assertIsNone(self.editor.floating)
        self.assertIsNone(self.editor.selection_rectangle)

    def test_undo_while_a_paste_floats_throws_only_the_paste_away(self):
        self.select((0, 0))
        self.editor.colors.set_color(RED, "primary")
        self.send("down", self.position(7, 7))
        self.send("up", self.position(7, 7))
        self.paste_picture(solid_tile(GREEN, 2))
        self.key(va.pygame.K_z, va.pygame.KMOD_CTRL)
        self.assertIsNone(self.editor.floating)
        self.assertEqual(self.pixel((0, 0), 7, 7), RED, "the earlier edit is still there")
        self.assertEqual(self.pixel((0, 0), 0, 0), BLUE)

    def test_a_committed_paste_is_one_undo_step(self):
        self.select((0, 0), (1, 0))
        self.paste_picture(solid_tile(RED, 16))
        self.key(va.pygame.K_RETURN)
        self.assertEqual(self.pixel((0, 0), 7, 7), RED)
        self.assertEqual(self.pixel((1, 0), 7, 7), RED)
        self.key(va.pygame.K_z, va.pygame.KMOD_CTRL)
        self.assertFalse(self.project.tiles[(0, 0)].is_modified or self.project.tiles[(1, 0)].is_modified)

    def test_a_paste_larger_than_the_canvas_is_clipped(self):
        self.select((0, 0))
        self.paste_picture(solid_tile(GREEN, 20))
        self.key(va.pygame.K_RETURN)
        self.assertTrue(numpy.all(self.project.tiles[(0, 0)].pixels == GREEN))
        self.assertEqual(self.project.tiles[(0, 0)].pixels.shape, (8, 8, 4))

    def test_a_paste_over_empty_places_changes_only_the_tiles(self):
        self.select((0, 0), (3, 3))
        self.paste_picture(solid_tile(GREEN, 40))
        self.key(va.pygame.K_RETURN)
        self.assertTrue(numpy.all(self.project.tiles[(0, 0)].pixels == GREEN))
        self.assertTrue(numpy.all(self.project.tiles[(3, 3)].pixels == GREEN))
        self.assertEqual(set(self.project.tiles), {(0, 0), (1, 0), (3, 3)})

    def test_a_paste_that_changes_nothing_leaves_no_undo_step(self):
        self.select((0, 0))
        self.paste_picture(solid_tile(TRANSPARENT, 3))
        self.key(va.pygame.K_RETURN)
        self.assertFalse(self.project.tiles[(0, 0)].is_modified)
        self.assertFalse(self.project.history.can_undo(self.project.shown_records()))

    def test_with_nothing_to_paste_the_user_is_told(self):
        self.select((0, 0))
        self.key(va.pygame.K_v, va.pygame.KMOD_CTRL)
        self.assertIsNone(self.editor.floating)
        self.assertIn("Nothing to paste", self.application.message)

    def test_a_paste_uses_the_system_clipboard_first_and_the_copy_made_here_otherwise(self):
        self.select((0, 0))
        self.select_rectangle((0, 0), (1, 1))
        self.key(va.pygame.K_c, va.pygame.KMOD_CTRL)
        self.clipboard.image = solid_tile(GREEN, 3)
        self.key(va.pygame.K_v, va.pygame.KMOD_CTRL)
        self.assertEqual(self.editor.floating.pixels.shape, (3, 3, 4), "something copied elsewhere since")
        self.key(va.pygame.K_ESCAPE)
        self.clipboard.image = None
        self.key(va.pygame.K_v, va.pygame.KMOD_CTRL)
        self.assertEqual(self.editor.floating.pixels.shape, (2, 2, 4), "the system clipboard has no picture")

    def test_a_copy_the_system_clipboard_refused_still_wins_over_an_older_picture_there(self):
        self.select((0, 0))
        self.clipboard.image = solid_tile(GREEN, 3)
        self.clipboard.can_write = False
        self.select_rectangle((0, 0), (1, 1))
        self.key(va.pygame.K_c, va.pygame.KMOD_CTRL)
        self.assertIn("inside this program", self.application.message)
        self.key(va.pygame.K_v, va.pygame.KMOD_CTRL)
        self.assertEqual(self.editor.floating.pixels.shape, (2, 2, 4))
        self.key(va.pygame.K_ESCAPE)
        self.clipboard.can_write = True
        self.select_rectangle((0, 0), (2, 2))
        self.key(va.pygame.K_c, va.pygame.KMOD_CTRL)
        self.clipboard.image = solid_tile(GREEN, 5)
        self.key(va.pygame.K_v, va.pygame.KMOD_CTRL)
        self.assertEqual(self.editor.floating.pixels.shape, (5, 5, 4), "after a good copy the system clipboard leads again")

    def test_the_selection_is_dropped_when_other_tiles_are_chosen_but_kept_through_an_edit(self):
        self.select((0, 0))
        self.select_rectangle((1, 1), (3, 3))
        self.key(va.pygame.K_DELETE)
        self.assertEqual(self.editor.selection_rectangle, (1, 1, 4, 4))
        self.select((1, 0))
        self.assertIsNone(self.editor.selection_rectangle)

    def test_the_window_draws_the_selection_and_the_floating_paste(self):
        self.select((0, 0))
        self.select_rectangle((1, 1), (3, 3))
        self.application.draw()
        self.paste_picture(solid_tile(RED, 2))
        self.application.draw()
        center = self.editor.view.pixel_rectangle(1, 1).center
        self.assertEqual(tuple(self.application.surface.get_at(center))[:3], RED[:3], "the paste is shown over the tile")
        self.assertEqual(self.pixel((0, 0), 1, 1), BLUE)

    def test_delete_throws_a_floating_paste_away_and_clears_nothing(self):
        self.select((0, 0))
        self.select_rectangle((1, 1), (3, 3))
        self.paste_picture(solid_tile(GREEN, 2))
        self.key(va.pygame.K_DELETE)
        self.assertIsNone(self.editor.floating)
        self.assertEqual(self.pixel((0, 0), 1, 1), BLUE)
        self.assertFalse(self.project.tiles[(0, 0)].is_modified)

    def test_a_paste_with_nothing_selected_lands_on_the_top_left_of_the_visible_canvas(self):
        self.select((0, 0), (1, 0))
        view = self.editor.view
        view.zoom_by(10, self.position(0, 0))
        view.origin_x = view.rect.x - 3 * view.zoom
        view.origin_y = view.rect.y - 2 * view.zoom
        self.paste_picture(solid_tile(GREEN, 2))
        self.assertEqual((self.editor.floating.column, self.editor.floating.row), (3, 2))

    def test_a_selection_dragged_entirely_outside_the_canvas_selects_nothing(self):
        self.select((0, 0))
        self.select_rectangle((-1, 3), (-2, 5))
        self.assertIsNone(self.editor.selection_rectangle)

    def test_the_floating_paste_is_grabbed_on_its_last_pixel_but_not_one_past_it(self):
        self.select((0, 0))
        self.paste_picture(solid_tile(GREEN, 2))
        self.send("down", self.position(1, 1))
        self.assertIsNotNone(self.editor.floating_drag)
        self.send("up", self.position(1, 1))
        self.send("down", self.position(2, 0))
        self.assertIsNone(self.editor.floating, "one pixel to the right is outside: the paste is placed")
        self.send("up", self.position(2, 0))
        self.paste_picture(solid_tile(GREEN, 2))
        self.send("down", self.position(0, 2))
        self.assertIsNone(self.editor.floating, "one pixel below is outside")
        self.send("up", self.position(0, 2))

    def test_a_keyboard_shortcut_that_changes_the_tool_closes_the_shape_menu(self):
        self.select((0, 0))
        self.click_tool("noise_brush")
        menu = self.editor.control_rectangles()["noise_shape"]
        self.send("down", menu.center)
        self.send("up", menu.center)
        self.assertTrue(self.editor.shape_dropdown_open)
        self.key(va.pygame.K_a, va.pygame.KMOD_CTRL)
        self.assertEqual(self.editor.tool, "select")
        self.assertFalse(self.editor.shape_dropdown_open)

    def test_a_tool_that_needs_no_tile_works_without_one_shown(self):
        for tool in ("noise_brush", "airbrush", "select"):
            self.click_tool(tool)
            self.send("down", self.position(3, 3))
            self.send("up", self.position(3, 3))
            self.application.draw()


    # Phase 5: renaming and the quit dialog -----------------------------------------------------------------

    def click_name(self):
        center = self.editor.areas.header.center
        self.send("down", center)
        self.send("up", center)

    def type_text(self, text):
        self.application.handle_event(va.pygame.event.Event(va.pygame.TEXTINPUT, text=text))

    def rename_to(self, name):
        self.click_name()
        self.editor.name_field.select_all()
        self.type_text(name)

    def png_names(self):
        return sorted(entry.name for entry in self.folder.iterdir() if entry.suffix == ".png")

    def test_clicking_the_name_of_a_single_tile_starts_editing_it(self):
        self.select((0, 0))
        self.click_name()
        self.assertTrue(self.editor.name_focused)
        self.assertEqual(self.editor.name_field.text, "a", "the extension is not part of the field")
        self.assertEqual(self.editor.name_extension, ".png")

    def test_the_name_of_several_tiles_or_of_none_cannot_be_edited(self):
        self.select((0, 0), (1, 0))
        self.click_name()
        self.assertFalse(self.editor.name_focused)
        self.select()
        self.click_name()
        self.assertFalse(self.editor.name_focused)

    def test_the_name_of_a_tile_that_cannot_be_edited_cannot_be_edited(self):
        project = self.make_project({(0, 0): ("gone.png", None)}, tile_size=8)
        application = va.Application(project, va.pygame.Surface((1280, 800)))
        project.select_only((0, 0))
        application.editor.sync()
        center = application.editor.areas.header.center
        application.handle_event(va.pygame.event.Event(va.pygame.MOUSEBUTTONDOWN, pos=center, button=1))
        self.assertFalse(application.editor.name_focused)

    def test_typing_a_name_and_pressing_enter_renames_the_file_at_once(self):
        self.select((0, 0))
        self.rename_to("stone")
        self.key(va.pygame.K_RETURN)
        self.assertEqual(self.png_names(), ["b.png", "c.png", "stone.png"])
        self.assertEqual(self.editor.header_text(), "stone.png")
        self.assertFalse(self.editor.name_focused)
        self.assertEqual(self.saved().coordinates_to_names[(0, 0)], "stone.png")
        self.assertIn("Renamed", self.application.message)

    def test_characters_a_file_name_cannot_contain_are_not_typed(self):
        self.select((0, 0))
        self.rename_to("x/y:z*")
        self.assertEqual(self.editor.name_field.text, "xyz")

    def test_a_name_that_is_not_allowed_keeps_the_field_open_with_the_reason(self):
        self.select((0, 0))
        self.rename_to("")
        self.key(va.pygame.K_RETURN)
        self.assertTrue(self.editor.name_focused)
        self.assertTrue(self.editor.name_invalid)
        self.assertIn("nothing before the extension", self.application.message)
        self.assertEqual(self.png_names(), ["a.png", "b.png", "c.png"])
        self.application.draw()
        self.key(va.pygame.K_ESCAPE)
        self.assertFalse(self.editor.name_focused)
        self.assertEqual(self.editor.header_text(), "a.png")

    def test_a_name_that_another_file_has_is_refused_and_that_file_is_not_touched(self):
        self.select((0, 0))
        self.rename_to("B")
        self.key(va.pygame.K_RETURN)
        self.assertTrue(self.editor.name_invalid)
        self.assertIn("already used", self.application.message)
        self.assertEqual(self.pixel((1, 0), 0, 0), GREEN)
        self.assertEqual(va.read_image_pixels(self.folder / "b.png")[0, 0].tolist(), list(GREEN))

    def test_typing_again_clears_the_flag(self):
        self.select((0, 0))
        self.rename_to("")
        self.key(va.pygame.K_RETURN)
        self.type_text("x")
        self.assertFalse(self.editor.name_invalid)

    def test_clicking_away_applies_a_valid_name(self):
        self.select((0, 0))
        self.rename_to("stone")
        self.send("down", self.application.unplaced_panel.rect.center)
        self.assertEqual(self.png_names(), ["b.png", "c.png", "stone.png"])
        self.assertFalse(self.editor.name_focused)

    def test_clicking_away_from_a_name_that_is_not_allowed_drops_it_and_says_why(self):
        self.select((0, 0))
        self.rename_to("")
        self.send("down", self.application.unplaced_panel.rect.center)
        self.assertEqual(self.png_names(), ["a.png", "b.png", "c.png"])
        self.assertFalse(self.editor.name_focused)
        self.assertIn("nothing before the extension", self.application.message)

    def test_clicking_another_tile_in_the_atlas_renames_the_first_before_the_selection_changes(self):
        self.select((0, 0))
        self.rename_to("stone")
        position = self.application.atlas_panel.tile_screen_rect((1, 0)).center
        self.send("down", position)
        self.send("up", position)
        self.assertEqual(self.png_names(), ["b.png", "c.png", "stone.png"])
        self.assertEqual(self.project.selection, {(1, 0)})

    def test_clicking_a_tool_or_the_canvas_while_editing_the_name_applies_it_first(self):
        self.select((0, 0))
        self.rename_to("stone")
        self.click_tool("eraser")
        self.assertEqual(self.png_names(), ["b.png", "c.png", "stone.png"])
        self.assertFalse(self.editor.name_focused)
        self.assertEqual(self.editor.tool, "eraser")
        self.click_name()
        self.editor.name_field.set_text("grass.png")
        self.send("down", self.position(3, 3))
        self.send("up", self.position(3, 3))
        self.assertFalse(self.editor.name_focused)
        self.assertIn("grass.png", self.png_names())

    def test_a_name_that_cannot_reach_the_disk_is_reported_and_dropped(self):
        self.select((0, 0))
        self.rename_to("stone")
        with mock.patch.object(va.os, "rename", side_effect=PermissionError("in use")):
            self.key(va.pygame.K_RETURN)
        self.assertIn("Could not rename", self.application.message)
        self.assertFalse(self.editor.name_focused)
        self.assertEqual(self.png_names(), ["a.png", "b.png", "c.png"])

    def test_ctrl_a_in_the_name_selects_all_of_it_and_not_the_canvas(self):
        self.select((0, 0))
        self.click_name()
        self.key(va.pygame.K_a, va.pygame.KMOD_CTRL)
        self.assertEqual(self.editor.name_field.selection, (0, 1))
        self.assertEqual(self.editor.tool, "pencil")
        self.assertIsNone(self.editor.selection_rectangle)
        self.editor.name_field.set_text("grass.top")
        self.key(va.pygame.K_a, va.pygame.KMOD_CTRL)
        self.assertEqual(self.editor.name_field.selection, (0, 9))

    def test_typing_over_the_selected_name_changes_it_and_the_extension_stays(self):
        self.select((0, 0))
        self.click_name()
        self.key(va.pygame.K_a, va.pygame.KMOD_CTRL)
        self.type_text("dirt")
        self.assertEqual(self.editor.name_field.text, "dirt")
        self.key(va.pygame.K_RETURN)
        self.assertEqual(self.png_names(), ["b.png", "c.png", "dirt.png"])

    def test_an_extension_typed_out_is_not_doubled(self):
        self.select((0, 0))
        self.rename_to("stone.png")
        self.key(va.pygame.K_RETURN)
        self.assertEqual(self.png_names(), ["b.png", "c.png", "stone.png"])

    def test_the_letter_case_of_the_extension_a_file_has_is_kept(self):
        project = self.make_project({(0, 0): ("Loud.PNG", solid_tile(BLUE, 8))}, tile_size=8)
        application = va.Application(project, va.pygame.Surface((1280, 800)))
        project.select_only((0, 0))
        application.editor.sync()
        center = application.editor.areas.header.center
        application.handle_event(va.pygame.event.Event(va.pygame.MOUSEBUTTONDOWN, pos=center, button=1))
        self.assertEqual((application.editor.name_field.text, application.editor.name_extension), ("Loud", ".PNG"))
        application.editor.name_field.set_text("Quiet")
        application.handle_event(va.pygame.event.Event(va.pygame.KEYDOWN, key=va.pygame.K_RETURN, mod=0, unicode="", scancode=0))
        self.assertEqual(project.tiles[(0, 0)].name, "Quiet.PNG")

    def test_the_extension_is_drawn_beside_the_field_and_not_inside_it(self):
        self.select((0, 0))
        self.click_name()
        field, extension = self.editor.name_field_areas()
        header = self.editor.areas.header
        self.assertTrue(header.contains(field) and header.contains(extension))
        self.assertGreaterEqual(extension.left, field.right)
        self.editor.name_field.set_text("a" * 200)
        field, extension = self.editor.name_field_areas()
        self.assertTrue(header.contains(field) and header.contains(extension))
        self.assertGreaterEqual(extension.left, field.right)

    def test_the_name_field_cannot_hold_more_than_a_file_name_has_room_for(self):
        self.select((0, 0))
        self.click_name()
        self.key(va.pygame.K_a, va.pygame.KMOD_CTRL)
        self.type_text("x" * 400)
        self.assertEqual(len(self.editor.name_field.text) + len(self.editor.name_extension), 255)

    def test_the_tools_are_icons_in_one_row_in_the_order_of_the_list(self):
        buttons = self.editor.areas.tool_buttons
        self.assertEqual(list(buttons), list(va.TOOL_LABELS))
        rectangles = list(buttons.values())
        self.assertEqual({(rectangle.y, rectangle.size) for rectangle in rectangles}, {(rectangles[0].y, rectangles[0].size)})
        for first, second in zip(rectangles, rectangles[1:]):
            self.assertLess(first.right, second.left)

    def test_resting_on_a_tool_shows_its_name(self):
        for tool, label in va.TOOL_LABELS.items():
            self.application.pointer = self.editor.areas.tool_buttons[tool].center
            self.assertEqual(self.application.hover_text(), label)
        self.application.pointer = self.editor.areas.square.center
        self.assertIsNone(self.application.hover_text())
        self.application.pointer = self.editor.areas.tool_buttons["pencil"].center
        self.application.draw()

    def test_every_tool_has_its_own_icon_that_is_drawn(self):
        blank = va.pygame.Surface((40, 36))
        blank.fill(va.Theme.BUTTON)
        blank_pixels = va.pygame.surfarray.array3d(blank).tobytes()
        pictures = {}
        for tool in va.TOOL_LABELS:
            surface = va.pygame.Surface((40, 36))
            surface.fill(va.Theme.BUTTON)
            va.draw_tool_icon(surface, tool, surface.get_rect(), va.Theme.TEXT)
            pictures[tool] = va.pygame.surfarray.array3d(surface).tobytes()
            self.assertNotEqual(pictures[tool], blank_pixels, tool)
        self.assertEqual(len(set(pictures.values())), len(pictures), "no two tools share an icon")

    def test_the_text_is_bright_the_brightest_is_white_and_the_darkest_is_still_a_light_grey(self):
        texts = [va.Theme.TEXT, va.Theme.TEXT_DIMMED, va.Theme.DISABLED_TEXT, va.Theme.ERROR]
        self.assertEqual(va.Theme.TEXT, (255, 255, 255))
        self.assertTrue(all(min(color) >= 128 for color in texts), texts)
        self.assertGreaterEqual(min(va.Theme.DISABLED_TEXT), 150)
        self.assertGreater(sum(va.Theme.TEXT), sum(va.Theme.TEXT_DIMMED))
        self.assertGreater(sum(va.Theme.TEXT_DIMMED), sum(va.Theme.DISABLED_TEXT))

    def test_keys_typed_into_the_name_do_not_reach_the_canvas(self):
        self.select((0, 0))
        self.click_name()
        self.key(va.pygame.K_RIGHTBRACKET)
        self.assertEqual(self.editor.settings.brush_sizes["pencil"], (1, 1))
        self.key(va.pygame.K_s, va.pygame.KMOD_CTRL)
        self.assertEqual(self.application.message, "")

    def test_choosing_other_tiles_without_the_mouse_ends_the_edit_without_renaming(self):
        self.select((0, 0))
        self.rename_to("stone")
        self.select((1, 0))
        self.assertFalse(self.editor.name_focused)
        self.assertEqual(self.png_names(), ["a.png", "b.png", "c.png"])

    def test_renaming_keeps_unsaved_edits_and_saving_afterwards_writes_the_new_file(self):
        self.select((0, 0))
        self.drag(self.position(0, 0), self.position(1, 0))
        self.rename_to("stone")
        self.key(va.pygame.K_RETURN)
        self.assertEqual(self.editor.header_text(), "stone.png*")
        self.key(va.pygame.K_s, va.pygame.KMOD_CTRL)
        self.assertEqual(va.read_image_pixels(self.folder / "stone.png")[0, 0].tolist(), [0, 0, 0, 255])
        self.assertEqual(self.png_names(), ["b.png", "c.png", "stone.png"])

    def test_the_window_draws_the_name_field_in_each_state(self):
        self.select((0, 0))
        self.editor.pointer = self.editor.areas.header.center
        self.application.draw()
        self.click_name()
        self.application.draw()
        self.type_text("!")
        self.editor.name_invalid = True
        self.application.draw()

    def unsaved_tiles_in_two_places(self):
        self.select((0, 0))
        self.drag(self.position(0, 0), self.position(1, 0))
        self.select((1, 0))
        self.drag(self.position(0, 0), self.position(1, 0))
        self.select()

    def quit_event(self):
        return self.application.handle_event(va.pygame.event.Event(va.pygame.QUIT))

    def press_dialog(self, name):
        center = self.application.dialog_areas()[name].center
        self.application.handle_event(va.pygame.event.Event(va.pygame.MOUSEBUTTONDOWN, pos=center, button=1))
        return self.application.handle_event(va.pygame.event.Event(va.pygame.MOUSEBUTTONUP, pos=center, button=1))

    def test_closing_with_nothing_unsaved_closes_at_once(self):
        self.assertFalse(self.quit_event())

    def test_closing_with_unsaved_changes_opens_the_dialog_instead(self):
        self.select((0, 0))
        self.drag(self.position(0, 0), self.position(1, 0))
        self.assertTrue(self.quit_event())
        self.assertIsNotNone(self.application.quit_dialog)
        self.assertEqual(self.application.quit_dialog.record.name, "a.png")
        self.application.draw()

    def test_closing_with_only_a_tile_that_has_no_file_asks_too(self):
        project = self.make_project({(0, 0): ("n.png", None)}, data={(0, 0): va.encode_tile_data(solid_tile(BLUE, 8))}, tile_size=8)
        application = va.Application(project, va.pygame.Surface((1280, 800)))
        self.assertTrue(application.handle_event(va.pygame.event.Event(va.pygame.QUIT)))
        self.assertIsNotNone(application.quit_dialog)
        application.draw()

    def test_the_save_button_writes_the_tile_and_closes(self):
        self.select((0, 0))
        self.drag(self.position(0, 0), self.position(1, 0))
        self.quit_event()
        self.assertFalse(self.press_dialog("save"))
        self.assertEqual(va.read_image_pixels(self.folder / "a.png")[0, 0].tolist(), [0, 0, 0, 255])
        self.application.draw()  # the real loop draws once more after the answer, before it ends

    def test_the_dont_save_button_closes_and_leaves_the_file(self):
        self.select((0, 0))
        self.drag(self.position(0, 0), self.position(1, 0))
        self.quit_event()
        self.assertFalse(self.press_dialog("discard"))
        self.assertEqual(va.read_image_pixels(self.folder / "a.png")[0, 0].tolist(), list(BLUE))

    def test_the_cancel_button_and_escape_keep_the_program_open_and_nothing_is_saved(self):
        self.select((0, 0))
        self.drag(self.position(0, 0), self.position(1, 0))
        self.quit_event()
        self.assertTrue(self.press_dialog("cancel"))
        self.assertIsNone(self.application.quit_dialog)
        self.assertEqual(va.read_image_pixels(self.folder / "a.png")[0, 0].tolist(), list(BLUE))
        self.quit_event()
        self.assertTrue(self.application.handle_event(va.pygame.event.Event(va.pygame.KEYDOWN, key=va.pygame.K_ESCAPE, mod=0, unicode="", scancode=0)))
        self.assertIsNone(self.application.quit_dialog)
        self.assertTrue(self.project.tiles[(0, 0)].is_modified)

    def test_enter_in_the_dialog_saves(self):
        self.select((0, 0))
        self.drag(self.position(0, 0), self.position(1, 0))
        self.quit_event()
        self.assertFalse(self.application.handle_event(va.pygame.event.Event(va.pygame.KEYDOWN, key=va.pygame.K_RETURN, mod=0, unicode="", scancode=0)))
        self.assertEqual(va.read_image_pixels(self.folder / "a.png")[0, 0].tolist(), [0, 0, 0, 255])

    def test_closing_the_window_again_while_the_dialog_is_open_changes_nothing(self):
        self.select((0, 0))
        self.drag(self.position(0, 0), self.position(1, 0))
        self.quit_event()
        dialog = self.application.quit_dialog
        self.assertTrue(self.quit_event())
        self.assertIs(self.application.quit_dialog, dialog)

    def test_nothing_behind_the_dialog_reacts_while_it_is_open(self):
        self.select((0, 0))
        self.drag(self.position(0, 0), self.position(1, 0))
        self.quit_event()
        self.click_tool("eraser")
        self.assertEqual(self.editor.tool, "pencil")
        self.send("down", self.application.atlas_panel.tile_screen_rect((1, 0)).center)
        self.assertEqual(self.project.selection, {(0, 0)})
        self.key(va.pygame.K_s, va.pygame.KMOD_CTRL | va.pygame.KMOD_SHIFT)
        self.assertEqual(va.read_image_pixels(self.folder / "a.png")[0, 0].tolist(), list(BLUE))
        self.assertIsNotNone(self.application.quit_dialog)

    def test_two_unsaved_tiles_are_asked_about_in_turn(self):
        self.unsaved_tiles_in_two_places()
        self.quit_event()
        self.assertIn("checkbox", self.application.dialog_areas())
        self.assertTrue(self.press_dialog("discard"))
        self.assertEqual(self.application.quit_dialog.record.name, "b.png")
        self.assertNotIn("checkbox", self.application.dialog_areas())
        self.assertFalse(self.press_dialog("save"))
        self.assertEqual(va.read_image_pixels(self.folder / "a.png")[0, 0].tolist(), list(BLUE))
        self.assertEqual(va.read_image_pixels(self.folder / "b.png")[0, 0].tolist(), [0, 0, 0, 255])

    def test_the_checkbox_applies_the_answer_to_every_remaining_tile(self):
        self.unsaved_tiles_in_two_places()
        self.quit_event()
        self.press_dialog("checkbox")
        self.assertTrue(self.application.quit_dialog.apply_to_all)
        self.application.draw()
        self.assertFalse(self.press_dialog("save"))
        self.assertEqual(va.read_image_pixels(self.folder / "a.png")[0, 0].tolist(), [0, 0, 0, 255])
        self.assertEqual(va.read_image_pixels(self.folder / "b.png")[0, 0].tolist(), [0, 0, 0, 255])

    def test_a_failed_save_leaves_the_dialog_open_and_shows_the_reason(self):
        self.select((0, 0))
        self.drag(self.position(0, 0), self.position(1, 0))
        self.quit_event()
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            self.assertTrue(self.press_dialog("save"))
        self.assertIn("disk full", self.application.quit_dialog.error)
        self.application.draw()
        self.assertFalse(self.press_dialog("discard"))

    def test_the_dialog_shows_the_current_tile_and_the_file_side_by_side(self):
        self.select((0, 0))
        self.drag(self.position(0, 0), self.position(1, 0))
        self.quit_event()
        self.application.draw()
        areas = self.application.dialog_areas()
        surface = self.application.surface
        current = va.fit_picture_rectangle(areas["current"], 8)
        disk = va.fit_picture_rectangle(areas["disk"], 8)
        self.assertEqual(tuple(surface.get_at((current.x + 2, current.y + 2)))[:3], (0, 0, 0))
        self.assertEqual(tuple(surface.get_at((disk.x + 2, disk.y + 2)))[:3], BLUE[:3])
        self.assertEqual(tuple(surface.get_at((current.right - 3, current.bottom - 3)))[:3], BLUE[:3])

    def test_the_dialog_draws_in_a_small_window_and_for_a_tile_with_no_file(self):
        project = self.make_project({(0, 0): ("n.png", None), (1, 0): ("m.png", None)}, data={(0, 0): va.encode_tile_data(solid_tile(BLUE, 8)), (1, 0): va.encode_tile_data(solid_tile(RED, 8))}, tile_size=8)
        application = va.Application(project, va.pygame.Surface((700, 520)))
        application.handle_event(va.pygame.event.Event(va.pygame.QUIT))
        application.draw()


class PurgeWindowTests(ProjectFixture):
    """Right-clicking a tile in the atlas offers to purge it, after a warning."""

    @classmethod
    def setUpClass(cls):
        va.pygame.init()

    def setUp(self):
        super().setUp()
        self.project = self.make_project(
            {(0, 0): ("a.png", solid_tile(RED)), (1, 0): ("b.png", solid_tile(GREEN))},
            unplaced=[("loose.png", solid_tile(BLUE))],
        )
        self.application = va.Application(self.project, va.pygame.Surface((1280, 800)))

    def send(self, kind, position, button=1):
        event_type = {"down": va.pygame.MOUSEBUTTONDOWN, "up": va.pygame.MOUSEBUTTONUP}[kind]
        return self.application.handle_event(va.pygame.event.Event(event_type, pos=position, button=button))

    def click(self, position, button=1):
        self.send("down", position, button)
        self.send("up", position, button)

    def key(self, key):
        return self.application.handle_event(va.pygame.event.Event(va.pygame.KEYDOWN, key=key, mod=0, unicode="", scancode=0))

    def tile_position(self, coordinate):
        return self.application.atlas_panel.tile_screen_rect(coordinate).center

    def open_menu(self, coordinate=(0, 0)):
        self.click(self.tile_position(coordinate), button=3)

    def open_dialog(self, coordinate=(0, 0)):
        self.open_menu(coordinate)
        self.click(self.application.context_menu_area().center)

    def press_dialog(self, name):
        self.click(self.application.purge_dialog_areas()[name].center)

    def test_right_clicking_a_tile_opens_a_menu_for_it_without_changing_the_selection(self):
        self.project.select_only((1, 0))
        self.open_menu((0, 0))
        self.assertEqual(self.application.context_menu.coordinate, (0, 0))
        self.assertEqual(self.project.selection, {(1, 0)})
        self.assertIsNone(self.application.purge_dialog)
        self.application.draw()

    def test_right_clicking_an_empty_place_or_an_unplaced_tile_opens_no_menu(self):
        self.open_menu((5, 3))
        self.assertIsNone(self.application.context_menu)
        self.click(self.application.unplaced_panel.tile_screen_rect((0, 0)).center, button=3)
        self.assertIsNone(self.application.context_menu)

    def test_the_menu_stays_inside_the_window_when_opened_near_the_edge(self):
        self.application.context_menu = va.ContextMenu((0, 0), (1279, 799))
        area = self.application.context_menu_area()
        self.assertTrue(self.application.surface.get_rect().contains(area))

    def test_choosing_the_menu_item_asks_first_and_deletes_nothing_yet(self):
        self.open_dialog()
        self.assertIsNone(self.application.context_menu)
        self.assertEqual(self.application.purge_dialog.title, "Purge a.png?")
        self.assertTrue((self.folder / "a.png").exists())
        self.assertIn((0, 0), self.saved().coordinates_to_names)
        self.application.draw()

    def test_confirming_deletes_the_file_and_the_entries_and_says_so(self):
        self.project.select_only((0, 0))
        self.application.draw()
        self.open_dialog()
        self.press_dialog("purge")
        self.assertIsNone(self.application.purge_dialog)
        self.assertFalse((self.folder / "a.png").exists())
        self.assertEqual(self.saved().coordinates_to_names, {(1, 0): "b.png"})
        self.assertEqual(self.project.selection, set())
        self.assertIn("Purged a.png", self.application.message)
        self.application.draw()
        self.assertIsNone(self.application.editor.layout, "the editor no longer shows the purged tile")

    def test_cancel_and_escape_and_the_menu_closing_delete_nothing(self):
        self.open_dialog()
        self.press_dialog("cancel")
        self.assertIsNone(self.application.purge_dialog)
        self.open_dialog()
        self.key(va.pygame.K_ESCAPE)
        self.assertIsNone(self.application.purge_dialog)
        self.open_menu()
        self.click((5, 5))
        self.assertIsNone(self.application.context_menu)
        self.open_menu()
        self.key(va.pygame.K_ESCAPE)
        self.assertIsNone(self.application.context_menu)
        self.assertIsNone(self.application.purge_dialog)
        self.assertTrue((self.folder / "a.png").exists())
        self.assertEqual(sorted(self.saved().coordinates_to_names), [(0, 0), (1, 0)])

    def test_enter_does_not_confirm_a_deletion(self):
        self.open_dialog()
        self.key(va.pygame.K_RETURN)
        self.assertIsNotNone(self.application.purge_dialog)
        self.assertTrue((self.folder / "a.png").exists())

    def test_clicking_outside_the_dialog_does_nothing_and_the_window_behind_it_is_idle(self):
        self.open_dialog()
        self.click(self.tile_position((1, 0)))
        self.assertEqual(self.project.selection, set())
        self.assertIsNotNone(self.application.purge_dialog)

    def test_a_click_outside_the_menu_closes_it_and_does_not_select_what_is_under_it(self):
        self.open_menu()
        self.click(self.tile_position((1, 0)))
        self.assertIsNone(self.application.context_menu)
        self.assertEqual(self.project.selection, set())

    def test_releasing_the_right_button_over_the_menu_does_not_choose_the_item(self):
        self.open_menu()
        self.send("up", self.application.context_menu_area().center, button=3)
        self.assertIsNotNone(self.application.context_menu)
        self.assertIsNone(self.application.purge_dialog)

    def test_a_disk_error_keeps_the_dialog_open_and_shows_why(self):
        self.open_dialog()
        with mock.patch.object(va.os, "replace", side_effect=OSError("disk full")):
            self.press_dialog("purge")
        self.assertIsNotNone(self.application.purge_dialog)
        self.assertIn("disk full", self.application.purge_dialog.error)
        self.assertTrue((self.folder / "a.png").exists())
        self.application.draw()
        self.press_dialog("purge")
        self.assertFalse((self.folder / "a.png").exists())

    def test_the_menu_and_dialog_draw_in_a_small_window(self):
        application = va.Application(self.project, va.pygame.Surface((600, 420)))
        application.context_menu = va.ContextMenu((0, 0), (590, 410))
        application.draw()
        application.context_menu = None
        application.purge_dialog = va.PurgeDialog(self.project, (0, 0))
        application.draw()

    def test_the_tile_the_menu_is_for_is_outlined_so_it_is_clear_which_one_is_meant(self):
        self.open_menu((1, 0))
        self.application.draw()
        rectangle = self.application.atlas_panel.tile_screen_rect((1, 0))
        outline = tuple(self.application.surface.get_at((rectangle.centerx, rectangle.y - 2)))[:3]
        self.assertEqual(outline, va.Theme.INVALID)

    def test_right_clicks_in_the_editor_still_go_to_the_editor(self):
        self.project.select_only((0, 0))
        self.application.draw()
        self.click(self.application.editor.areas.canvas.center, button=3)
        self.assertIsNone(self.application.context_menu)

    def test_the_program_with_a_menu_open_closes_on_quit_like_before(self):
        self.open_menu()
        self.assertFalse(self.application.handle_event(va.pygame.event.Event(va.pygame.QUIT)))


class WrapAssistEditorTests(EditorDriver):
    """The W button in the sidebar and what wrap-assist does to the canvas: a tile shown as a repeating grid."""

    def paint(self, coordinate, column, row, color):
        record = self.project.tiles[coordinate]
        layout = va.compose_canvas({coordinate: record.pixels}, 8)
        canvas = layout.pixels.copy()
        canvas[row, column] = color
        self.project.apply_canvas_edit(layout, canvas)

    def wrap_button(self):
        return self.editor.areas.sidebar_buttons["wrap"]

    def click_wrap_button(self):
        self.send("down", self.wrap_button().center)
        self.send("up", self.wrap_button().center)

    def color_at(self, position):
        return tuple(self.application.surface.get_at(position))[:3]

    def copies_in_view(self, column, row, size=8):
        """The positions of a canvas pixel in every copy of the canvas whose center is in the canvas area."""
        found = []
        for copy_column in range(-6, 9):
            for copy_row in range(-6, 9):
                center = self.editor.view.pixel_rectangle(column + copy_column * size, row + copy_row * size).center
                if self.editor.areas.canvas.collidepoint(center):
                    found.append(((copy_column, copy_row), center))
        return found

    def test_wrap_assist_starts_off_and_the_button_toggles_it(self):
        self.select((0, 0))
        self.assertFalse(self.editor.wrap_assist)
        self.click_wrap_button()
        self.assertTrue(self.editor.wrap_assist)
        self.click_wrap_button()
        self.assertFalse(self.editor.wrap_assist)

    def test_clicking_the_button_does_not_draw_or_change_the_selection(self):
        self.select((0, 0))
        self.click_wrap_button()
        self.assertFalse(self.project.tiles[(0, 0)].is_modified)
        self.assertEqual(self.project.selection, {(0, 0)})

    def test_the_button_says_w_and_is_empty_when_unchecked_and_all_blue_when_checked(self):
        self.select((0, 0))
        self.application.draw()
        corner = (self.wrap_button().x + 4, self.wrap_button().y + 4)
        self.assertEqual(self.color_at(corner), va.Theme.PANEL, "empty: it shows the panel behind it")
        self.click_wrap_button()
        self.application.draw()
        self.assertEqual(self.color_at(corner), va.Theme.ACCENT)
        self.assertEqual(self.color_at((self.wrap_button().right - 5, self.wrap_button().bottom - 5)), va.Theme.ACCENT)
        self.click_wrap_button()
        self.application.draw()
        self.assertEqual(self.color_at(corner), va.Theme.PANEL)

    def test_the_button_has_a_name_when_the_pointer_rests_on_it(self):
        self.application.pointer = self.wrap_button().center
        self.assertIn("Wrap", self.application.hover_text())

    def test_the_tile_is_shown_as_a_repeating_grid_only_while_wrap_assist_is_on(self):
        self.select((0, 0))
        self.paint((0, 0), 2, 3, RED)
        self.editor.view.zoom = 4  # small enough that there is room beside the tile
        self.application.draw()
        beyond = self.editor.view.pixel_rectangle(2 + 8, 3).center
        self.assertTrue(self.editor.areas.canvas.collidepoint(beyond))
        self.assertNotEqual(self.color_at(beyond), RED[:3], "one tile, and nothing beside it")
        self.click_wrap_button()
        self.park_the_pointer()
        self.application.draw()
        copies = self.copies_in_view(2, 3)
        self.assertGreaterEqual(len(copies), 9)
        beside = 0
        for copy, center in copies:
            self.assertEqual(self.color_at(center), RED[:3], copy)
            neighbour = (center[0] + self.editor.view.zoom, center[1])
            if self.editor.areas.canvas.collidepoint(neighbour):
                self.assertEqual(self.color_at(neighbour), BLUE[:3], copy)
                beside += 1
        self.assertGreaterEqual(beside, 6)

    def test_the_whole_canvas_repeats_when_several_tiles_are_shown(self):
        self.select((0, 0), (1, 0))
        self.paint((1, 0), 7, 0, RED)
        self.click_wrap_button()
        self.park_the_pointer()
        self.application.draw()
        for copy, center in self.copies_in_view(15, 0, size=16):
            self.assertEqual(self.color_at(center), RED[:3], copy)

    def park_the_pointer(self):
        """Away from the canvas and the button, so that no tooltip or brush guide is drawn over what is sampled."""
        self.application.pointer = self.editor.pointer = (5, 5)

    def test_no_line_marks_where_a_copy_of_the_tile_ends_and_the_next_begins(self):
        self.select((0, 0))
        self.park_the_pointer()
        self.application.draw()
        left_edge = self.editor.view.pixel_rectangle(0, 0).x
        row = self.editor.view.pixel_rectangle(0, 3).centery
        self.assertNotEqual(self.color_at((left_edge, row)), BLUE[:3], "without wrap-assist the tile's edge is outlined")
        self.click_wrap_button()
        self.park_the_pointer()
        self.editor.view.zoom = 2
        self.application.draw()
        area = self.editor.areas.canvas
        for x in range(area.x + 2, area.right - 2):
            self.assertEqual(self.color_at((x, area.centery)), BLUE[:3], x)
        for y in range(area.y + 2, area.bottom - 2):
            self.assertEqual(self.color_at((area.centerx, y)), BLUE[:3], y)

    def test_the_brush_guide_is_drawn_once_for_each_copy_of_the_tile(self):
        self.select((0, 0))
        self.click_wrap_button()
        self.move(self.position(3, 2))
        self.application.draw()
        copies = self.copies_in_view(3, 2)
        self.assertGreaterEqual(len(copies), 9)
        for copy, _ in copies:
            rectangle = self.editor.view.pixel_rectangle(3 + copy[0] * 8, 2 + copy[1] * 8)
            self.assertEqual(self.color_at(rectangle.topleft), (255, 255, 255), copy)
        stray = self.editor.view.pixel_rectangle(4, 2)
        self.assertNotEqual(self.color_at(stray.topleft), (255, 255, 255), "no guide on a pixel that is not under the brush")

    def test_without_wrap_assist_there_is_one_brush_guide(self):
        self.select((0, 0))
        self.move(self.position(3, 2))
        self.application.draw()
        rectangle = self.editor.view.pixel_rectangle(3, 2)
        self.assertEqual(self.color_at(rectangle.topleft), (255, 255, 255))
        self.assertEqual(len([1 for _, center in self.copies_in_view(3, 2) if self.color_at(center) == (255, 255, 255)]), 0)

    def test_a_bigger_brush_has_a_bigger_guide_in_every_copy(self):
        self.select((0, 0))
        self.click_wrap_button()
        self.editor.settings.brush_sizes["pencil"] = (3, 2)
        self.move(self.position(7, 7))
        self.application.draw()
        for copy_column, copy_row in [(0, 0), (1, 0), (0, 1), (1, 1), (-1, -1)]:
            rectangle = self.editor.view.pixel_rectangle(6 + copy_column * 8, 6 + copy_row * 8, 3, 2)
            if self.editor.areas.canvas.contains(rectangle):
                self.assertEqual(self.color_at(rectangle.topleft), (255, 255, 255), (copy_column, copy_row))
                self.assertEqual(self.color_at((rectangle.right - 1, rectangle.bottom - 1)), (255, 255, 255))

    def test_drawing_wraps_around_the_tile_and_is_one_undo_step(self):
        self.select((0, 0))
        self.click_wrap_button()
        self.editor.colors.set_color(RED, "primary")
        self.drag(self.position(6, 1), self.position(10, 1))
        for column in (6, 7, 0, 1, 2):
            self.assertEqual(self.pixel((0, 0), column, 1), RED, column)
        for column in (3, 4, 5):
            self.assertEqual(self.pixel((0, 0), column, 1), BLUE, column)
        self.key(va.pygame.K_z, va.pygame.KMOD_CTRL)
        self.assertFalse(self.project.tiles[(0, 0)].is_modified, "the whole stroke went in one step")

    def test_drawing_in_any_copy_changes_the_tile(self):
        self.select((0, 0))
        self.click_wrap_button()
        self.editor.colors.set_color(RED, "primary")
        self.drag(self.position(8 + 4, 16 + 5), self.position(8 + 4, 16 + 5))
        self.assertEqual(self.pixel((0, 0), 4, 5), RED)

    def test_without_wrap_assist_the_same_stroke_stops_at_the_edge(self):
        self.select((0, 0))
        self.editor.colors.set_color(RED, "primary")
        self.drag(self.position(6, 1), self.position(10, 1))
        self.assertEqual(self.pixel((0, 0), 7, 1), RED)
        self.assertEqual(self.pixel((0, 0), 0, 1), BLUE)

    def test_with_several_tiles_shown_a_stroke_wraps_around_all_of_them(self):
        self.select((0, 0), (1, 0))
        self.click_wrap_button()
        self.editor.colors.set_color(RED, "primary")
        self.drag(self.position(14, 2), self.position(18, 2))
        self.assertEqual([self.pixel((1, 0), column, 2) for column in (5, 6, 7)], [GREEN, RED, RED])
        self.assertEqual([self.pixel((0, 0), column, 2) for column in (0, 1, 2, 3)], [RED, RED, RED, BLUE])

    def test_the_eye_dropper_picks_from_any_copy(self):
        self.select((0, 0))
        self.paint((0, 0), 2, 3, GREEN)
        self.click_wrap_button()
        self.click_tool("eyedropper")
        self.send("down", self.position(2 + 8, 3 + 16))
        self.send("up", self.position(2 + 8, 3 + 16))
        self.assertEqual(self.editor.colors.get("primary"), GREEN)

    def test_the_fill_bucket_works_from_any_copy(self):
        self.select((0, 0))
        self.paint((0, 0), 2, 3, GREEN)
        self.click_wrap_button()
        self.click_tool("fill")
        self.editor.colors.set_color(RED, "primary")
        self.send("down", self.position(5 + 8, 5 + 8))
        self.send("up", self.position(5 + 8, 5 + 8))
        self.assertEqual(self.pixel((0, 0), 2, 3), GREEN, "the green pixel is a different color and stays")
        self.assertEqual(self.pixel((0, 0), 0, 0), RED)
        self.assertEqual(self.pixel((0, 0), 7, 7), RED)

    def test_the_select_tool_works_on_the_tile_whichever_copy_it_is_used_in(self):
        self.select((0, 0))
        self.click_wrap_button()
        self.click_tool("select")
        self.drag(self.position(8 + 1, 8 + 1), self.position(8 + 3, 8 + 3))
        self.assertEqual(self.editor.selection_rectangle, (1, 1, 4, 4))

    def test_a_pasted_picture_starts_at_the_tile_even_when_the_view_has_moved_away(self):
        self.select((0, 0))
        self.click_wrap_button()
        self.editor.internal_image = solid_tile(RED, 2)
        self.editor.internal_is_newer = True
        self.editor.view.pan_by(120, 90)
        self.editor.paste()
        self.assertEqual((self.editor.floating.column, self.editor.floating.row), (0, 0))

    def test_turning_it_on_shows_several_copies_and_turning_it_off_shows_the_one_tile_as_big_as_before(self):
        self.select((0, 0))
        before = self.editor.view.zoom
        self.click_wrap_button()
        self.assertLess(self.editor.view.zoom, before)
        self.assertGreaterEqual(self.editor.view.zoom * 8 * 3, 1)
        area = self.editor.areas.canvas
        self.assertLessEqual(self.editor.view.zoom * 8 * 3, min(area.size))
        self.click_wrap_button()
        self.assertEqual(self.editor.view.zoom, before)

    def test_wrap_assist_stays_on_when_another_tile_is_selected(self):
        self.select((0, 0))
        self.click_wrap_button()
        self.select((1, 0))
        self.assertTrue(self.editor.wrap_assist)
        self.application.draw()

    def test_it_can_be_turned_on_with_nothing_shown_and_the_window_draws_in_a_small_window(self):
        self.click_wrap_button()
        self.application.draw()
        small = va.Application(self.project, va.pygame.Surface((700, 520)))
        small.editor.wrap_assist = True
        self.project.selection = {(0, 0)}
        small.editor.sync()
        small.handle_event(va.pygame.event.Event(va.pygame.MOUSEMOTION, pos=small.editor.areas.canvas.center, rel=(0, 0), buttons=(0, 0, 0)))
        small.draw()


if __name__ == "__main__":
    unittest.main()
