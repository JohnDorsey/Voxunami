#!/usr/bin/env python3
"""VirtualAtlas: edit a folder of tile images as if they were one texture atlas.

The tiles are separate PNG files in this folder. Their arrangement in the atlas is stored in atlas.json
in this folder. No single atlas image exists.

atlas.json:
    {
        "tile_size": 16,
        "coordinates_to_names": {"(column, row)": "file.png"},
        "coordinates_to_data": {"(column, row)": "<base64 of tile_size * tile_size * 4 RGBA bytes>"}
    }
coordinates_to_data holds the pixels of a tile that has no PNG file yet. The entry is removed when the tile is
saved as a file. Unknown top-level keys are kept when the file is rewritten.

This file is organised in sections: file formats and folder scanning, atlas arrangement rules, painting
algorithms, edit history, the project (tiles, selection, saving), then the user interface. Everything except the
drawing is plain logic that can be tested without a window (tools/virtual_atlas/test_virtual_atlas.py).
"""

from __future__ import annotations

import base64
import binascii
import dataclasses
import io
import itertools
import json
import logging
import math
import os
import random
import re
import sys
import tempfile
from collections import deque
from fractions import Fraction
from pathlib import Path
from typing import Callable, Hashable, Iterable, Optional

os.environ.setdefault("PYGAME_HIDE_SUPPORT_PROMPT", "1")
try:
    import numpy
    import pygame
    from PIL import Image
except ImportError as missing_module_error:
    sys.exit(
        f"VirtualAtlas needs numpy, Pillow and pygame (missing: {missing_module_error.name}).\n"
        "Install them with: pip install numpy pillow pygame"
    )

logger = logging.getLogger("VirtualAtlas")

ATLAS_FILE_NAME = "atlas.json"
DEFAULT_TILE_SIZE = 16
MINIMUM_TILE_SIZE = 1
MAXIMUM_TILE_SIZE = 1024
TILE_FILE_EXTENSION = ".png"
TEMPORARY_FILE_PREFIX = ".virtualatlas-"
TEMPORARY_FILE_SUFFIX = ".tmp"
HISTORY_DEPTH = 100

# A tile's place in the atlas: (column, row), counted from the top left, starting at 0.
Coordinate = tuple[int, int]
# Colors are (red, green, blue, alpha), each 0..255.
Color = tuple[int, int, int, int]
TRANSPARENT: Color = (0, 0, 0, 0)

_INVALID_NAME_CHARACTERS = set('\\/:*?"<>|')
_RESERVED_DEVICE_NAMES = {"con", "prn", "aux", "nul"} | {f"com{number}" for number in range(1, 10)} | {
    f"lpt{number}" for number in range(1, 10)
}
_COORDINATE_PATTERN = re.compile(r"^\(\s*(-?\d+)\s*,\s*(-?\d+)\s*\)$")


# ----------------------------------------------------------------------------------------------------------
# Coordinates and names
# ----------------------------------------------------------------------------------------------------------


def reading_order_key(coordinate: Coordinate) -> tuple[int, int]:
    """Sort key that puts coordinates in reading order: row by row, left to right."""
    column, row = coordinate
    return (row, column)


def format_coordinate(coordinate: Coordinate) -> str:
    column, row = coordinate
    return f"({column}, {row})"


def parse_coordinate(text: str) -> Coordinate:
    """Parse "(column, row)". Raises ValueError for anything else, including negative numbers."""
    match = _COORDINATE_PATTERN.match(text)
    if match is None:
        raise ValueError(f"{text!r} is not a coordinate written as \"(column, row)\"")
    column, row = int(match.group(1)), int(match.group(2))
    if column < 0 or row < 0:
        raise ValueError(f"{text!r} has a negative number")
    return (column, row)


def validate_tile_name(name: str) -> Optional[str]:
    """Return why a file name cannot be used for a tile, or None if it can."""
    if not name or name != name.strip():
        return "the name is empty or starts or ends with a space"
    if len(name) > 255:
        return "the name is longer than 255 characters"
    if not name.lower().endswith(TILE_FILE_EXTENSION):
        return f"the name must end with {TILE_FILE_EXTENSION}"
    stem = name[: -len(TILE_FILE_EXTENSION)]
    if not stem:
        return "the name has nothing before the extension"
    for character in name:
        if character in _INVALID_NAME_CHARACTERS or ord(character) < 32:
            return f"the name contains the character {character!r}"
    if stem.split(".")[0].rstrip().casefold() in _RESERVED_DEVICE_NAMES:
        return f"{stem!r} is a reserved device name on Windows"
    return None


def find_name_problem(name: str, taken_names: Iterable[str]) -> Optional[str]:
    """Like validate_tile_name, and also reports a clash with `taken_names` (compared ignoring case)."""
    problem = validate_tile_name(name)
    if problem is not None:
        return problem
    if name.casefold() in {taken.casefold() for taken in taken_names}:
        return f"{name!r} is already used by another file"
    return None


def make_unique_name(name: str, taken_names: Iterable[str]) -> str:
    """`name`, or `name` with _2, _3, ... before the extension, so that it is not in `taken_names`."""
    taken = {taken_name.casefold() for taken_name in taken_names}
    if name.casefold() not in taken:
        return name
    stem, extension = name[: -len(TILE_FILE_EXTENSION)], name[-len(TILE_FILE_EXTENSION):]
    for suffix_number in itertools.count(2):
        candidate = f"{stem}_{suffix_number}{extension}"
        if candidate.casefold() not in taken:
            return candidate
    raise AssertionError("unreachable")


# ----------------------------------------------------------------------------------------------------------
# atlas.json
# ----------------------------------------------------------------------------------------------------------


class AtlasFileError(Exception):
    """atlas.json cannot be used. The program refuses to start; `problems` lists every reason found."""

    def __init__(self, problems: list[str]):
        super().__init__("; ".join(problems))
        self.problems = problems


@dataclasses.dataclass
class AtlasFile:
    tile_size: Optional[int]
    coordinates_to_names: dict[Coordinate, str] = dataclasses.field(default_factory=dict)
    # Base64 text, not decoded, so that data that cannot be used is preserved when the file is rewritten.
    coordinates_to_data: dict[Coordinate, str] = dataclasses.field(default_factory=dict)
    other_settings: dict[str, object] = dataclasses.field(default_factory=dict)


class _DuplicateKeyError(Exception):
    pass


def _reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise _DuplicateKeyError(key)
        result[key] = value
    return result


def _read_coordinate_mapping(
    section_name: str, section: object, value_description: str, problems: list[str]
) -> dict[Coordinate, object]:
    """Turn {"(c, r)": value} into {(c, r): value}, reporting bad or repeated coordinates."""
    if not isinstance(section, dict):
        problems.append(f"{section_name} must be an object")
        return {}
    result: dict[Coordinate, object] = {}
    spelling_of: dict[Coordinate, str] = {}
    for text, value in section.items():
        try:
            coordinate = parse_coordinate(text)
        except ValueError as error:
            problems.append(f"{section_name}: {error}")
            continue
        if coordinate in result:
            problems.append(
                f"{section_name}: the position {format_coordinate(coordinate)} appears twice, "
                f"as {spelling_of[coordinate]!r} and {text!r}"
            )
            continue
        if not isinstance(value, str):
            problems.append(f"{section_name}: the {value_description} at {text!r} must be a string")
            continue
        result[coordinate] = value
        spelling_of[coordinate] = text
    return result


def parse_atlas_file(text: str) -> AtlasFile:
    """Parse and validate atlas.json. Raises AtlasFileError listing every problem found."""
    try:
        document = json.loads(text, object_pairs_hook=_reject_duplicate_keys)
    except _DuplicateKeyError as error:
        raise AtlasFileError([f"the key {error.args[0]!r} appears twice in the same object"]) from None
    except ValueError as error:
        raise AtlasFileError([f"not valid JSON: {error}"]) from None
    if not isinstance(document, dict):
        raise AtlasFileError(["the top level must be an object"])

    problems: list[str] = []
    other_settings = dict(document)

    tile_size = other_settings.pop("tile_size", None)
    if tile_size is not None and (
        isinstance(tile_size, bool)
        or not isinstance(tile_size, int)
        or not MINIMUM_TILE_SIZE <= tile_size <= MAXIMUM_TILE_SIZE
    ):
        problems.append(f"tile_size must be a whole number from {MINIMUM_TILE_SIZE} to {MAXIMUM_TILE_SIZE}")
        tile_size = None

    names = _read_coordinate_mapping(
        "coordinates_to_names", other_settings.pop("coordinates_to_names", {}), "file name", problems
    )
    coordinate_of_name: dict[str, Coordinate] = {}
    for coordinate in sorted(names, key=reading_order_key):
        name = names[coordinate]
        name_problem = validate_tile_name(name)
        if name_problem is not None:
            problems.append(f"coordinates_to_names: {format_coordinate(coordinate)} has a bad name: {name_problem}")
            continue
        earlier = coordinate_of_name.setdefault(name.casefold(), coordinate)
        if earlier != coordinate:
            problems.append(
                f"coordinates_to_names: {name!r} appears at both {format_coordinate(earlier)} "
                f"and {format_coordinate(coordinate)}"
            )

    data = _read_coordinate_mapping(
        "coordinates_to_data", other_settings.pop("coordinates_to_data", {}), "pixel data", problems
    )
    for coordinate in sorted(data, key=reading_order_key):
        if coordinate not in names:
            problems.append(f"coordinates_to_data: {format_coordinate(coordinate)} has data but no name")

    if problems:
        raise AtlasFileError(problems)
    return AtlasFile(tile_size, dict(names), dict(data), other_settings)  # type: ignore[arg-type]


def serialize_atlas_file(atlas_file: AtlasFile) -> str:
    """The text of atlas.json: entries in reading order, indented by 4, unknown settings kept at the end."""
    document: dict[str, object] = {}
    if atlas_file.tile_size is not None:
        document["tile_size"] = atlas_file.tile_size
    document["coordinates_to_names"] = {
        format_coordinate(coordinate): atlas_file.coordinates_to_names[coordinate]
        for coordinate in sorted(atlas_file.coordinates_to_names, key=reading_order_key)
    }
    document["coordinates_to_data"] = {
        format_coordinate(coordinate): atlas_file.coordinates_to_data[coordinate]
        for coordinate in sorted(atlas_file.coordinates_to_data, key=reading_order_key)
    }
    document.update(atlas_file.other_settings)
    return json.dumps(document, indent=4, ensure_ascii=False) + "\n"


def read_atlas_file(path: Path) -> Optional[AtlasFile]:
    """The parsed atlas.json, or None if the file does not exist. Raises AtlasFileError if it is unusable."""
    try:
        text = path.read_bytes().decode("utf-8-sig")
    except FileNotFoundError:
        return None
    except (OSError, UnicodeDecodeError) as error:
        raise AtlasFileError([f"cannot read {path.name}: {error}"]) from None
    return parse_atlas_file(text)


def write_bytes_atomically(path: Path, content: bytes) -> None:
    """Write `content` to `path` so that readers see the old file or the new one, never half of it."""
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=TEMPORARY_FILE_PREFIX, suffix=TEMPORARY_FILE_SUFFIX, dir=path.parent
    )
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary_name, path)
    except BaseException:
        try:
            os.unlink(temporary_name)
        except FileNotFoundError:
            pass
        raise


def write_atlas_file(path: Path, atlas_file: AtlasFile) -> None:
    write_bytes_atomically(path, serialize_atlas_file(atlas_file).encode("utf-8"))


# ----------------------------------------------------------------------------------------------------------
# Tile pixels, base64 data and PNG files
# ----------------------------------------------------------------------------------------------------------


class TileDataError(ValueError):
    """Stored pixel data that cannot be turned into a tile of the project's size."""


class TileImageError(ValueError):
    """A tile image file that cannot be read."""


def encode_tile_data(pixels: numpy.ndarray) -> str:
    """Base64 of the raw RGBA bytes, row by row."""
    return base64.b64encode(numpy.ascontiguousarray(pixels, dtype=numpy.uint8).tobytes()).decode("ascii")


def decode_tile_data(text: str, tile_size: int) -> numpy.ndarray:
    try:
        raw = base64.b64decode(text, validate=True)
    except (binascii.Error, ValueError):
        raise TileDataError("is not valid base64") from None
    expected_length = tile_size * tile_size * 4
    if len(raw) != expected_length:
        raise TileDataError(f"holds {len(raw)} bytes but a {tile_size}x{tile_size} tile needs {expected_length}")
    return numpy.frombuffer(raw, dtype=numpy.uint8).reshape(tile_size, tile_size, 4).copy()


def read_image_size(path: Path) -> tuple[int, int]:
    """(width, height) of an image file, read from its header."""
    try:
        with Image.open(path) as image:
            return image.size
    except (OSError, ValueError, SyntaxError, Image.DecompressionBombError) as error:
        raise TileImageError(f"cannot be read as an image: {error}") from None


def read_image_pixels(path: Path) -> numpy.ndarray:
    """All pixels of an image file as an (height, width, 4) uint8 RGBA array."""
    try:
        with Image.open(path) as image:
            return numpy.array(image.convert("RGBA"), dtype=numpy.uint8)
    except (OSError, ValueError, SyntaxError, Image.DecompressionBombError) as error:
        raise TileImageError(f"cannot be read as an image: {error}") from None


def encode_png(pixels: numpy.ndarray) -> bytes:
    buffer = io.BytesIO()
    Image.fromarray(numpy.ascontiguousarray(pixels, dtype=numpy.uint8)).save(buffer, format="PNG")
    return buffer.getvalue()


def write_png_atomically(path: Path, pixels: numpy.ndarray) -> None:
    write_bytes_atomically(path, encode_png(pixels))


# ----------------------------------------------------------------------------------------------------------
# Loading a folder
# ----------------------------------------------------------------------------------------------------------


@dataclasses.dataclass(eq=False)
class TileRecord:
    """One tile of the atlas.

    pixels is the current content (None for an error tile). disk_pixels is the content of the PNG file
    (None when the tile has no usable file yet). error says why a tile cannot be edited.
    """

    name: str
    pixels: Optional[numpy.ndarray]
    disk_pixels: Optional[numpy.ndarray]
    error: Optional[str] = None
    # Increased whenever pixels change, so that pictures drawn from them know to be drawn again.
    revision: int = 0
    # The stored base64 of an error tile whose data could not be used, kept so that saving does not drop it.
    preserved_data_text: Optional[str] = None

    @property
    def is_new(self) -> bool:
        """Exists only in atlas.json so far."""
        return self.error is None and self.disk_pixels is None

    @property
    def is_modified(self) -> bool:
        """Has changes that the PNG file does not have, or has no PNG file."""
        if self.error is not None:
            return False
        return self.disk_pixels is None or not numpy.array_equal(self.pixels, self.disk_pixels)

    @property
    def can_return_to_unplaced(self) -> bool:
        """Only a tile that is exactly its file can leave the atlas."""
        return self.error is None and not self.is_modified


@dataclasses.dataclass(eq=False)
class UnplacedTile:
    name: str
    pixels: numpy.ndarray


@dataclasses.dataclass
class FolderScan:
    tiles: dict[Coordinate, TileRecord]
    unplaced: list[UnplacedTile]
    # Lower-cased names of every PNG file in the folder, including ones the program ignores.
    existing_names: set[str]
    # Coordinates whose stored data equals the file on disk; the caller removes these entries from atlas.json.
    stale_data_coordinates: list[Coordinate]


def scan_tile_folder(folder: Path, atlas_file: AtlasFile, tile_size: int) -> FolderScan:
    """Match the PNG files in `folder` against atlas.json.

    Listed files with a problem become error tiles. Unlisted files of the right size are returned as
    unplaced; unlisted files of any other size, or unreadable, are ignored.
    """
    png_names = sorted(
        entry.name
        for entry in os.scandir(folder)
        if entry.is_file() and entry.name.lower().endswith(TILE_FILE_EXTENSION)
    )
    actual_names_by_casefold: dict[str, list[str]] = {}
    for actual_name in png_names:
        actual_names_by_casefold.setdefault(actual_name.casefold(), []).append(actual_name)

    def find_actual_name(listed_name: str) -> Optional[str]:
        if listed_name in png_names:
            return listed_name
        candidates = actual_names_by_casefold.get(listed_name.casefold())
        return candidates[0] if candidates else None

    def read_tile_file(actual_name: str) -> tuple[Optional[numpy.ndarray], Optional[str]]:
        """(pixels, None) for a usable file, (None, reason) for one that is not."""
        path = folder / actual_name
        try:
            width, height = read_image_size(path)
            if (width, height) != (tile_size, tile_size):
                return None, f"has size {width}x{height} but the tile size is {tile_size}x{tile_size}"
            return read_image_pixels(path), None
        except TileImageError as error:
            return None, str(error)

    tiles: dict[Coordinate, TileRecord] = {}
    stale_data_coordinates: list[Coordinate] = []
    for coordinate in sorted(atlas_file.coordinates_to_names, key=reading_order_key):
        name = atlas_file.coordinates_to_names[coordinate]
        actual_name = find_actual_name(name)
        if actual_name is None:
            disk_pixels, disk_problem = None, "is missing"
        else:
            disk_pixels, disk_problem = read_tile_file(actual_name)

        data_text = atlas_file.coordinates_to_data.get(coordinate)
        if data_text is None:
            if disk_pixels is None:
                tiles[coordinate] = TileRecord(name, None, None, f"the file {disk_problem}")
            else:
                tiles[coordinate] = TileRecord(name, disk_pixels.copy(), disk_pixels)
            continue

        try:
            data_pixels = decode_tile_data(data_text, tile_size)
        except TileDataError as error:
            tiles[coordinate] = TileRecord(name, None, None, f"the stored pixel data {error}", preserved_data_text=data_text)
            continue
        if actual_name is None:
            tiles[coordinate] = TileRecord(name, data_pixels, None)
        elif disk_pixels is None:
            tiles[coordinate] = TileRecord(
                name,
                None,
                None,
                f"stored pixel data exists, but the file on disk {disk_problem}",
                preserved_data_text=data_text,
            )
        elif numpy.array_equal(data_pixels, disk_pixels):
            stale_data_coordinates.append(coordinate)
            tiles[coordinate] = TileRecord(name, disk_pixels.copy(), disk_pixels)
        else:
            tiles[coordinate] = TileRecord(name, data_pixels, disk_pixels)

    for coordinate, record in tiles.items():
        if record.error is not None:
            logger.error("%s at %s: %s", record.name, format_coordinate(coordinate), record.error)

    listed_casefolded = {name.casefold() for name in atlas_file.coordinates_to_names.values()}
    unplaced: list[UnplacedTile] = []
    for actual_name in png_names:
        if actual_name.casefold() in listed_casefolded:
            continue
        pixels, _ = read_tile_file(actual_name)
        if pixels is not None:
            unplaced.append(UnplacedTile(actual_name, pixels))

    return FolderScan(tiles, unplaced, {name.casefold() for name in png_names}, stale_data_coordinates)


# ----------------------------------------------------------------------------------------------------------
# Arranging tiles in the atlas
# ----------------------------------------------------------------------------------------------------------


def plan_group_move(
    occupied: Iterable[Coordinate],
    selected: Iterable[Coordinate],
    grabbed: Coordinate,
    drop: Coordinate,
) -> Optional[dict[Coordinate, Coordinate]]:
    """Where every tile goes when the selected tiles are dragged by `grabbed` and let go over `drop`.

    All selected tiles move by the same offset (drop - grabbed). The tiles they land on, which are not
    selected, move to the cells the selection left that it does not land on, both in reading order. For one
    tile this is a swap. Returns {old: new} for every tile that changes place, an empty dict when nothing
    moves, or None when a destination would have a negative coordinate.
    """
    occupied_set = set(occupied)
    selected_set = set(selected)
    if grabbed not in selected_set:
        raise ValueError("the grabbed tile must be selected")
    if not selected_set <= occupied_set:
        raise ValueError("every selected coordinate must hold a tile")
    offset_column, offset_row = drop[0] - grabbed[0], drop[1] - grabbed[1]
    if offset_column == 0 and offset_row == 0:
        return {}
    moves = {(column, row): (column + offset_column, row + offset_row) for column, row in selected_set}
    destinations = set(moves.values())
    if any(column < 0 or row < 0 for column, row in destinations):
        return None
    displaced = sorted((destinations - selected_set) & occupied_set, key=reading_order_key)
    vacated = sorted(selected_set - destinations, key=reading_order_key)
    # |destinations| == |selected|, so the cells left behind are at least as many as the tiles displaced.
    moves.update(zip(displaced, vacated))
    return moves


def move_keys(dictionary: dict[Coordinate, object], moves: dict[Coordinate, Coordinate]) -> dict[Coordinate, object]:
    """A copy of `dictionary` with each key listed in `moves` replaced by its new coordinate."""
    return {moves.get(coordinate, coordinate): value for coordinate, value in dictionary.items()}


def can_place_unplaced_tile(occupied: Iterable[Coordinate], target: Coordinate) -> bool:
    """An unplaced tile may only be dropped on an empty place."""
    return target[0] >= 0 and target[1] >= 0 and target not in set(occupied)


def atlas_extent(occupied: Iterable[Coordinate]) -> tuple[int, int]:
    """(columns, rows) to show: the occupied area plus one empty column and row to drop tiles into."""
    coordinates = list(occupied)
    if not coordinates:
        return (1, 1)
    return (max(column for column, _ in coordinates) + 2, max(row for _, row in coordinates) + 2)


# ----------------------------------------------------------------------------------------------------------
# Importing a picture as an atlas
# ----------------------------------------------------------------------------------------------------------


@dataclasses.dataclass(frozen=True)
class ImportGrid:
    full_columns: int
    full_rows: int
    # Width and height of the part at the right and bottom edge that does not fill a tile (0 if it fits).
    remainder_width: int
    remainder_height: int

    @property
    def divides_evenly(self) -> bool:
        return self.remainder_width == 0 and self.remainder_height == 0


def describe_import_grid(width: int, height: int, tile_size: int) -> ImportGrid:
    return ImportGrid(width // tile_size, height // tile_size, width % tile_size, height % tile_size)


def slice_image_into_tiles(pixels: numpy.ndarray, tile_size: int, mode: str) -> dict[Coordinate, numpy.ndarray]:
    """Cut a picture into tiles. `mode` is "crop" (drop partial tiles) or "pad" (fill them with transparency)."""
    if mode not in ("crop", "pad"):
        raise ValueError(f"unknown import mode {mode!r}")
    height, width = pixels.shape[:2]
    grid = describe_import_grid(width, height, tile_size)
    columns = grid.full_columns + (1 if mode == "pad" and grid.remainder_width else 0)
    rows = grid.full_rows + (1 if mode == "pad" and grid.remainder_height else 0)
    if columns == 0 or rows == 0:
        raise ValueError("the picture is smaller than one tile")
    padded = numpy.zeros((rows * tile_size, columns * tile_size, 4), dtype=numpy.uint8)
    copy_height, copy_width = min(height, rows * tile_size), min(width, columns * tile_size)
    padded[:copy_height, :copy_width] = pixels[:copy_height, :copy_width]
    return {
        (column, row): padded[
            row * tile_size : (row + 1) * tile_size, column * tile_size : (column + 1) * tile_size
        ].copy()
        for row in range(rows)
        for column in range(columns)
    }


def default_import_names(coordinates: Iterable[Coordinate], taken_names: Iterable[str]) -> dict[Coordinate, str]:
    """tile_<row>_<column>.png for each coordinate, made different from `taken_names` and from each other."""
    ordered = sorted(coordinates, key=reading_order_key)
    largest = max((max(column, row) for column, row in ordered), default=0)
    digits = max(2, len(str(largest)))
    taken = set(taken_names)
    names: dict[Coordinate, str] = {}
    for column, row in ordered:
        name = make_unique_name(f"tile_{row:0{digits}d}_{column:0{digits}d}{TILE_FILE_EXTENSION}", taken)
        taken.add(name)
        names[(column, row)] = name
    return names


# ----------------------------------------------------------------------------------------------------------
# Several tiles as one canvas
# ----------------------------------------------------------------------------------------------------------


@dataclasses.dataclass
class CanvasLayout:
    """The selected tiles laid out as one image, in the arrangement they have in the atlas."""

    pixels: numpy.ndarray
    # False where no selected tile is (empty cells between tiles); nothing can be drawn there.
    editable_mask: numpy.ndarray
    first_column: int
    first_row: int
    tile_size: int
    tile_coordinates: frozenset[Coordinate]


def compose_canvas(tiles: dict[Coordinate, numpy.ndarray], tile_size: int) -> CanvasLayout:
    if not tiles:
        raise ValueError("no tiles to compose")
    first_column = min(column for column, _ in tiles)
    first_row = min(row for _, row in tiles)
    columns = max(column for column, _ in tiles) - first_column + 1
    rows = max(row for _, row in tiles) - first_row + 1
    pixels = numpy.zeros((rows * tile_size, columns * tile_size, 4), dtype=numpy.uint8)
    editable_mask = numpy.zeros(pixels.shape[:2], dtype=bool)
    for (column, row), tile_pixels in tiles.items():
        top, left = (row - first_row) * tile_size, (column - first_column) * tile_size
        pixels[top : top + tile_size, left : left + tile_size] = tile_pixels
        editable_mask[top : top + tile_size, left : left + tile_size] = True
    return CanvasLayout(pixels, editable_mask, first_column, first_row, tile_size, frozenset(tiles))


def extract_canvas_tiles(layout: CanvasLayout, canvas_pixels: numpy.ndarray) -> dict[Coordinate, numpy.ndarray]:
    """Cut `canvas_pixels` (an edited copy of layout.pixels) back into tiles; pixels over empty cells are dropped."""
    size = layout.tile_size
    return {
        (column, row): canvas_pixels[
            (row - layout.first_row) * size : (row - layout.first_row + 1) * size,
            (column - layout.first_column) * size : (column - layout.first_column + 1) * size,
        ].copy()
        for column, row in layout.tile_coordinates
    }


# ----------------------------------------------------------------------------------------------------------
# Painting: which pixels a tool touches
# ----------------------------------------------------------------------------------------------------------
#
# Every tool first works out a boolean mask of the pixels it touches (shape (height, width) of the canvas),
# then the color is applied through the mask. Positions are (column, row) pixels of the canvas.
# A brush of size n covers n // 2 pixels before the cursor pixel and the rest after it, so odd sizes are
# centered on the cursor pixel and even sizes extend one more pixel toward the right and bottom.


def _clipped_slice(start: int, length: int, limit: int) -> slice:
    return slice(max(start, 0), max(min(start + length, limit), 0))


def stamp_rectangle(mask: numpy.ndarray, column: int, row: int, width: int, height: int) -> None:
    """Set the pixels of a width x height brush with its cursor pixel at (column, row), clipped to the mask."""
    mask_height, mask_width = mask.shape
    mask[
        _clipped_slice(row - height // 2, height, mask_height),
        _clipped_slice(column - width // 2, width, mask_width),
    ] = True


def rectangle_brush_mask(
    canvas_shape: tuple[int, int], column: int, row: int, width: int, height: int
) -> numpy.ndarray:
    mask = numpy.zeros(canvas_shape, dtype=bool)
    stamp_rectangle(mask, column, row, width, height)
    return mask


def line_pixels(start: Coordinate, end: Coordinate) -> list[Coordinate]:
    """The pixels of a one pixel wide line from start to end, both included (Bresenham)."""
    column, row = start
    end_column, end_row = end
    delta_column, delta_row = abs(end_column - column), -abs(end_row - row)
    step_column = 1 if column < end_column else -1
    step_row = 1 if row < end_row else -1
    error = delta_column + delta_row
    points = []
    while True:
        points.append((column, row))
        if column == end_column and row == end_row:
            return points
        doubled_error = 2 * error
        if doubled_error >= delta_row:
            error += delta_row
            column += step_column
        if doubled_error <= delta_column:
            error += delta_column
            row += step_row


def swept_rectangle_brush_mask(
    canvas_shape: tuple[int, int], start: Coordinate, end: Coordinate, width: int, height: int
) -> numpy.ndarray:
    """A brush dragged from start to end: the pencil and the eraser."""
    mask = numpy.zeros(canvas_shape, dtype=bool)
    for column, row in line_pixels(start, end):
        stamp_rectangle(mask, column, row, width, height)
    return mask


def thick_line_mask(canvas_shape: tuple[int, int], start: Coordinate, end: Coordinate, width: int) -> numpy.ndarray:
    """A straight line `width` pixels thick with square ends.

    The shape is a rectangle whose short sides are perpendicular to the line and pass through the two end
    pixels. A pixel is painted when its center is inside, counting the sides toward the start and end, and
    counting one of the two long sides (the one toward the top or left) but not the other, so that every
    width paints exactly that many pixels across a horizontal or vertical line. A width of 1 is the
    one pixel line_pixels draws.
    """
    mask = numpy.zeros(canvas_shape, dtype=bool)
    canvas_height, canvas_width = canvas_shape
    if width == 1:
        for column, row in line_pixels(start, end):
            if 0 <= column < canvas_width and 0 <= row < canvas_height:
                mask[row, column] = True
        return mask
    (start_column, start_row), (end_column, end_row) = start, end
    delta_column, delta_row = end_column - start_column, end_row - start_row
    if delta_column == 0 and delta_row == 0:
        stamp_rectangle(mask, start_column, start_row, width, width)
        return mask
    # The side normal is chosen to point down, or right for a vertical line, so the rule above is the same
    # whichever way the line was drawn.
    normal_column, normal_row = -delta_row, delta_column
    if normal_row < 0 or (normal_row == 0 and normal_column < 0):
        normal_column, normal_row = -normal_column, -normal_row
    length = math.hypot(delta_column, delta_row)
    first_column = max(min(start_column, end_column) - width, 0)
    last_column = min(max(start_column, end_column) + width, canvas_width - 1)
    first_row = max(min(start_row, end_row) - width, 0)
    last_row = min(max(start_row, end_row) + width, canvas_height - 1)
    if first_column > last_column or first_row > last_row:
        return mask
    columns, rows = numpy.meshgrid(
        numpy.arange(first_column, last_column + 1) - start_column,
        numpy.arange(first_row, last_row + 1) - start_row,
    )
    along = columns * delta_column + rows * delta_row
    across = columns * normal_column + rows * normal_row
    inside = (
        (along >= 0)
        & (along <= delta_column**2 + delta_row**2)
        & (2 * across >= -width * length)
        & (2 * across < width * length)
    )
    mask[first_row : last_row + 1, first_column : last_column + 1] = inside
    return mask


def horizontal_parallelogram_mask(
    canvas_shape: tuple[int, int], start: Coordinate, end: Coordinate, width: int
) -> numpy.ndarray:
    """The shape swept by a horizontal segment `width` pixels long, centered on the line from start to end.

    Its ends are horizontal. On each row it covers the `width` pixels centered on the line, with the same
    rule for even widths as the pencil. When start and end are on the same row there is no height, so it
    is the one pixel high run from the first end's left edge to the last end's right edge.
    """
    mask = numpy.zeros(canvas_shape, dtype=bool)
    canvas_height, canvas_width = canvas_shape
    (start_column, start_row), (end_column, end_row) = start, end
    for row in range(max(min(start_row, end_row), 0), min(max(start_row, end_row), canvas_height - 1) + 1):
        if start_row == end_row:
            first = min(start_column, end_column) - width // 2
            last = max(start_column, end_column) - width // 2 + width - 1
        else:
            center = start_column + Fraction((end_column - start_column) * (row - start_row), end_row - start_row)
            first = math.ceil(center - Fraction(width, 2))
            last = math.ceil(center + Fraction(width, 2)) - 1
        first, last = max(first, 0), min(last, canvas_width - 1)
        if first <= last:
            mask[row, first : last + 1] = True
    return mask


def vertical_parallelogram_mask(
    canvas_shape: tuple[int, int], start: Coordinate, end: Coordinate, width: int
) -> numpy.ndarray:
    """The vertical-ended counterpart of horizontal_parallelogram_mask."""
    canvas_height, canvas_width = canvas_shape
    return horizontal_parallelogram_mask(
        (canvas_width, canvas_height), (start[1], start[0]), (end[1], end[0]), width
    ).T


def flood_fill_mask(
    pixels: numpy.ndarray, column: int, row: int, editable_mask: Optional[numpy.ndarray] = None
) -> numpy.ndarray:
    """The pixels reachable from (column, row) through side-by-side pixels that are exactly the same RGBA."""
    height, width = pixels.shape[:2]
    result = numpy.zeros((height, width), dtype=bool)
    if not (0 <= column < width and 0 <= row < height):
        return result
    if editable_mask is not None and not editable_mask[row, column]:
        return result
    matching = numpy.all(pixels == pixels[row, column], axis=2)
    if editable_mask is not None:
        matching &= editable_mask
    pending = deque([(column, row)])
    result[row, column] = True
    while pending:
        current_column, current_row = pending.popleft()
        for next_column, next_row in (
            (current_column - 1, current_row),
            (current_column + 1, current_row),
            (current_column, current_row - 1),
            (current_column, current_row + 1),
        ):
            if (
                0 <= next_column < width
                and 0 <= next_row < height
                and matching[next_row, next_column]
                and not result[next_row, next_column]
            ):
                result[next_row, next_column] = True
                pending.append((next_column, next_row))
    return result


# ----------------------------------------------------------------------------------------------------------
# Painting: applying colors
# ----------------------------------------------------------------------------------------------------------


def alpha_over(destination: numpy.ndarray, source: numpy.ndarray) -> numpy.ndarray:
    """`source` composited over `destination` (both uint8 RGBA, same shape; colors are not premultiplied).

    Where the source is fully transparent the destination is returned exactly as it was.
    """
    source_alpha = source[..., 3].astype(numpy.float64) / 255.0
    destination_alpha = destination[..., 3].astype(numpy.float64) / 255.0
    destination_weight = destination_alpha * (1.0 - source_alpha)
    result_alpha = source_alpha + destination_weight
    divisor = numpy.where(result_alpha > 0.0, result_alpha, 1.0)[..., None]
    result_color = (
        source[..., :3] * source_alpha[..., None] + destination[..., :3] * destination_weight[..., None]
    ) / divisor
    result = numpy.empty_like(destination)
    result[..., :3] = numpy.clip(numpy.rint(result_color), 0, 255)
    result[..., 3] = numpy.clip(numpy.rint(result_alpha * 255.0), 0, 255)
    return numpy.where((source[..., 3] == 0)[..., None], destination, result)


def color_layer(shape: tuple[int, int], color: Color, coverage: numpy.ndarray) -> numpy.ndarray:
    """An RGBA layer of one color whose alpha is the color's alpha times `coverage` (0..1 per pixel)."""
    layer = numpy.zeros((*shape, 4), dtype=numpy.uint8)
    layer[..., :3] = color[:3]
    layer[..., 3] = numpy.clip(numpy.rint(color[3] * coverage), 0, 255)
    return layer


def apply_color_to_mask(pixels: numpy.ndarray, mask: numpy.ndarray, color: Color) -> None:
    """Write `color` exactly (alpha included) to the masked pixels: the pencil, lines, fill and the eraser."""
    pixels[mask] = color


def crop_pixels(pixels: numpy.ndarray, rectangle: tuple[int, int, int, int]) -> numpy.ndarray:
    left, top, right, bottom = rectangle
    return pixels[top:bottom, left:right].copy()


def rectangle_from_corners(
    corner: Coordinate, other_corner: Coordinate, canvas_shape: tuple[int, int]
) -> tuple[int, int, int, int]:
    """(left, top, right, bottom) of the pixels between two corner pixels, both included, clipped to the canvas.

    right and bottom are one past the last pixel, so the result is empty when the corners are outside.
    """
    canvas_height, canvas_width = canvas_shape
    left, right = sorted((corner[0], other_corner[0]))
    top, bottom = sorted((corner[1], other_corner[1]))
    return (
        max(left, 0),
        max(top, 0),
        max(min(right + 1, canvas_width), 0),
        max(min(bottom + 1, canvas_height), 0),
    )


def clear_rectangle(
    pixels: numpy.ndarray, rectangle: tuple[int, int, int, int], editable_mask: Optional[numpy.ndarray] = None
) -> None:
    left, top, right, bottom = rectangle
    region = numpy.zeros(pixels.shape[:2], dtype=bool)
    region[top:bottom, left:right] = True
    if editable_mask is not None:
        region &= editable_mask
    pixels[region] = TRANSPARENT


def paste_pixels(
    destination: numpy.ndarray,
    source: numpy.ndarray,
    column: int,
    row: int,
    editable_mask: Optional[numpy.ndarray] = None,
) -> numpy.ndarray:
    """A copy of `destination` with `source` composited over it with its top left pixel at (column, row).

    Parts outside the destination, or over cells that are not editable, are dropped.
    """
    result = destination.copy()
    height, width = destination.shape[:2]
    source_height, source_width = source.shape[:2]
    left, top = max(column, 0), max(row, 0)
    right, bottom = min(column + source_width, width), min(row + source_height, height)
    if left >= right or top >= bottom:
        return result
    visible_source = source[top - row : bottom - row, left - column : right - column]
    blended = alpha_over(destination[top:bottom, left:right], visible_source)
    if editable_mask is not None:
        allowed = editable_mask[top:bottom, left:right][..., None]
        blended = numpy.where(allowed, blended, destination[top:bottom, left:right])
    result[top:bottom, left:right] = blended
    return result


# ----------------------------------------------------------------------------------------------------------
# Painting: round and square brushes (noise and air brush)
# ----------------------------------------------------------------------------------------------------------


@dataclasses.dataclass(frozen=True)
class FootprintPixel:
    column_offset: int
    row_offset: int
    # Distance of the pixel's center from the brush center, as a fraction of the brush radius.
    distance_ratio: float


def brush_footprint(diameter: int, shape: str) -> list[FootprintPixel]:
    """The pixels of a round ("circle") or "square" brush, relative to the cursor pixel, in reading order.

    Diameters 1 and 2 are full squares. Larger circles contain a pixel when its center is closer to the
    brush center than sqrt(diameter^2 - 1) / 2, which makes a diameter of 3 a plus sign and 5 a rounded square.
    """
    if shape not in ("circle", "square"):
        raise ValueError(f"unknown brush shape {shape!r}")
    first = -(diameter // 2)
    pixels = []
    for row_offset in range(first, first + diameter):
        for column_offset in range(first, first + diameter):
            # Twice the offset of the pixel's center from the brush center, so everything stays whole numbers.
            doubled_column = 2 * column_offset + 1 - (2 * first + diameter)
            doubled_row = 2 * row_offset + 1 - (2 * first + diameter)
            squared = doubled_column**2 + doubled_row**2
            if shape == "circle" and diameter > 2 and squared >= diameter**2 - 1:
                continue
            pixels.append(FootprintPixel(column_offset, row_offset, math.sqrt(squared) / diameter))
    return pixels


def noise_stamp(
    pixels: numpy.ndarray,
    column: int,
    row: int,
    color: Color,
    diameter: int,
    shape: str,
    density: float,
    opacity: float,
    random_source: "RandomSource",
    editable_mask: Optional[numpy.ndarray] = None,
) -> None:
    """One stamp of the noise brush, applied in place.

    Each pixel of the brush area becomes `color` with probability `density`, rolled separately for every stamp,
    so pixels rolled true by several stamps of one stroke build up. At opacity 1 the color is written exactly;
    below it, the color is composited over the pixel with its alpha times `opacity`.
    """
    height, width = pixels.shape[:2]
    hit = numpy.zeros((height, width), dtype=bool)
    for footprint_pixel in brush_footprint(diameter, shape):
        rolled_true = random_source.random() < density
        target_column, target_row = column + footprint_pixel.column_offset, row + footprint_pixel.row_offset
        if rolled_true and 0 <= target_column < width and 0 <= target_row < height:
            hit[target_row, target_column] = True
    if editable_mask is not None:
        hit &= editable_mask
    if opacity >= 1.0:
        apply_color_to_mask(pixels, hit, color)
    elif hit.any():
        layer = color_layer((height, width), color, hit * opacity)
        pixels[...] = alpha_over(pixels, layer)


class AirbrushStroke:
    """One press-drag-release of the air brush.

    Every pixel keeps the strongest coverage any stamp of the stroke gave it, so passing over a pixel again
    within the stroke does not add up. The result is the color composited over the pixels as they were when the
    stroke began; a later stroke composites over this one as usual.
    """

    def __init__(
        self,
        pixels_before_stroke: numpy.ndarray,
        color: Color,
        diameter: int,
        hardness: float,
        opacity: float,
        editable_mask: Optional[numpy.ndarray] = None,
    ):
        self.pixels_before_stroke = pixels_before_stroke.copy()
        self.color = color
        self.hardness = hardness
        self.opacity = opacity
        self.editable_mask = editable_mask
        self.footprint = brush_footprint(diameter, "circle")
        self.coverage = numpy.zeros(pixels_before_stroke.shape[:2], dtype=numpy.float64)

    def falloff(self, distance_ratio: float) -> float:
        """1 up to `hardness` of the radius, then fading linearly to 0 at the radius."""
        if distance_ratio <= self.hardness:
            return 1.0
        return max(0.0, (1.0 - distance_ratio) / (1.0 - self.hardness))

    def stamp(self, column: int, row: int) -> None:
        height, width = self.coverage.shape
        for footprint_pixel in self.footprint:
            target_column, target_row = column + footprint_pixel.column_offset, row + footprint_pixel.row_offset
            if not (0 <= target_column < width and 0 <= target_row < height):
                continue
            if self.editable_mask is not None and not self.editable_mask[target_row, target_column]:
                continue
            strength = self.opacity * self.falloff(footprint_pixel.distance_ratio)
            if strength > self.coverage[target_row, target_column]:
                self.coverage[target_row, target_column] = strength

    def stamp_path(self, start: Coordinate, end: Coordinate) -> None:
        for column, row in line_pixels(start, end):
            self.stamp(column, row)

    def result(self) -> numpy.ndarray:
        layer = color_layer(self.coverage.shape, self.color, self.coverage)
        return alpha_over(self.pixels_before_stroke, layer)


class RandomSource:
    """What noise_stamp needs from random.Random: a method random() returning a float in [0, 1)."""

    def random(self) -> float:  # pragma: no cover - interface only
        raise NotImplementedError


# ----------------------------------------------------------------------------------------------------------
# Edit history
# ----------------------------------------------------------------------------------------------------------


@dataclasses.dataclass(eq=False)
class _HistoryEntry:
    group: int
    before: numpy.ndarray
    after: numpy.ndarray
    # Set each time the entry moves between the undo and redo stacks; the larger, the more recent.
    moved_order: int = 0


class EditHistory:
    """Undo and redo, kept separately for each tile (identified by any hashable key).

    One edit that changed several tiles is stored once in each of their histories and shares a group number.
    Undo and redo work on the tiles being shown: they step back the most recent edit among those tiles, and
    step the same edit back in every other tile it touched, as long as that edit is still the latest one there.
    Otherwise the other tile keeps its newer edits and the older part waits until they are undone.
    """

    def __init__(self, depth: int = HISTORY_DEPTH):
        self.depth = depth
        self._undo_stacks: dict[Hashable, list[_HistoryEntry]] = {}
        self._redo_stacks: dict[Hashable, list[_HistoryEntry]] = {}
        self._group_counter = 0
        self._move_counter = 0

    def record(self, changes: dict[Hashable, tuple[numpy.ndarray, numpy.ndarray]]) -> None:
        """Record one edit as {key: (pixels before, pixels after)}. Tiles that did not change are skipped."""
        self._group_counter += 1
        for key, (before, after) in changes.items():
            if numpy.array_equal(before, after):
                continue
            stack = self._undo_stacks.setdefault(key, [])
            stack.append(_HistoryEntry(self._group_counter, before.copy(), after.copy()))
            del stack[: -self.depth]
            self._redo_stacks.pop(key, None)

    def can_undo(self, shown_keys: Iterable[Hashable]) -> bool:
        return any(self._undo_stacks.get(key) for key in shown_keys)

    def can_redo(self, shown_keys: Iterable[Hashable]) -> bool:
        return any(self._redo_stacks.get(key) for key in shown_keys)

    def undo(self, shown_keys: Iterable[Hashable]) -> Optional[dict[Hashable, numpy.ndarray]]:
        """Step back the latest edit among the shown tiles. Returns {key: restored pixels}, or None if nothing to undo."""
        return self._step(shown_keys, self._undo_stacks, self._redo_stacks, lambda entry: entry.group, True)

    def redo(self, shown_keys: Iterable[Hashable]) -> Optional[dict[Hashable, numpy.ndarray]]:
        """Step forward the edit most recently undone among the shown tiles."""
        return self._step(shown_keys, self._redo_stacks, self._undo_stacks, lambda entry: entry.moved_order, False)

    def forget(self, key: Hashable) -> None:
        self._undo_stacks.pop(key, None)
        self._redo_stacks.pop(key, None)

    def _step(
        self,
        shown_keys: Iterable[Hashable],
        source_stacks: dict[Hashable, list[_HistoryEntry]],
        target_stacks: dict[Hashable, list[_HistoryEntry]],
        recency: Callable[[_HistoryEntry], int],
        is_undo: bool,
    ) -> Optional[dict[Hashable, numpy.ndarray]]:
        candidates = [source_stacks[key][-1] for key in shown_keys if source_stacks.get(key)]
        if not candidates:
            return None
        group = max(candidates, key=recency).group
        restored: dict[Hashable, numpy.ndarray] = {}
        for key, stack in source_stacks.items():
            if stack and stack[-1].group == group:
                entry = stack.pop()
                self._move_counter += 1
                entry.moved_order = self._move_counter
                target_stacks.setdefault(key, []).append(entry)
                restored[key] = (entry.before if is_undo else entry.after).copy()
        return restored


# ----------------------------------------------------------------------------------------------------------
# The project: tiles, selection and saving
# ----------------------------------------------------------------------------------------------------------


class AtlasProject:
    """The tiles of one folder, which of them are selected, and the saving of atlas.json after every change.

    Every change to the arrangement is written to atlas.json at once. If the write fails the change is undone and
    the OSError is raised, so memory and disk never disagree.
    """

    def __init__(self, folder: Path, atlas_file: Optional[AtlasFile] = None):
        self.folder = Path(folder)
        self.atlas_file_exists = atlas_file is not None
        loaded = atlas_file if atlas_file is not None else AtlasFile(None)
        self.tile_size_is_saved = loaded.tile_size is not None
        # Until atlas.json says otherwise the size is provisional, and can be changed.
        self.tile_size = loaded.tile_size if loaded.tile_size is not None else DEFAULT_TILE_SIZE
        self.other_settings = dict(loaded.other_settings)
        self.selection: set[Coordinate] = set()
        self.tiles: dict[Coordinate, TileRecord] = {}
        self.unplaced: list[UnplacedTile] = []
        self.existing_names: set[str] = set()
        self._stale_data_coordinates: list[Coordinate] = []
        self.history = EditHistory()
        self._scan(loaded)

    @classmethod
    def open(cls, folder: Path) -> "AtlasProject":
        """Open a folder. Raises AtlasFileError if atlas.json is unusable."""
        folder = Path(folder)
        project = cls(folder, read_atlas_file(folder / ATLAS_FILE_NAME))
        if project._stale_data_coordinates:
            try:
                project.save_atlas_file()
            except OSError as error:
                logger.warning("could not remove stored pixel data that equals its file: %s", error)
        return project

    def _scan(self, atlas_file: AtlasFile) -> None:
        scan = scan_tile_folder(self.folder, atlas_file, self.tile_size)
        self.tiles = scan.tiles
        self.unplaced = scan.unplaced
        self.existing_names = scan.existing_names
        self._stale_data_coordinates = scan.stale_data_coordinates
        self.selection &= set(self.tiles)

    @property
    def is_empty(self) -> bool:
        return not self.tiles

    def build_atlas_file(self) -> AtlasFile:
        """What atlas.json should contain for the current state."""
        data: dict[Coordinate, str] = {}
        for coordinate, record in self.tiles.items():
            if record.is_new:
                data[coordinate] = encode_tile_data(record.pixels)
            elif record.preserved_data_text is not None:
                data[coordinate] = record.preserved_data_text
        return AtlasFile(
            self.tile_size,
            {coordinate: record.name for coordinate, record in self.tiles.items()},
            data,
            dict(self.other_settings),
        )

    def save_atlas_file(self) -> None:
        write_atlas_file(self.folder / ATLAS_FILE_NAME, self.build_atlas_file())
        self.atlas_file_exists = True
        self.tile_size_is_saved = True

    def _commit(self, change: Callable[[], None]) -> None:
        saved_state = (dict(self.tiles), list(self.unplaced), set(self.selection))
        change()
        try:
            self.save_atlas_file()
        except BaseException:
            self.tiles, self.unplaced, self.selection = saved_state
            raise

    def set_tile_size(self, tile_size: int) -> bool:
        """Change the provisional tile size and look at the folder again. False if that is not allowed now."""
        if self.tile_size_is_saved or any(record.is_modified for record in self.tiles.values()):
            return False
        if not MINIMUM_TILE_SIZE <= tile_size <= MAXIMUM_TILE_SIZE:
            return False
        atlas_file = self.build_atlas_file()
        atlas_file.tile_size = tile_size
        self.tile_size = tile_size
        self._scan(atlas_file)
        return True

    # Editing the pixels --------------------------------------------------------------------------------

    def shown_records(self) -> list[TileRecord]:
        """The selected tiles that can be edited, in reading order: what the canvas shows."""
        return [
            self.tiles[coordinate]
            for coordinate in sorted(self.selection, key=reading_order_key)
            if self.tiles[coordinate].error is None
        ]

    def _replace_pixels(self, new_pixels: dict[TileRecord, numpy.ndarray]) -> None:
        """Give tiles new pixels. New tiles keep their pixels in atlas.json, so it is rewritten; if that fails
        the old pixels come back and the OSError is raised."""
        old_pixels = {record: record.pixels for record in new_pixels}
        for record, pixels in new_pixels.items():
            record.pixels = pixels.copy()
            record.revision += 1
        try:
            if any(record.is_new for record in new_pixels):
                self.save_atlas_file()
        except BaseException:
            for record, pixels in old_pixels.items():
                record.pixels = pixels
            raise

    def apply_canvas_edit(self, layout: CanvasLayout, new_canvas_pixels: numpy.ndarray) -> bool:
        """Make an edited canvas the new content of its tiles, as one step in the history. False if nothing changed."""
        changes: dict[TileRecord, tuple[numpy.ndarray, numpy.ndarray]] = {}
        for coordinate, pixels in extract_canvas_tiles(layout, new_canvas_pixels).items():
            record = self.tiles[coordinate]
            if not numpy.array_equal(record.pixels, pixels):
                changes[record] = (record.pixels, pixels)
        if not changes:
            return False
        self._replace_pixels({record: after for record, (_, after) in changes.items()})
        self.history.record(changes)
        return True

    def undo(self) -> bool:
        """Step back the latest edit among the shown tiles."""
        return self._step_history(is_undo=True)

    def redo(self) -> bool:
        return self._step_history(is_undo=False)

    def _step_history(self, is_undo: bool) -> bool:
        shown = self.shown_records()
        restored = self.history.undo(shown) if is_undo else self.history.redo(shown)
        if restored is None:
            return False
        try:
            self._replace_pixels(restored)
        except BaseException:
            (self.history.redo if is_undo else self.history.undo)(list(restored))
            raise
        return True

    # Saving tile files ------------------------------------------------------------------------------------

    @property
    def has_unsaved_file_changes(self) -> bool:
        """Some tile with a PNG file has changes the file does not have (new tiles are kept in atlas.json instead)."""
        return any(record.is_modified and not record.is_new for record in self.tiles.values())

    def save_tile(self, coordinate: Coordinate) -> None:
        """Write a tile to its PNG file. Raises OSError (FileExistsError if a new tile's name is taken)."""
        record = self.tiles[coordinate]
        if record.error is not None:
            raise ValueError("an error tile cannot be saved")
        was_new = record.is_new
        path = self.folder / record.name
        if was_new and (path.exists() or record.name.casefold() in self.existing_names):
            raise FileExistsError(f"{record.name} already exists in the folder")
        write_png_atomically(path, record.pixels)
        record.disk_pixels = record.pixels.copy()
        self.existing_names.add(record.name.casefold())
        if was_new:
            self.save_atlas_file()

    def save_tiles(self, coordinates: Iterable[Coordinate]) -> tuple[int, list[str]]:
        """Save every modified tile among `coordinates`. Returns how many were saved and a message for each failure."""
        saved = 0
        failures: list[str] = []
        for coordinate in sorted(coordinates, key=reading_order_key):
            record = self.tiles[coordinate]
            if record.error is not None or not record.is_modified:
                continue
            try:
                self.save_tile(coordinate)
                saved += 1
            except OSError as error:
                failures.append(f"{record.name}: {error}")
        return saved, failures

    def save_all_tiles(self) -> tuple[int, list[str]]:
        return self.save_tiles(list(self.tiles))

    # Selection ------------------------------------------------------------------------------------------

    def select_only(self, coordinate: Coordinate) -> None:
        self.selection = {coordinate}

    def toggle_selection(self, coordinate: Coordinate) -> None:
        self.selection ^= {coordinate}

    def clear_selection(self) -> None:
        self.selection = set()

    # Changes to the arrangement -------------------------------------------------------------------------

    def move_selection(self, grabbed: Coordinate, drop: Coordinate) -> bool:
        """Drag the selected tiles by `grabbed` and let go over `drop`. False if the move is refused."""
        moves = plan_group_move(self.tiles, self.selection, grabbed, drop)
        if moves is None:
            return False
        if not moves:
            return True

        def change() -> None:
            self.tiles = move_keys(self.tiles, moves)
            self.selection = {moves.get(coordinate, coordinate) for coordinate in self.selection}

        self._commit(change)
        return True

    def place_unplaced_tile(self, index: int, target: Coordinate) -> bool:
        """Put the unplaced tile at `index` on an empty place. False if the place is not free."""
        if not can_place_unplaced_tile(self.tiles, target):
            return False

        def change() -> None:
            tile = self.unplaced.pop(index)
            self.tiles[target] = TileRecord(tile.name, tile.pixels.copy(), tile.pixels)

        self._commit(change)
        return True

    def can_return_to_unplaced(self, coordinates: Iterable[Coordinate]) -> bool:
        records = [self.tiles.get(coordinate) for coordinate in coordinates]
        return bool(records) and all(record is not None and record.can_return_to_unplaced for record in records)

    def return_to_unplaced(self, coordinates: Iterable[Coordinate]) -> bool:
        """Take tiles out of the atlas. Only tiles that are exactly their file may leave; all or none."""
        coordinates = list(coordinates)
        if not self.can_return_to_unplaced(coordinates):
            return False

        def change() -> None:
            for coordinate in coordinates:
                record = self.tiles.pop(coordinate)
                self.unplaced.append(UnplacedTile(record.name, record.disk_pixels))
                self.selection.discard(coordinate)
            self.unplaced.sort(key=lambda tile: tile.name.casefold())

        removed = [self.tiles[coordinate] for coordinate in coordinates]
        self._commit(change)
        for record in removed:
            self.history.forget(record)
        return True


# ----------------------------------------------------------------------------------------------------------
# Tools, colors and strokes on the canvas
# ----------------------------------------------------------------------------------------------------------

TOOL_LABELS = {
    "pencil": "Pencil",
    "eraser": "Eraser",
    "line": "Line",
    "horizontal_line": "Horizontal line",
    "vertical_line": "Vertical line",
    "horizontal_parallelogram": "H parallelogram",
    "vertical_parallelogram": "V parallelogram",
    "noise_brush": "Noise brush",
    "airbrush": "Air brush",
    "select": "Select",
    "eyedropper": "Eye dropper",
    "fill": "Fill bucket",
}
BRUSH_TOOLS = ("pencil", "eraser")
LINE_TOOLS = ("line", "horizontal_line", "vertical_line", "horizontal_parallelogram", "vertical_parallelogram")
# The noise brush and the air brush have one diameter each, changed with the bracket keys.
DIAMETER_TOOLS = ("noise_brush", "airbrush")
STROKE_TOOLS = BRUSH_TOOLS + LINE_TOOLS + DIAMETER_TOOLS + ("fill",)
BRUSH_SHAPES = ("circle", "square")
# The sliders each brush shows: (name of the setting in ToolSettings.fractions, label).
SLIDERS = {
    "noise_brush": (("noise_density", "Density"), ("noise_opacity", "Opacity")),
    "airbrush": (("airbrush_hardness", "Hardness"), ("airbrush_opacity", "Opacity")),
}
COLOR_HISTORY_LENGTH = 16
MAXIMUM_CANVAS_ZOOM = 64


class ToolSettings:
    """The sizes and other settings of the tools. The pencil and the eraser have a width and a height each; every
    line tool shares one width; the noise brush and the air brush have a diameter each and sliders (fractions
    from 0 to 1, kept to whole percents)."""

    def __init__(self) -> None:
        self.brush_sizes: dict[str, tuple[int, int]] = {"pencil": (1, 1), "eraser": (1, 1)}
        self.line_width = 1
        self.diameters: dict[str, int] = {"noise_brush": 5, "airbrush": 9}
        self.noise_shape = "circle"
        self.fractions: dict[str, float] = {
            "noise_density": 0.5,
            "noise_opacity": 1.0,
            "airbrush_hardness": 0.3,
            "airbrush_opacity": 1.0,
        }

    def set_fraction(self, name: str, value: float) -> None:
        if name not in self.fractions:
            raise KeyError(name)
        self.fractions[name] = round(max(0.0, min(1.0, value)), 2)

    def footprint_size(self, tool: str) -> tuple[int, int]:
        """(width, height) of the area a tool covers around the cursor pixel, for drawing its outline."""
        if tool in BRUSH_TOOLS:
            return self.brush_sizes[tool]
        if tool in DIAMETER_TOOLS:
            return (self.diameters[tool], self.diameters[tool])
        if tool in ("line", "horizontal_line", "vertical_line"):
            return (self.line_width, self.line_width)
        if tool == "horizontal_parallelogram":
            return (self.line_width, 1)
        if tool == "vertical_parallelogram":
            return (1, self.line_width)
        return (1, 1)

    def adjust(
        self, tool: str, delta: int, change_width: bool, change_height: bool, canvas_shape: tuple[int, int]
    ) -> None:
        """Make the tool bigger (delta > 0) or smaller. Sizes stay between 1 and the size of the canvas."""
        canvas_height, canvas_width = canvas_shape
        if tool in BRUSH_TOOLS:
            width, height = self.brush_sizes[tool]
            if change_width:
                width = max(1, min(width + delta, canvas_width))
            if change_height:
                height = max(1, min(height + delta, canvas_height))
            self.brush_sizes[tool] = (width, height)
        elif tool in DIAMETER_TOOLS and change_width and change_height:
            self.diameters[tool] = max(1, min(self.diameters[tool] + delta, max(canvas_width, canvas_height)))
        elif tool in LINE_TOOLS and change_width and change_height:
            # A line has one width, changed by the plain bracket keys only.
            self.line_width = max(1, min(self.line_width + delta, max(canvas_width, canvas_height)))


def slider_fraction(rectangle: pygame.Rect, x: int) -> float:
    """The value (0 to 1, in whole percents) of a slider whose track is `rectangle`, for a pointer at `x`."""
    position = (x - rectangle.x) / max(rectangle.width - 1, 1)
    return round(max(0.0, min(1.0, position)), 2)


class CanvasSession:
    """One press, drag and release of a drawing tool on the canvas.

    `pixels` is what the canvas should look like now; the layout's own pixels are not touched, so the stroke
    can be shown as a preview and thrown away.
    """

    def __init__(
        self,
        layout: CanvasLayout,
        tool: str,
        settings: ToolSettings,
        color: Color,
        start: Coordinate,
        random_source: Optional[RandomSource] = None,
    ):
        if tool not in STROKE_TOOLS:
            raise ValueError(f"{tool!r} does not draw")
        self.layout = layout
        self.tool = tool
        self.color = TRANSPARENT if tool == "eraser" else color
        self.base = layout.pixels
        self.pixels = layout.pixels.copy()
        self.shape = self.base.shape[:2]
        self.brush_width, self.brush_height = settings.brush_sizes.get(tool, (1, 1))
        self.line_width = settings.line_width
        self.start = start
        self.last = start
        self.settings = settings
        self.random_source = random_source if random_source is not None else random.Random()
        self.airbrush: Optional[AirbrushStroke] = None
        if tool in BRUSH_TOOLS:
            self._paint(swept_rectangle_brush_mask(self.shape, start, start, self.brush_width, self.brush_height))
        elif tool == "noise_brush":
            self._stamp_noise(start)
        elif tool == "airbrush":
            self.airbrush = AirbrushStroke(
                self.base,
                self.color,
                settings.diameters["airbrush"],
                settings.fractions["airbrush_hardness"],
                settings.fractions["airbrush_opacity"],
                layout.editable_mask,
            )
            self._stamp_air_path(start, start)
        elif tool == "fill":
            self._paint(flood_fill_mask(self.base, start[0], start[1], layout.editable_mask))
        else:
            self._preview_shape(start)

    def _paint(self, mask: numpy.ndarray) -> None:
        apply_color_to_mask(self.pixels, mask & self.layout.editable_mask, self.color)

    def _stamp_noise(self, pixel: Coordinate) -> None:
        settings = self.settings
        noise_stamp(
            self.pixels,
            pixel[0],
            pixel[1],
            self.color,
            settings.diameters["noise_brush"],
            settings.noise_shape,
            settings.fractions["noise_density"],
            settings.fractions["noise_opacity"],
            self.random_source,
            self.layout.editable_mask,
        )

    def _stamp_air_path(self, start: Coordinate, end: Coordinate) -> None:
        assert self.airbrush is not None
        self.airbrush.stamp_path(start, end)
        self.pixels = self.airbrush.result()

    def _shape_mask(self, pixel: Coordinate) -> numpy.ndarray:
        if self.tool == "line":
            return thick_line_mask(self.shape, self.start, pixel, self.line_width)
        if self.tool == "horizontal_line":
            return thick_line_mask(self.shape, self.start, (pixel[0], self.start[1]), self.line_width)
        if self.tool == "vertical_line":
            return thick_line_mask(self.shape, self.start, (self.start[0], pixel[1]), self.line_width)
        if self.tool == "horizontal_parallelogram":
            return horizontal_parallelogram_mask(self.shape, self.start, pixel, self.line_width)
        return vertical_parallelogram_mask(self.shape, self.start, pixel, self.line_width)

    def _preview_shape(self, pixel: Coordinate) -> None:
        self.pixels = self.base.copy()
        self._paint(self._shape_mask(pixel))

    def move(self, pixel: Coordinate) -> None:
        if pixel == self.last:
            return
        if self.tool in BRUSH_TOOLS:
            self._paint(swept_rectangle_brush_mask(self.shape, self.last, pixel, self.brush_width, self.brush_height))
        elif self.tool == "noise_brush":
            # One stamp for each pixel the pointer moved over; the pixel it started on was stamped already.
            for stamp_pixel in line_pixels(self.last, pixel)[1:]:
                self._stamp_noise(stamp_pixel)
        elif self.tool == "airbrush":
            self._stamp_air_path(self.last, pixel)
        elif self.tool in LINE_TOOLS:
            self._preview_shape(pixel)
        self.last = pixel

    @property
    def changed(self) -> bool:
        return not numpy.array_equal(self.pixels, self.base)


def pick_color(layout: CanvasLayout, pixel: Coordinate) -> Optional[Color]:
    """The color of a canvas pixel, or None outside the canvas or over an empty place."""
    column, row = pixel
    height, width = layout.pixels.shape[:2]
    if not (0 <= column < width and 0 <= row < height) or not layout.editable_mask[row, column]:
        return None
    red, green, blue, alpha = (int(value) for value in layout.pixels[row, column])
    return (red, green, blue, alpha)


@dataclasses.dataclass
class FloatingPaste:
    """Pasted pixels that hover over the canvas, can be moved, and only become an edit when they are placed."""

    pixels: numpy.ndarray
    column: int
    row: int

    @property
    def rectangle(self) -> tuple[int, int, int, int]:
        """(left, top, right, bottom) of the whole picture, even the part that hangs over the canvas edge."""
        height, width = self.pixels.shape[:2]
        return (self.column, self.row, self.column + width, self.row + height)

    def contains(self, pixel: Coordinate) -> bool:
        left, top, right, bottom = self.rectangle
        return left <= pixel[0] < right and top <= pixel[1] < bottom


def parse_hex_color(text: str) -> Optional[Color]:
    """RGB, RGBA, RRGGBB or RRGGBBAA hexadecimal, with or without a leading #. None if it is not one."""
    digits = text.strip().lstrip("#")
    if len(digits) in (3, 4):
        digits = "".join(character * 2 for character in digits)
    if len(digits) not in (6, 8) or any(character not in "0123456789abcdefABCDEF" for character in digits):
        return None
    values = [int(digits[index : index + 2], 16) for index in range(0, len(digits), 2)]
    return (values[0], values[1], values[2], values[3] if len(values) == 4 else 255)


def format_hex_color(color: Color) -> str:
    """#RRGGBB for an opaque color, #RRGGBBAA otherwise."""
    text = "#" + "".join(f"{value:02x}" for value in color)
    return text[:7] if color[3] == 255 else text


class ColorState:
    """The two drawing colors (left and right mouse button), which one is being edited, and recent colors.

    The hue, saturation and value of each color are kept as well as the RGB, because a grey or black color has no
    hue or saturation of its own, and dragging in a color picker must not lose them.
    """

    SLOTS = ("primary", "secondary")

    def __init__(self) -> None:
        self.colors: dict[str, Color] = {"primary": (0, 0, 0, 255), "secondary": (255, 255, 255, 255)}
        self.hsv: dict[str, tuple[float, float, float]] = {"primary": (0.0, 0.0, 0.0), "secondary": (0.0, 0.0, 1.0)}
        self.active_slot = "primary"
        self.history: list[Color] = []

    def get(self, slot: Optional[str] = None) -> Color:
        return self.colors[slot or self.active_slot]

    def get_hsva(self, slot: Optional[str] = None) -> tuple[float, float, float, float]:
        slot = slot or self.active_slot
        hue, saturation, value = self.hsv[slot]
        return (hue, saturation, value, self.colors[slot][3] / 255.0)

    def set_color(self, color: Color, slot: Optional[str] = None) -> None:
        """Set a slot to an RGBA color (typed in, picked with the eye dropper, or taken from the history)."""
        import colorsys

        slot = slot or self.active_slot
        hue, saturation, value = colorsys.rgb_to_hsv(color[0] / 255.0, color[1] / 255.0, color[2] / 255.0)
        old_hue, old_saturation, _ = self.hsv[slot]
        if value == 0:
            hue, saturation = old_hue, old_saturation
        elif saturation == 0:
            hue = old_hue
        self.colors[slot] = tuple(color)  # type: ignore[assignment]
        self.hsv[slot] = (hue, saturation, value)

    def set_hsva(self, hue: float, saturation: float, value: float, alpha: float, slot: Optional[str] = None) -> None:
        """Set a slot from the color picker's hue, saturation, value and alpha (each 0..1)."""
        import colorsys

        slot = slot or self.active_slot
        hue, saturation, value, alpha = (max(0.0, min(1.0, number)) for number in (hue, saturation, value, alpha))
        red, green, blue = (int(round(channel * 255)) for channel in colorsys.hsv_to_rgb(hue, saturation, value))
        self.colors[slot] = (red, green, blue, int(round(alpha * 255)))
        self.hsv[slot] = (hue, saturation, value)

    def remember(self, color: Color) -> None:
        """Add a color that was used to the front of the history of recent colors."""
        color = tuple(color)  # type: ignore[assignment]
        if color in self.history:
            self.history.remove(color)  # type: ignore[arg-type]
        self.history.insert(0, color)  # type: ignore[arg-type]
        del self.history[COLOR_HISTORY_LENGTH:]


class CanvasView:
    """Where the canvas picture is in its area of the window: a whole number of screen pixels per canvas pixel."""

    def __init__(self) -> None:
        self.rect = pygame.Rect(0, 0, 100, 100)
        self.zoom = 1
        self.origin_x = 0
        self.origin_y = 0

    def fit(self, width: int, height: int) -> None:
        """Zoom in as far as fits the whole canvas in the area and center it."""
        self.zoom = max(1, min(MAXIMUM_CANVAS_ZOOM, self.rect.width // max(width, 1), self.rect.height // max(height, 1)))
        self.origin_x = self.rect.centerx - width * self.zoom // 2
        self.origin_y = self.rect.centery - height * self.zoom // 2

    def pixel_at(self, position: tuple[int, int]) -> Coordinate:
        """The canvas pixel under a screen position. May be outside the canvas."""
        return ((position[0] - self.origin_x) // self.zoom, (position[1] - self.origin_y) // self.zoom)

    def pixel_rectangle(self, column: int, row: int, width: int = 1, height: int = 1) -> pygame.Rect:
        return pygame.Rect(
            self.origin_x + column * self.zoom, self.origin_y + row * self.zoom, width * self.zoom, height * self.zoom
        )

    def zoom_by(self, steps: int, anchor: tuple[int, int]) -> None:
        """Zoom in or out, keeping the canvas pixel under `anchor` where it is."""
        new_zoom = max(1, min(MAXIMUM_CANVAS_ZOOM, self.zoom + steps))
        if new_zoom == self.zoom:
            return
        column = (anchor[0] - self.origin_x) / self.zoom
        row = (anchor[1] - self.origin_y) / self.zoom
        self.zoom = new_zoom
        self.origin_x = int(round(anchor[0] - column * new_zoom))
        self.origin_y = int(round(anchor[1] - row * new_zoom))

    def pan_by(self, delta_x: int, delta_y: int) -> None:
        self.origin_x += delta_x
        self.origin_y += delta_y

    def visible_pixels(self, width: int, height: int) -> tuple[int, int, int, int]:
        """(first column, first row, past-last column, past-last row) of the canvas pixels in the area."""
        first_column = max(0, (self.rect.x - self.origin_x) // self.zoom)
        first_row = max(0, (self.rect.y - self.origin_y) // self.zoom)
        last_column = min(width, -(-(self.rect.right - self.origin_x) // self.zoom))
        last_row = min(height, -(-(self.rect.bottom - self.origin_y) // self.zoom))
        return (first_column, first_row, last_column, last_row)


# ----------------------------------------------------------------------------------------------------------
# User interface: theme, text and the grid of tiles
# ----------------------------------------------------------------------------------------------------------

USER_INTERFACE_SCALE = 1.0
DRAG_START_DISTANCE = 4
MINIMUM_ZOOM = 1
MAXIMUM_ZOOM = 24


def scaled(value: float) -> int:
    """A size in pixels adjusted for the screen's scaling (1 on an ordinary screen)."""
    return max(1, int(round(value * USER_INTERFACE_SCALE)))


class Theme:
    BACKGROUND = (22, 24, 28)
    PANEL = (30, 33, 38)
    PANEL_BORDER = (52, 56, 64)
    TEXT = (218, 222, 228)
    TEXT_DIMMED = (140, 146, 156)
    ACCENT = (90, 170, 255)
    SLIDER_FILL = (40, 78, 118)
    BUTTON = (50, 55, 64)
    BUTTON_HOVERED = (66, 73, 86)
    DISABLED_TEXT = (96, 100, 108)
    FIELD = (18, 20, 24)
    ERROR = (230, 96, 96)
    ERROR_BACKGROUND = (74, 30, 32)
    UNSAVED = (255, 170, 60)
    VALID = (96, 214, 136)
    INVALID = (232, 92, 92)
    CHECKER_LIGHT = (66, 68, 74)
    CHECKER_DARK = (52, 54, 60)
    TOOLTIP_BACKGROUND = (10, 11, 14)


class Fonts:
    def __init__(self) -> None:
        names = "segoeui,arial,helvetica,dejavusans"
        self.small = pygame.font.SysFont(names, scaled(13))
        self.normal = pygame.font.SysFont(names, scaled(15))
        self.large = pygame.font.SysFont(names, scaled(20))


def draw_text(
    surface: pygame.Surface,
    font: pygame.font.Font,
    text: str,
    color: tuple[int, int, int],
    position: tuple[int, int],
    anchor: str = "topleft",
) -> pygame.Rect:
    rendered = font.render(text, True, color)
    rectangle = rendered.get_rect(**{anchor: position})
    surface.blit(rendered, rectangle)
    return rectangle


def wrap_text(font: pygame.font.Font, text: str, width: int) -> list[str]:
    """Break `text` into lines no wider than `width` pixels (a single longer word keeps its own line)."""
    lines: list[str] = []
    for paragraph in text.split("\n"):
        line = ""
        for word in paragraph.split(" "):
            candidate = word if not line else f"{line} {word}"
            if line and font.size(candidate)[0] > width:
                lines.append(line)
                line = word
            else:
                line = candidate
        lines.append(line)
    return lines


class SystemTextClipboard:
    """Plain text on the operating system's clipboard, through tkinter (which is only loaded when first needed)."""

    def __init__(self) -> None:
        self._root = None

    def _tk_root(self):
        if self._root is None:
            import tkinter

            self._root = tkinter.Tk()
            self._root.withdraw()
        return self._root

    def get_text(self) -> str:
        try:
            return str(self._tk_root().clipboard_get())
        except Exception:  # a clipboard that cannot be read must never crash the editor
            return ""

    def set_text(self, text: str) -> None:
        try:
            root = self._tk_root()
            root.clipboard_clear()
            root.clipboard_append(text)
            root.update()
        except Exception:  # see get_text
            logger.warning("could not write to the clipboard")


def encode_dib(pixels: numpy.ndarray) -> bytes:
    """A device independent bitmap with a version 5 header: 32 bits per pixel, blue green red alpha, bottom row
    first, the way the Windows clipboard keeps pictures that have transparency."""
    import struct

    height, width = pixels.shape[:2]
    bitmap_info_header = struct.pack(
        "<IiiHHIIiiII", 124, width, height, 1, 32, 3, width * height * 4, 0, 0, 0, 0
    )
    masks = struct.pack("<IIII", 0x00FF0000, 0x0000FF00, 0x000000FF, 0xFF000000)
    color_space = struct.pack("<I", 0x73524742)  # "sRGB"
    color_space_details = bytes(36) + bytes(12) + struct.pack("<I", 4) + bytes(12)  # endpoints, gamma, intent, profile
    header = bitmap_info_header + masks + color_space + color_space_details
    assert len(header) == 124
    blue_green_red_alpha = pixels[::-1][..., [2, 1, 0, 3]]
    return header + numpy.ascontiguousarray(blue_green_red_alpha).tobytes()


def _picture_to_pixels(picture: Image.Image) -> numpy.ndarray:
    return numpy.array(picture.convert("RGBA"), dtype=numpy.uint8)


def decode_image_bytes(data: bytes, formats: Optional[list[str]] = None) -> Optional[numpy.ndarray]:
    """RGBA pixels of picture data (a PNG, or any other format Pillow reads), or None if it is not a picture."""
    try:
        with Image.open(io.BytesIO(data), formats=formats) as picture:
            return _picture_to_pixels(picture)
    except (OSError, ValueError, SyntaxError, Image.DecompressionBombError):
        return None


def decode_dib(data: bytes) -> Optional[numpy.ndarray]:
    """RGBA pixels of a device independent bitmap as the Windows clipboard holds it (no file header).

    Windows puts color masks after the header even behind a version 5 header, which already holds them, so the
    pixels are taken to be the last bytes of the data rather than found by adding up the sizes of the parts.
    The data is given to Pillow as a bitmap file that says where the pixels begin.
    """
    import struct

    if len(data) < 40:
        return None
    header_size, width, height, _planes, bit_count, _compression = struct.unpack_from("<IiiHHI", data)
    pixel_bytes = (width * bit_count + 31) // 32 * 4 * abs(height)
    if header_size < 40 or header_size > len(data) or width <= 0 or height == 0 or bit_count <= 0:
        return None
    offset = max(len(data) - pixel_bytes, header_size)
    file_header = b"BM" + struct.pack("<IHHI", 14 + len(data), 0, 0, 14 + offset)
    return decode_image_bytes(file_header + data, ["BMP"])


class SystemImageClipboard:
    """Pictures on the operating system's clipboard. Only Windows is supported: elsewhere nothing is ever read
    and writing reports failure, and the editor then pastes what it copied itself.

    Pictures are written as PNG (which keeps transparency in programs that read it) and as a bitmap with an alpha
    channel. A picture is read from PNG first, then from a bitmap.
    """

    CF_DIB = 8
    CF_DIBV5 = 17
    GMEM_MOVEABLE = 0x0002
    OPEN_ATTEMPTS = 20

    def __init__(self) -> None:
        self._functions = None

    def _api(self):
        if self._functions is None:
            import ctypes
            from ctypes import wintypes

            user32 = ctypes.WinDLL("user32", use_last_error=True)
            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            # Handles and pointers are 64 bits wide: without these declarations ctypes would cut them to 32.
            user32.OpenClipboard.argtypes = [wintypes.HWND]
            user32.OpenClipboard.restype = wintypes.BOOL
            user32.CloseClipboard.restype = wintypes.BOOL
            user32.EmptyClipboard.restype = wintypes.BOOL
            user32.IsClipboardFormatAvailable.argtypes = [wintypes.UINT]
            user32.IsClipboardFormatAvailable.restype = wintypes.BOOL
            user32.GetClipboardData.argtypes = [wintypes.UINT]
            user32.GetClipboardData.restype = ctypes.c_void_p
            user32.SetClipboardData.argtypes = [wintypes.UINT, ctypes.c_void_p]
            user32.SetClipboardData.restype = ctypes.c_void_p
            user32.RegisterClipboardFormatW.argtypes = [wintypes.LPCWSTR]
            user32.RegisterClipboardFormatW.restype = wintypes.UINT
            kernel32.GlobalAlloc.argtypes = [wintypes.UINT, ctypes.c_size_t]
            kernel32.GlobalAlloc.restype = ctypes.c_void_p
            kernel32.GlobalLock.argtypes = [ctypes.c_void_p]
            kernel32.GlobalLock.restype = ctypes.c_void_p
            kernel32.GlobalUnlock.argtypes = [ctypes.c_void_p]
            kernel32.GlobalUnlock.restype = wintypes.BOOL
            kernel32.GlobalSize.argtypes = [ctypes.c_void_p]
            kernel32.GlobalSize.restype = ctypes.c_size_t
            kernel32.GlobalFree.argtypes = [ctypes.c_void_p]
            kernel32.GlobalFree.restype = ctypes.c_void_p
            self._functions = (ctypes, user32, kernel32)
        return self._functions

    def _window_handle(self):
        try:
            return pygame.display.get_wm_info().get("window")
        except pygame.error:
            return None

    def _open(self, user32) -> bool:
        import time

        for _ in range(self.OPEN_ATTEMPTS):  # another program may hold the clipboard for a moment
            if user32.OpenClipboard(self._window_handle()):
                return True
            time.sleep(0.01)
        return False

    def get_image(self) -> Optional[numpy.ndarray]:
        if sys.platform != "win32":
            return None
        try:
            ctypes, user32, kernel32 = self._api()
            if not self._open(user32):
                return None
            try:
                png_format = user32.RegisterClipboardFormatW("PNG")
                for clipboard_format in (png_format, self.CF_DIBV5, self.CF_DIB):
                    if not user32.IsClipboardFormatAvailable(clipboard_format):
                        continue
                    handle = user32.GetClipboardData(clipboard_format)
                    if not handle:
                        continue
                    pointer = kernel32.GlobalLock(handle)
                    if not pointer:
                        continue
                    try:
                        data = ctypes.string_at(pointer, kernel32.GlobalSize(handle))
                    finally:
                        kernel32.GlobalUnlock(handle)
                    pixels = decode_image_bytes(data, ["PNG"]) if clipboard_format == png_format else decode_dib(data)
                    if pixels is not None:
                        return pixels
                return None
            finally:
                user32.CloseClipboard()
        except Exception:  # a clipboard that cannot be read must never crash the editor
            logger.warning("could not read a picture from the clipboard", exc_info=True)
            return None

    def set_image(self, pixels: numpy.ndarray) -> bool:
        """Put the picture on the clipboard in place of what was there. False if that did not work."""
        if sys.platform != "win32":
            return False
        try:
            ctypes, user32, kernel32 = self._api()
            if not self._open(user32):
                return False
            try:
                user32.EmptyClipboard()
                written = False
                for clipboard_format, data in (
                    (user32.RegisterClipboardFormatW("PNG"), encode_png(pixels)),
                    (self.CF_DIBV5, encode_dib(pixels)),
                ):
                    handle = kernel32.GlobalAlloc(self.GMEM_MOVEABLE, len(data))
                    if not handle:
                        continue
                    pointer = kernel32.GlobalLock(handle)
                    if not pointer:
                        kernel32.GlobalFree(handle)
                        continue
                    ctypes.memmove(pointer, data, len(data))
                    kernel32.GlobalUnlock(handle)
                    if user32.SetClipboardData(clipboard_format, handle):
                        written = True  # the clipboard owns the memory now
                    else:
                        kernel32.GlobalFree(handle)
                return written
            finally:
                user32.CloseClipboard()
        except Exception:  # see get_image
            logger.warning("could not write a picture to the clipboard", exc_info=True)
            return False


class TextField:
    """A single line of editable text with a cursor and a selection. It handles keys but does not draw itself."""

    def __init__(
        self,
        text: str = "",
        accepted_characters: Optional[Callable[[str], bool]] = None,
        maximum_length: int = 255,
        clipboard: Optional[object] = None,
    ):
        self.accepted_characters = accepted_characters
        self.maximum_length = maximum_length
        self.clipboard = clipboard if clipboard is not None else SystemTextClipboard()
        self.text = text
        self.cursor = len(text)
        self.anchor = len(text)

    @property
    def selection(self) -> tuple[int, int]:
        return (min(self.cursor, self.anchor), max(self.cursor, self.anchor))

    @property
    def has_selection(self) -> bool:
        return self.cursor != self.anchor

    @property
    def selected_text(self) -> str:
        start, end = self.selection
        return self.text[start:end]

    def set_text(self, text: str) -> None:
        self.text = text[: self.maximum_length]
        self.cursor = self.anchor = len(self.text)

    def select(self, start: int, end: int) -> None:
        self.anchor, self.cursor = start, end

    def select_all(self) -> None:
        self.select(0, len(self.text))

    def _move_cursor(self, position: int, extend_selection: bool) -> None:
        self.cursor = max(0, min(position, len(self.text)))
        if not extend_selection:
            self.anchor = self.cursor

    def delete_selection(self) -> None:
        start, end = self.selection
        self.text = self.text[:start] + self.text[end:]
        self.cursor = self.anchor = start

    def insert_text(self, text: str) -> None:
        if self.accepted_characters is not None:
            text = "".join(character for character in text if self.accepted_characters(character))
        text = "".join(character for character in text if character.isprintable())
        start, end = self.selection
        room = self.maximum_length - (len(self.text) - (end - start))
        text = text[: max(room, 0)]
        self.text = self.text[:start] + text + self.text[end:]
        self.cursor = self.anchor = start + len(text)

    def handle_key_down(self, key: int, modifiers: int) -> bool:
        """Edit with a key press. Returns True if the key was used."""
        control = bool(modifiers & pygame.KMOD_CTRL)
        shift = bool(modifiers & pygame.KMOD_SHIFT)
        if control and key == pygame.K_a:
            self.select_all()
        elif control and key == pygame.K_c:
            if self.has_selection:
                self.clipboard.set_text(self.selected_text)
        elif control and key == pygame.K_x:
            if self.has_selection:
                self.clipboard.set_text(self.selected_text)
                self.delete_selection()
        elif control and key == pygame.K_v:
            self.insert_text(self.clipboard.get_text().replace("\r", "").replace("\n", " "))
        elif key == pygame.K_LEFT:
            if self.has_selection and not shift:
                self._move_cursor(self.selection[0], False)
            else:
                self._move_cursor(self.cursor - 1, shift)
        elif key == pygame.K_RIGHT:
            if self.has_selection and not shift:
                self._move_cursor(self.selection[1], False)
            else:
                self._move_cursor(self.cursor + 1, shift)
        elif key == pygame.K_HOME:
            self._move_cursor(0, shift)
        elif key == pygame.K_END:
            self._move_cursor(len(self.text), shift)
        elif key == pygame.K_BACKSPACE:
            if not self.has_selection:
                self._move_cursor(self.cursor - 1, True)
            self.delete_selection()
        elif key == pygame.K_DELETE:
            if not self.has_selection:
                self._move_cursor(self.cursor + 1, True)
            self.delete_selection()
        else:
            return False
        return True

    def draw(
        self,
        surface: pygame.Surface,
        rectangle: pygame.Rect,
        font: pygame.font.Font,
        focused: bool,
        caret_visible: bool,
        invalid: bool = False,
    ) -> None:
        pygame.draw.rect(surface, Theme.FIELD, rectangle, border_radius=3)
        border = Theme.ERROR if invalid else (Theme.ACCENT if focused else Theme.PANEL_BORDER)
        pygame.draw.rect(surface, border, rectangle, width=1, border_radius=3)
        padding = scaled(6)
        text_origin = (rectangle.x + padding, rectangle.centery)
        if focused and self.has_selection:
            start, end = self.selection
            left = text_origin[0] + font.size(self.text[:start])[0]
            width = font.size(self.text[start:end])[0]
            pygame.draw.rect(surface, (44, 84, 130), (left, rectangle.y + 3, width, rectangle.height - 6))
        draw_text(surface, font, self.text, Theme.TEXT, text_origin, "midleft")
        if focused and caret_visible:
            caret_x = text_origin[0] + font.size(self.text[: self.cursor])[0]
            pygame.draw.line(surface, Theme.TEXT, (caret_x, rectangle.y + 4), (caret_x, rectangle.bottom - 5))


def draw_button(
    surface: pygame.Surface,
    rectangle: pygame.Rect,
    label: str,
    font: pygame.font.Font,
    hovered: bool,
    enabled: bool = True,
) -> None:
    fill = Theme.BUTTON_HOVERED if hovered and enabled else Theme.BUTTON
    pygame.draw.rect(surface, fill, rectangle, border_radius=4)
    pygame.draw.rect(surface, Theme.PANEL_BORDER, rectangle, width=1, border_radius=4)
    draw_text(surface, font, label, Theme.TEXT if enabled else Theme.DISABLED_TEXT, rectangle.center, "center")


def draw_slider(surface: pygame.Surface, rectangle: pygame.Rect, label: str, fraction: float, font: pygame.font.Font) -> None:
    """A track filled up to `fraction`, with its name and value in whole percents written on it."""
    pygame.draw.rect(surface, Theme.FIELD, rectangle, border_radius=4)
    filled = rectangle.copy()
    filled.width = int(round(rectangle.width * fraction))
    if filled.width > 0:
        pygame.draw.rect(surface, Theme.SLIDER_FILL, filled, border_radius=4)
    pygame.draw.rect(surface, Theme.PANEL_BORDER, rectangle, width=1, border_radius=4)
    draw_text(surface, font, f"{label} {int(round(fraction * 100))}%", Theme.TEXT, rectangle.center, "center")


def draw_dashed_outline(surface: pygame.Surface, rectangle: pygame.Rect) -> None:
    """A rectangle of black and white dashes, visible on any picture."""
    pygame.draw.rect(surface, (0, 0, 0), rectangle, width=1)
    dash = scaled(4)
    left, top, right, bottom = rectangle.left, rectangle.top, rectangle.right - 1, rectangle.bottom - 1
    for start in range(left, right + 1, 2 * dash):
        pygame.draw.line(surface, (255, 255, 255), (start, top), (min(start + dash - 1, right), top))
        pygame.draw.line(surface, (255, 255, 255), (start, bottom), (min(start + dash - 1, right), bottom))
    for start in range(top, bottom + 1, 2 * dash):
        pygame.draw.line(surface, (255, 255, 255), (left, start), (left, min(start + dash - 1, bottom)))
        pygame.draw.line(surface, (255, 255, 255), (right, start), (right, min(start + dash - 1, bottom)))


# ----------------------------------------------------------------------------------------------------------
# The grid of tiles: geometry, scrolling, and what is under the pointer
# ----------------------------------------------------------------------------------------------------------


def default_zoom(tile_size: int) -> int:
    """The whole number of screen pixels per tile pixel that makes a tile about 64 pixels across."""
    return max(MINIMUM_ZOOM, min(MAXIMUM_ZOOM, round(scaled(64) / tile_size)))


@dataclasses.dataclass(frozen=True)
class GridGeometry:
    """Where tiles are in the content of a panel (before scrolling), in screen pixels.

    Tiles are `cell` pixels square and `gap` pixels of the panel's background apart, so empty places and the
    spaces between tiles look the same.
    """

    tile_size: int
    zoom: int

    @property
    def cell(self) -> int:
        return self.tile_size * self.zoom

    @property
    def gap(self) -> int:
        return max(3, self.cell // 8)

    @property
    def pitch(self) -> int:
        return self.cell + self.gap

    def tile_origin(self, coordinate: Coordinate) -> tuple[int, int]:
        column, row = coordinate
        return (self.gap + column * self.pitch, self.gap + row * self.pitch)

    def exact_coordinate(self, x: int, y: int) -> Optional[Coordinate]:
        """The place whose tile covers this point, or None for a point in a gap or outside."""
        if x < self.gap or y < self.gap:
            return None
        column, column_offset = divmod(x - self.gap, self.pitch)
        row, row_offset = divmod(y - self.gap, self.pitch)
        if column_offset >= self.cell or row_offset >= self.cell:
            return None
        return (column, row)

    def nearest_coordinate(self, x: int, y: int) -> Coordinate:
        """The place this point belongs to: each place owns half of the gap around its tile. May be negative."""
        return ((x - self.gap // 2) // self.pitch, (y - self.gap // 2) // self.pitch)

    def content_size(self, columns: int, rows: int) -> tuple[int, int]:
        return (self.gap + columns * self.pitch, self.gap + rows * self.pitch)


class TileGridPanel:
    """A scrolling, zoomable grid of tile places inside a rectangle of the window."""

    def __init__(self, tile_size: int):
        self.rect = pygame.Rect(0, 0, 100, 100)
        self.scroll_x = 0
        self.scroll_y = 0
        self.set_tile_size(tile_size)

    def set_tile_size(self, tile_size: int) -> None:
        self.tile_size = tile_size
        self.zoom = default_zoom(tile_size)
        self.scroll_x = self.scroll_y = 0

    @property
    def geometry(self) -> GridGeometry:
        return GridGeometry(self.tile_size, self.zoom)

    def extent(self) -> tuple[int, int]:
        """(columns, rows) of places to show."""
        raise NotImplementedError

    def clamp_scroll(self) -> None:
        content_width, content_height = self.geometry.content_size(*self.extent())
        self.scroll_x = max(0, min(self.scroll_x, content_width - self.rect.width))
        self.scroll_y = max(0, min(self.scroll_y, content_height - self.rect.height))

    def content_point(self, position: tuple[int, int]) -> tuple[int, int]:
        return (position[0] - self.rect.x + self.scroll_x, position[1] - self.rect.y + self.scroll_y)

    def tile_screen_rect(self, coordinate: Coordinate) -> pygame.Rect:
        x, y = self.geometry.tile_origin(coordinate)
        return pygame.Rect(x - self.scroll_x + self.rect.x, y - self.scroll_y + self.rect.y, self.geometry.cell, self.geometry.cell)

    def scroll_by(self, delta_x: int, delta_y: int) -> None:
        self.scroll_x += delta_x
        self.scroll_y += delta_y
        self.clamp_scroll()

    def zoom_by(self, steps: int, anchor: tuple[int, int]) -> None:
        """Zoom in or out by whole steps, keeping the content under `anchor` (a screen position) where it is."""
        new_zoom = max(MINIMUM_ZOOM, min(MAXIMUM_ZOOM, self.zoom + steps))
        if new_zoom == self.zoom:
            return
        old_pitch = self.geometry.pitch
        content_x, content_y = self.content_point(anchor)
        self.zoom = new_zoom
        ratio = self.geometry.pitch / old_pitch
        self.scroll_x = int(content_x * ratio) - (anchor[0] - self.rect.x)
        self.scroll_y = int(content_y * ratio) - (anchor[1] - self.rect.y)
        self.clamp_scroll()

    def visible_coordinates(self) -> Iterable[Coordinate]:
        geometry = self.geometry
        columns, rows = self.extent()
        first_column = max(0, (self.scroll_x - geometry.gap) // geometry.pitch)
        last_column = min(columns - 1, (self.scroll_x + self.rect.width) // geometry.pitch)
        first_row = max(0, (self.scroll_y - geometry.gap) // geometry.pitch)
        last_row = min(rows - 1, (self.scroll_y + self.rect.height) // geometry.pitch)
        for row in range(first_row, last_row + 1):
            for column in range(first_column, last_column + 1):
                yield (column, row)


class AtlasPanel(TileGridPanel):
    def __init__(self, project: AtlasProject):
        self.project = project
        super().__init__(project.tile_size)

    def extent(self) -> tuple[int, int]:
        return atlas_extent(self.project.tiles)

    def tile_at(self, position: tuple[int, int]) -> Optional[Coordinate]:
        coordinate = self.geometry.exact_coordinate(*self.content_point(position))
        return coordinate if coordinate in self.project.tiles else None

    def drop_coordinate(self, position: tuple[int, int]) -> Coordinate:
        return self.geometry.nearest_coordinate(*self.content_point(position))


class UnplacedPanel(TileGridPanel):
    """The tiles whose files are in the folder but not in atlas.json, in rows that wrap at the panel's width."""

    def __init__(self, project: AtlasProject):
        self.project = project
        super().__init__(project.tile_size)

    def columns(self) -> int:
        geometry = self.geometry
        return max(1, (self.rect.width - geometry.gap) // geometry.pitch)

    def extent(self) -> tuple[int, int]:
        columns = self.columns()
        return (columns, max(1, -(-len(self.project.unplaced) // columns)))

    def coordinate_of_index(self, index: int) -> Coordinate:
        columns = self.columns()
        return (index % columns, index // columns)

    def tile_at(self, position: tuple[int, int]) -> Optional[int]:
        coordinate = self.geometry.exact_coordinate(*self.content_point(position))
        if coordinate is None:
            return None
        index = coordinate[1] * self.columns() + coordinate[0]
        return index if coordinate[0] < self.columns() and index < len(self.project.unplaced) else None


# ----------------------------------------------------------------------------------------------------------
# Clicking and dragging tiles
# ----------------------------------------------------------------------------------------------------------


@dataclasses.dataclass
class Drag:
    source: str  # "atlas", "unplaced", or "background" (a click on empty space, which cannot be dragged)
    start_position: tuple[int, int]
    grabbed_coordinate: Optional[Coordinate] = None
    unplaced_index: Optional[int] = None
    # What a release without dragging does to the selection: "select_only", "toggle_off", "clear" or None.
    click_action: Optional[str] = None
    started: bool = False
    pointer: tuple[int, int] = (0, 0)
    # Where it would land: a place in the atlas, or "unplaced" for the section below.
    drop_coordinate: Optional[Coordinate] = None
    drops_on_unplaced: bool = False
    drop_is_valid: bool = False


class AtlasInteraction:
    """Mouse handling for the atlas and the unplaced section: selecting, and dragging tiles to rearrange them.

    It knows nothing about drawing; the window passes in mouse positions and reads `drag` to draw feedback.
    """

    def __init__(self, project: AtlasProject, atlas_panel: AtlasPanel, unplaced_panel: UnplacedPanel):
        self.project = project
        self.atlas_panel = atlas_panel
        self.unplaced_panel = unplaced_panel
        self.drag: Optional[Drag] = None

    @property
    def dragging(self) -> bool:
        return self.drag is not None and self.drag.started

    def mouse_down(self, position: tuple[int, int], control_held: bool) -> None:
        selection = self.project.selection
        if self.atlas_panel.rect.collidepoint(position):
            coordinate = self.atlas_panel.tile_at(position)
            if coordinate is None:
                self.drag = Drag("background", position, click_action=None if control_held else "clear")
                return
            click_action = None
            if control_held:
                if coordinate in selection:
                    click_action = "toggle_off"
                else:
                    selection.add(coordinate)
            elif coordinate in selection:
                click_action = "select_only"
            else:
                self.project.select_only(coordinate)
            self.drag = Drag("atlas", position, grabbed_coordinate=coordinate, click_action=click_action)
        elif self.unplaced_panel.rect.collidepoint(position):
            index = self.unplaced_panel.tile_at(position)
            if index is not None:
                self.drag = Drag("unplaced", position, unplaced_index=index)

    def mouse_move(self, position: tuple[int, int]) -> None:
        drag = self.drag
        if drag is None or drag.source == "background":
            return
        if not drag.started:
            distance = max(abs(position[0] - drag.start_position[0]), abs(position[1] - drag.start_position[1]))
            if distance < scaled(DRAG_START_DISTANCE):
                return
            drag.started = True
            drag.click_action = None
        drag.pointer = position
        drag.drop_coordinate = None
        drag.drops_on_unplaced = False
        drag.drop_is_valid = False
        if drag.source == "atlas":
            if self.unplaced_panel.rect.collidepoint(position):
                drag.drops_on_unplaced = True
                drag.drop_is_valid = self.project.can_return_to_unplaced(self.project.selection)
            elif self.atlas_panel.rect.collidepoint(position):
                drag.drop_coordinate = self.atlas_panel.drop_coordinate(position)
                moves = plan_group_move(
                    self.project.tiles, self.project.selection, drag.grabbed_coordinate, drag.drop_coordinate
                )
                drag.drop_is_valid = moves is not None
        elif self.atlas_panel.rect.collidepoint(position):
            drag.drop_coordinate = self.atlas_panel.drop_coordinate(position)
            drag.drop_is_valid = can_place_unplaced_tile(self.project.tiles, drag.drop_coordinate)

    def mouse_up(self, position: tuple[int, int]) -> None:
        """Finish the click or drag. Raises OSError if atlas.json could not be written (the change is undone)."""
        drag, self.drag = self.drag, None
        if drag is None:
            return
        if drag.started:
            self.drag = drag
            self.mouse_move(position)
            self.drag = None
            if not drag.drop_is_valid:
                return
            if drag.source == "atlas":
                if drag.drops_on_unplaced:
                    self.project.return_to_unplaced(self.project.selection)
                else:
                    self.project.move_selection(drag.grabbed_coordinate, drag.drop_coordinate)
            else:
                self.project.place_unplaced_tile(drag.unplaced_index, drag.drop_coordinate)
            return
        if drag.click_action == "select_only":
            self.project.select_only(drag.grabbed_coordinate)
        elif drag.click_action == "toggle_off":
            self.project.toggle_selection(drag.grabbed_coordinate)
        elif drag.click_action == "clear":
            self.project.clear_selection()

    def cancel(self) -> None:
        self.drag = None


# ----------------------------------------------------------------------------------------------------------
# The editor on the right: canvas, tools and colors
# ----------------------------------------------------------------------------------------------------------


@dataclasses.dataclass
class EditorAreas:
    header: pygame.Rect
    canvas: pygame.Rect
    tool_buttons: dict[str, pygame.Rect]
    options: pygame.Rect
    controls: pygame.Rect
    square: pygame.Rect
    hue_bar: pygame.Rect
    alpha_bar: pygame.Rect
    primary_swatch: pygame.Rect
    secondary_swatch: pygame.Rect
    hex_field: pygame.Rect
    history: list[pygame.Rect]


def compute_editor_areas(rect: pygame.Rect, tools: list[str]) -> EditorAreas:
    """Place the parts of the editor: the name above, the canvas, the tool buttons, the color picker below."""
    margin, gap = scaled(10), scaled(6)
    width = rect.width - 2 * margin
    header = pygame.Rect(rect.x + margin, rect.y + margin, width, scaled(30))
    square_size = scaled(120)
    color_top = rect.bottom - margin - square_size
    options = pygame.Rect(rect.x + margin, color_top - scaled(30), width, scaled(22))
    # The sliders and the shape menu of the noise brush and the air brush; empty for the other tools.
    controls = pygame.Rect(rect.x + margin, options.y - gap - scaled(24), width, scaled(24))
    columns, button_height = 4, scaled(28)
    rows = -(-len(tools) // columns)
    tools_top = controls.y - gap - rows * button_height - (rows - 1) * gap
    button_width = (width - (columns - 1) * gap) // columns
    tool_buttons = {
        tool: pygame.Rect(
            rect.x + margin + (index % columns) * (button_width + gap),
            tools_top + (index // columns) * (button_height + gap),
            button_width,
            button_height,
        )
        for index, tool in enumerate(tools)
    }
    canvas_top = header.bottom + gap
    canvas = pygame.Rect(rect.x + margin, canvas_top, width, max(scaled(60), tools_top - scaled(8) - canvas_top))
    left = rect.x + margin
    square = pygame.Rect(left, color_top, square_size, square_size)
    hue_bar = pygame.Rect(square.right + gap, color_top, scaled(16), square_size)
    alpha_bar = pygame.Rect(hue_bar.right + gap, color_top, scaled(16), square_size)
    swatch_left = alpha_bar.right + scaled(20)
    swatch_size = scaled(48)
    primary_swatch = pygame.Rect(swatch_left, color_top, swatch_size, swatch_size)
    secondary_swatch = pygame.Rect(primary_swatch.right + gap, color_top, swatch_size, swatch_size)
    hex_field = pygame.Rect(swatch_left, color_top + swatch_size + gap, 2 * swatch_size + gap, scaled(28))
    history_left = secondary_swatch.right + scaled(16)
    history_swatch = scaled(22)
    history_columns = max(2, (rect.right - margin - history_left) // (history_swatch + 4))
    history = []
    for index in range(COLOR_HISTORY_LENGTH):
        column, row = index % history_columns, index // history_columns
        if (row + 1) * (history_swatch + 4) > square_size:
            break
        history.append(pygame.Rect(history_left + column * (history_swatch + 4), color_top + row * (history_swatch + 4), history_swatch, history_swatch))
    return EditorAreas(header, canvas, tool_buttons, options, controls, square, hue_bar, alpha_bar, primary_swatch, secondary_swatch, hex_field, history)


def saturation_value_square(hue: float, size: int) -> pygame.Surface:
    """The picture of a color picker's square for one hue: saturation grows to the right, value falls downward."""
    import colorsys

    saturation = numpy.linspace(0.0, 1.0, size)[None, :, None]
    value = numpy.linspace(1.0, 0.0, size)[:, None, None]
    pure = numpy.array(colorsys.hsv_to_rgb(hue, 1.0, 1.0))
    rgb = (value * ((1.0 - saturation) + saturation * pure) * 255).astype(numpy.uint8)
    return pygame.image.frombuffer(rgb.tobytes(), (size, size), "RGB")


def hue_gradient(width: int, height: int) -> pygame.Surface:
    import colorsys

    column = numpy.array([colorsys.hsv_to_rgb(row / max(height - 1, 1), 1.0, 1.0) for row in range(height)])
    rgb = numpy.repeat((column * 255).astype(numpy.uint8)[:, None, :], width, axis=1)
    return pygame.image.frombuffer(rgb.tobytes(), (width, height), "RGB")


def hatch_surface(width: int, height: int) -> pygame.Surface:
    """Diagonal stripes, drawn where the canvas has no tile."""
    surface = pygame.Surface((width, height))
    surface.fill((20, 22, 26))
    spacing = scaled(10)
    for offset in range(-height, width, spacing):
        pygame.draw.line(surface, (44, 48, 56), (offset, height), (offset + height, 0), 1)
    return surface


class EditorPanel:
    """The right half of the window: the canvas for the selected tiles, the tools and the color picker.

    The window passes in mouse and key events with screen positions; this class draws the canvas, runs strokes
    through CanvasSession and commits them to the project.
    """

    def __init__(self, project: AtlasProject, fonts: Fonts):
        self.project = project
        self.fonts = fonts
        self.tool = "pencil"
        self.settings = ToolSettings()
        self.colors = ColorState()
        self.view = CanvasView()
        self.layout: Optional[CanvasLayout] = None
        self.session: Optional[CanvasSession] = None
        self.random_source = random.Random()
        self.clipboard = SystemImageClipboard()
        # The pixels of the last copy made here, used when the system clipboard cannot hold or give a picture.
        self.internal_image: Optional[numpy.ndarray] = None
        self.internal_is_newer = False
        # The rectangle chosen with the select tool, (left, top, right, bottom) in canvas pixels, and a pasted
        # picture that has not been placed yet.
        self.selection_rectangle: Optional[tuple[int, int, int, int]] = None
        self.floating: Optional[FloatingPaste] = None
        self.selecting_from: Optional[Coordinate] = None
        self.selection_moved = False
        self.floating_drag: Optional[tuple[Coordinate, int, int]] = None
        self.shape_dropdown_open = False
        self.slider_drag: Optional[str] = None
        self.rect = pygame.Rect(0, 0, 100, 100)
        self.areas = compute_editor_areas(self.rect, list(TOOL_LABELS))
        self.hex_field = TextField("", lambda character: character in "#0123456789abcdefABCDEF", 9)
        self.hex_focused = False
        self.hex_invalid = False
        self.color_drag: Optional[str] = None
        self.panning_from: Optional[tuple[int, int]] = None
        self.pointer = (-1, -1)
        self.message = ""
        self._signature: Optional[tuple] = None
        self._coordinate_signature: Optional[tuple] = None
        self._checker: Optional[pygame.Surface] = None
        self._hatches: dict[tuple[int, int], pygame.Surface] = {}
        self._squares: dict[tuple[int, int], pygame.Surface] = {}
        self._hue_gradients: dict[tuple[int, int], pygame.Surface] = {}

    # State ------------------------------------------------------------------------------------------------

    def set_rect(self, rect: pygame.Rect) -> None:
        self.rect = rect
        self.areas = compute_editor_areas(rect, list(TOOL_LABELS))
        self.view.rect = self.areas.canvas
        self._checker = None
        if self.layout is not None:
            self.view.fit(self.layout.pixels.shape[1], self.layout.pixels.shape[0])

    def sync(self) -> None:
        """Rebuild the canvas when the selection or the pixels of the selected tiles have changed."""
        entries = [
            (coordinate, self.project.tiles[coordinate])
            for coordinate in sorted(self.project.selection, key=reading_order_key)
            if self.project.tiles[coordinate].error is None
        ]
        signature = tuple((coordinate, id(record), record.revision) for coordinate, record in entries)
        if signature == self._signature:
            return
        self._signature = signature
        self.session = None
        if not entries:
            self.layout = None
            self._coordinate_signature = None
            self._forget_selection()
            return
        self.layout = compose_canvas({coordinate: record.pixels for coordinate, record in entries}, self.project.tile_size)
        coordinate_signature = tuple(coordinate for coordinate, _ in entries)
        if coordinate_signature != self._coordinate_signature:
            self._coordinate_signature = coordinate_signature
            self._forget_selection()
            self.view.rect = self.areas.canvas
            self.view.fit(self.layout.pixels.shape[1], self.layout.pixels.shape[0])

    def contains(self, position: tuple[int, int]) -> bool:
        return self.rect.collidepoint(position)

    def _forget_selection(self) -> None:
        """Other tiles are shown: what was selected or pasted on the old canvas means nothing now."""
        self.selection_rectangle = None
        self.floating = None
        self.selecting_from = None
        self.floating_drag = None

    def canvas_shape(self) -> tuple[int, int]:
        return self.layout.pixels.shape[:2] if self.layout is not None else (self.project.tile_size, self.project.tile_size)

    def header_text(self) -> str:
        selected = [self.project.tiles[coordinate] for coordinate in sorted(self.project.selection, key=reading_order_key)]
        if not selected:
            return ""
        marker = "*" if any(record.is_modified for record in selected) else ""
        return f"{selected[0].name if len(selected) == 1 else '(multiple files)'}{marker}"

    def choose_tool(self, tool: str) -> None:
        """Make `tool` the current tool. What the select tool had selected or pasted is finished first."""
        self.shape_dropdown_open = False
        if tool != "select":
            self.finish_selection()
        self.tool = tool

    def control_rectangles(self) -> dict[str, pygame.Rect]:
        """The sliders (named like ToolSettings.fractions) and the shape menu ("noise_shape") of the current tool."""
        if self.tool not in SLIDERS:
            return {}
        names = [name for name, _label in SLIDERS[self.tool]]
        gap = scaled(8)
        area = self.areas.controls
        left = area.x
        rectangles = {}
        if self.tool == "noise_brush":
            menu_width = scaled(110)
            rectangles["noise_shape"] = pygame.Rect(left, area.y, menu_width, area.height)
            left += menu_width + gap
        slider_width = (area.right - left - (len(names) - 1) * gap) // len(names)
        for name in names:
            rectangles[name] = pygame.Rect(left, area.y, slider_width, area.height)
            left += slider_width + gap
        return rectangles

    def shape_option_rectangles(self) -> dict[str, pygame.Rect]:
        """The choices of the shape menu while it is open, stacked above the menu."""
        menu = self.control_rectangles().get("noise_shape")
        if menu is None:
            return {}
        return {
            shape: pygame.Rect(menu.x, menu.y - (len(BRUSH_SHAPES) - index) * menu.height, menu.width, menu.height)
            for index, shape in enumerate(BRUSH_SHAPES)
        }

    # Mouse ------------------------------------------------------------------------------------------------

    def mouse_down(self, button: int, position: tuple[int, int]) -> None:
        self.sync()
        self.pointer = position
        areas = self.areas
        if self.hex_focused and not (button == 1 and areas.hex_field.collidepoint(position)):
            self.apply_hex_field()
            self.hex_focused = False
        if self.shape_dropdown_open:
            # A press closes the menu. Choosing an option or pressing the menu itself is all it does; a press on
            # the canvas must not also draw.
            self.shape_dropdown_open = False
            menu = self.control_rectangles().get("noise_shape")
            if button == 1:
                for shape, rectangle in self.shape_option_rectangles().items():
                    if rectangle.collidepoint(position):
                        self.settings.noise_shape = shape
                        return
            if (menu is not None and menu.collidepoint(position)) or areas.canvas.collidepoint(position):
                return
        if button == 1:
            if areas.hex_field.collidepoint(position):
                self.hex_focused = True
                return
            for tool, rectangle in areas.tool_buttons.items():
                if rectangle.collidepoint(position):
                    self.choose_tool(tool)
                    return
            for name, rectangle in self.control_rectangles().items():
                if rectangle.collidepoint(position):
                    if name == "noise_shape":
                        self.shape_dropdown_open = True
                    else:
                        self.slider_drag = name
                        self.settings.set_fraction(name, slider_fraction(rectangle, position[0]))
                    return
            if areas.primary_swatch.collidepoint(position):
                self.colors.active_slot = "primary"
                return
            if areas.secondary_swatch.collidepoint(position):
                self.colors.active_slot = "secondary"
                return
            for color, rectangle in zip(self.colors.history, areas.history):
                if rectangle.collidepoint(position):
                    self.colors.set_color(color, "primary")
                    self.colors.active_slot = "primary"
                    return
            for name, rectangle in (("square", areas.square), ("hue", areas.hue_bar), ("alpha", areas.alpha_bar)):
                if rectangle.collidepoint(position):
                    self.color_drag = name
                    self._drag_color(position)
                    return
        elif button == 3:
            for color, rectangle in zip(self.colors.history, areas.history):
                if rectangle.collidepoint(position):
                    self.colors.set_color(color, "secondary")
                    return
        if areas.canvas.collidepoint(position):
            if button == 2:
                self.panning_from = position
            elif button in (1, 3) and self.layout is not None:
                self._press_on_canvas(button, position)

    def _press_on_canvas(self, button: int, position: tuple[int, int]) -> None:
        slot = "primary" if button == 1 else "secondary"
        pixel = self.view.pixel_at(position)
        if self.tool == "eyedropper":
            color = pick_color(self.layout, pixel)
            if color is not None:
                self.colors.set_color(color, slot)
            self.color_drag = f"dropper_{slot}"
            return
        if self.tool == "select":
            if button == 1:
                self._press_select(pixel)
            return
        self.session = CanvasSession(
            self.layout, self.tool, self.settings, self.colors.get(slot), pixel, self.random_source
        )

    def _press_select(self, pixel: Coordinate) -> None:
        if self.floating is not None:
            if self.floating.contains(pixel):
                self.floating_drag = (pixel, self.floating.column, self.floating.row)
                return
            self.commit_floating()
        self.selecting_from = pixel
        self.selection_moved = False
        self._set_selection(pixel, pixel)

    def _set_selection(self, corner: Coordinate, other_corner: Coordinate) -> None:
        rectangle = rectangle_from_corners(corner, other_corner, self.canvas_shape())
        left, top, right, bottom = rectangle
        self.selection_rectangle = rectangle if right > left and bottom > top else None

    def mouse_move(self, position: tuple[int, int]) -> None:
        self.pointer = position
        if self.color_drag is not None:
            if self.color_drag.startswith("dropper_") and self.layout is not None:
                color = pick_color(self.layout, self.view.pixel_at(position))
                if color is not None:
                    self.colors.set_color(color, self.color_drag.removeprefix("dropper_"))
            else:
                self._drag_color(position)
        elif self.slider_drag is not None:
            rectangle = self.control_rectangles().get(self.slider_drag)
            if rectangle is not None:
                self.settings.set_fraction(self.slider_drag, slider_fraction(rectangle, position[0]))
        elif self.panning_from is not None:
            self.view.pan_by(position[0] - self.panning_from[0], position[1] - self.panning_from[1])
            self.panning_from = position
        elif self.floating_drag is not None and self.floating is not None:
            pressed, column, row = self.floating_drag
            pixel = self.view.pixel_at(position)
            self.floating.column = column + pixel[0] - pressed[0]
            self.floating.row = row + pixel[1] - pressed[1]
        elif self.selecting_from is not None:
            pixel = self.view.pixel_at(position)
            self.selection_moved = self.selection_moved or pixel != self.selecting_from
            self._set_selection(self.selecting_from, pixel)
        elif self.session is not None:
            self.session.move(self.view.pixel_at(position))

    def mouse_up(self, button: int, position: tuple[int, int]) -> None:
        self.pointer = position
        if button == 2:
            self.panning_from = None
        if self.color_drag is not None and button in (1, 3):
            self.color_drag = None
        if button == 1:
            self.slider_drag = None
            self.floating_drag = None
            if self.selecting_from is not None:
                pixel = self.view.pixel_at(position)
                self.selection_moved = self.selection_moved or pixel != self.selecting_from
                self._set_selection(self.selecting_from, pixel)
                if not self.selection_moved:
                    self.selection_rectangle = None  # a click that did not drag deselects
                self.selecting_from = None
        if self.session is not None and button in (1, 3):
            self.session.move(self.view.pixel_at(position))
            session, self.session = self.session, None
            self._commit(session)

    def _commit(self, session: CanvasSession) -> None:
        if not session.changed:
            return
        try:
            self.project.apply_canvas_edit(session.layout, session.pixels)
        except OSError as error:
            self.message = f"Could not save {ATLAS_FILE_NAME}: {error}"
            return
        if session.tool != "eraser":
            self.colors.remember(session.color)
        self.sync()

    # Selection and clipboard ------------------------------------------------------------------------------

    def _apply_pixels(self, new_pixels: numpy.ndarray) -> bool:
        """Make the canvas look like `new_pixels` as one edit. False if that changes nothing or cannot be saved."""
        layout = self.layout
        if layout is None or numpy.array_equal(new_pixels, layout.pixels):
            return False
        try:
            self.project.apply_canvas_edit(layout, new_pixels)
        except OSError as error:
            self.message = f"Could not save {ATLAS_FILE_NAME}: {error}"
            return False
        self.sync()
        return True

    def select_all(self) -> None:
        self.sync()
        if self.layout is None:
            return
        self.commit_floating()
        self.choose_tool("select")
        height, width = self.layout.pixels.shape[:2]
        self.selection_rectangle = (0, 0, width, height)

    def finish_selection(self) -> None:
        """Place a pasted picture that is still floating and drop the selection."""
        self.commit_floating()
        self.selection_rectangle = None
        self.selecting_from = None

    def commit_floating(self) -> None:
        paste, self.floating, self.floating_drag = self.floating, None, None
        if paste is None or self.layout is None:
            return
        self._apply_pixels(paste_pixels(self.layout.pixels, paste.pixels, paste.column, paste.row, self.layout.editable_mask))
        left, top, right, bottom = paste.rectangle
        self._set_selection((left, top), (right - 1, bottom - 1))

    def cancel_floating(self) -> None:
        self.floating = None
        self.floating_drag = None

    def copy_selection(self, verb: str = "Copied") -> bool:
        """Copy the selected pixels. Nothing is copied, and the clipboard is left alone, without a selection."""
        self.commit_floating()
        if self.layout is None or self.selection_rectangle is None:
            return False
        pixels = crop_pixels(self.layout.pixels, self.selection_rectangle)
        height, width = pixels.shape[:2]
        self.internal_image = pixels
        self.internal_is_newer = not self.clipboard.set_image(pixels)
        self.message = f"{verb} {width} x {height} pixels."
        if self.internal_is_newer:
            self.message += " The system clipboard refused them, so they can be pasted inside this program only."
        return True

    def _clear_selection_pixels(self) -> None:
        if self.layout is None or self.selection_rectangle is None:
            return
        pixels = self.layout.pixels.copy()
        clear_rectangle(pixels, self.selection_rectangle, self.layout.editable_mask)
        self._apply_pixels(pixels)

    def cut_selection(self) -> None:
        if self.copy_selection("Cut"):
            self._clear_selection_pixels()

    def delete_selection(self) -> None:
        if self.floating is not None:
            self.cancel_floating()
        else:
            self._clear_selection_pixels()

    def _paste_source(self) -> Optional[numpy.ndarray]:
        """The picture to paste: what is on the system clipboard, unless this program's own copy is newer."""
        if not self.internal_is_newer:
            image = self.clipboard.get_image()
            if image is not None:
                return image
        return self.internal_image

    def paste(self) -> None:
        self.sync()
        if self.layout is None:
            self.message = "Select a tile to paste into."
            return
        image = self._paste_source()
        if image is None:
            self.message = "Nothing to paste."
            return
        self.commit_floating()
        self.choose_tool("select")
        if self.selection_rectangle is not None:
            column, row = self.selection_rectangle[:2]
        else:
            height, width = self.layout.pixels.shape[:2]
            column, row = self.view.visible_pixels(width, height)[:2]
        self.floating = FloatingPaste(image, column, row)
        self.message = "Drag the pasted picture to move it. Enter places it, Esc throws it away."

    def wheel(self, steps: int, position: tuple[int, int]) -> None:
        if self.layout is not None and self.areas.canvas.collidepoint(position):
            self.view.zoom_by(steps, position)

    def _drag_color(self, position: tuple[int, int]) -> None:
        areas = self.areas
        hue, saturation, value, alpha = self.colors.get_hsva()
        if self.color_drag == "square":
            saturation = (position[0] - areas.square.x) / max(areas.square.width - 1, 1)
            value = 1.0 - (position[1] - areas.square.y) / max(areas.square.height - 1, 1)
        elif self.color_drag == "hue":
            hue = (position[1] - areas.hue_bar.y) / max(areas.hue_bar.height - 1, 1)
        elif self.color_drag == "alpha":
            alpha = 1.0 - (position[1] - areas.alpha_bar.y) / max(areas.alpha_bar.height - 1, 1)
        self.colors.set_hsva(hue, saturation, value, alpha)

    # Keyboard ---------------------------------------------------------------------------------------------

    def apply_hex_field(self) -> None:
        color = parse_hex_color(self.hex_field.text)
        if color is None:
            self.hex_invalid = True
            return
        self.hex_invalid = False
        self.colors.set_color(color)

    def lose_focus(self) -> None:
        """Called when the user clicks somewhere else: a typed color is applied."""
        if self.hex_focused:
            self.apply_hex_field()
            self.hex_focused = False

    def hex_key_down(self, event: pygame.event.Event) -> None:
        if event.key in (pygame.K_RETURN, pygame.K_KP_ENTER):
            self.apply_hex_field()
            if not self.hex_invalid:
                self.hex_focused = False
        elif event.key == pygame.K_ESCAPE:
            self.hex_focused = False
            self.hex_invalid = False
        else:
            self.hex_field.handle_key_down(event.key, event.mod)
            self.hex_invalid = False

    def key_down(self, key: int, modifiers: int, scancode: int = 0) -> bool:
        """Editor shortcuts. Returns True if the key was used."""
        control = bool(modifiers & pygame.KMOD_CTRL)
        shift = bool(modifiers & pygame.KMOD_SHIFT)
        alt = bool(modifiers & pygame.KMOD_ALT)
        self.sync()
        if control and key == pygame.K_a:
            self.select_all()
            return True
        if control and key == pygame.K_c:
            self.copy_selection()
            return True
        if control and key == pygame.K_x:
            self.cut_selection()
            return True
        if control and key == pygame.K_v:
            self.paste()
            return True
        if key == pygame.K_DELETE and not control:
            self.delete_selection()
            return True
        if self.floating is not None:
            if key in (pygame.K_RETURN, pygame.K_KP_ENTER):
                self.commit_floating()
                return True
            if key == pygame.K_ESCAPE or (control and key == pygame.K_z and not shift):
                self.cancel_floating()
                return True
        if control and key == pygame.K_z and not shift:
            return self._history_step(self.project.undo)
        if control and (key == pygame.K_y or (key == pygame.K_z and shift)):
            return self._history_step(self.project.redo)
        if control and key == pygame.K_s:
            self._save(self.project.selection if not shift else list(self.project.tiles))
            return True
        if key in (pygame.K_LEFTBRACKET, pygame.K_RIGHTBRACKET) and not control:
            change_width, change_height = (True, False) if shift and not alt else (False, True) if alt and not shift else (True, True)
            self.settings.adjust(self.tool, 1 if key == pygame.K_RIGHTBRACKET else -1, change_width, change_height, self.canvas_shape())
            return True
        return False

    def _history_step(self, step: Callable[[], bool]) -> bool:
        try:
            step()
        except OSError as error:
            self.message = f"Could not save {ATLAS_FILE_NAME}: {error}"
        self.sync()
        return True

    def _save(self, coordinates: Iterable[Coordinate]) -> None:
        saved, failures = self.project.save_tiles(coordinates)
        if failures:
            self.message = "Could not save " + "; ".join(failures)
        elif saved:
            self.message = f"Saved {saved} tile{'' if saved == 1 else 's'}."
        else:
            self.message = "Nothing to save."
        self.sync()

    # Drawing ----------------------------------------------------------------------------------------------

    def draw(self, surface: pygame.Surface, caret_visible: bool) -> None:
        self.sync()
        pygame.draw.rect(surface, Theme.PANEL, self.rect)
        pygame.draw.rect(surface, Theme.PANEL_BORDER, self.rect, width=1)
        header = self.header_text()
        if header:
            draw_text(surface, self.fonts.large, header, Theme.TEXT, self.areas.header.midleft, "midleft")
        self.draw_canvas(surface)
        self.draw_tools(surface)
        self.draw_colors(surface, caret_visible)
        self.draw_shape_menu(surface)

    def message_for_empty_canvas(self) -> str:
        selected = [self.project.tiles[coordinate] for coordinate in self.project.selection]
        if not selected:
            return "Select a tile in the atlas to edit it."
        if len(selected) == 1:
            return f"This tile cannot be edited: {selected[0].error}."
        return "None of the selected tiles can be edited."

    def draw_canvas(self, surface: pygame.Surface) -> None:
        area = self.areas.canvas
        pygame.draw.rect(surface, Theme.BACKGROUND, area)
        if self.layout is None:
            lines = wrap_text(self.fonts.normal, self.message_for_empty_canvas(), area.width - scaled(40))
            for number, line in enumerate(lines):
                draw_text(surface, self.fonts.normal, line, Theme.TEXT_DIMMED, (area.centerx, area.centery + number * scaled(20)), "midtop")
            return
        layout, view = self.layout, self.view
        pixels = self.session.pixels if self.session is not None else layout.pixels
        if self.floating is not None:
            paste = self.floating
            pixels = paste_pixels(pixels, paste.pixels, paste.column, paste.row, layout.editable_mask)
        height, width = pixels.shape[:2]
        surface.set_clip(area)
        image_rectangle = view.pixel_rectangle(0, 0, width, height)
        visible = image_rectangle.clip(area)
        if self._checker is None or self._checker.get_width() < area.width or self._checker.get_height() < area.height:
            self._checker = checkerboard(max(area.width, area.height) + 16)
        surface.blit(self._checker, visible, pygame.Rect(0, 0, visible.width, visible.height))
        first_column, first_row, last_column, last_row = view.visible_pixels(width, height)
        if last_column > first_column and last_row > first_row:
            crop = numpy.ascontiguousarray(pixels[first_row:last_row, first_column:last_column])
            picture = pygame.image.frombuffer(crop.tobytes(), (crop.shape[1], crop.shape[0]), "RGBA")
            picture = pygame.transform.scale(picture, (crop.shape[1] * view.zoom, crop.shape[0] * view.zoom))
            surface.blit(picture, view.pixel_rectangle(first_column, first_row).topleft)
        self.draw_empty_places(surface, layout)
        self.draw_overlay(surface, layout, width, height, (first_column, first_row, last_column, last_row))
        surface.set_clip(None)

    def draw_empty_places(self, surface: pygame.Surface, layout: CanvasLayout) -> None:
        size = layout.tile_size
        cell_columns, cell_rows = layout.pixels.shape[1] // size, layout.pixels.shape[0] // size
        for cell_row in range(cell_rows):
            for cell_column in range(cell_columns):
                if (layout.first_column + cell_column, layout.first_row + cell_row) in layout.tile_coordinates:
                    continue
                rectangle = self.view.pixel_rectangle(cell_column * size, cell_row * size, size, size)
                if rectangle.colliderect(self.areas.canvas):
                    key = (rectangle.width, rectangle.height)
                    if key not in self._hatches:
                        self._hatches[key] = hatch_surface(*key)
                    surface.blit(self._hatches[key], rectangle)

    def draw_overlay(self, surface: pygame.Surface, layout: CanvasLayout, width: int, height: int, visible: tuple[int, int, int, int]) -> None:
        view, area = self.view, self.areas.canvas
        overlay = pygame.Surface(area.size, pygame.SRCALPHA)
        image = view.pixel_rectangle(0, 0, width, height).move(-area.x, -area.y)
        first_column, first_row, last_column, last_row = visible

        def vertical_line(column: int, color: tuple[int, int, int, int]) -> None:
            x = view.pixel_rectangle(column, 0).x - area.x
            pygame.draw.line(overlay, color, (x, image.top), (x, image.bottom))

        def horizontal_line(row: int, color: tuple[int, int, int, int]) -> None:
            y = view.pixel_rectangle(0, row).y - area.y
            pygame.draw.line(overlay, color, (image.left, y), (image.right, y))

        if view.zoom >= 6:
            for column in range(first_column, last_column + 1):
                vertical_line(column, (150, 156, 168, 46))
            for row in range(first_row, last_row + 1):
                horizontal_line(row, (150, 156, 168, 46))
        size = layout.tile_size
        for column in range(0, width + 1, size):
            vertical_line(column, (90, 170, 255, 150))
        for row in range(0, height + 1, size):
            horizontal_line(row, (90, 170, 255, 150))
        surface.blit(overlay, area.topleft)
        marked = self.floating.rectangle if self.floating is not None else self.selection_rectangle
        if marked is not None:
            left, top, right, bottom = marked
            draw_dashed_outline(surface, view.pixel_rectangle(left, top, right - left, bottom - top))
        if self.tool_uses_outline() and area.collidepoint(self.pointer) and self.session is None:
            self.draw_footprint_outline(surface)

    def tool_uses_outline(self) -> bool:
        return (
            self.panning_from is None
            and self.color_drag is None
            and self.slider_drag is None
            and not self.shape_dropdown_open
            and self.tool != "select"
        )

    def draw_footprint_outline(self, surface: pygame.Surface) -> None:
        column, row = self.view.pixel_at(self.pointer)
        width, height = self.settings.footprint_size(self.tool)
        rectangle = self.view.pixel_rectangle(column - width // 2, row - height // 2, width, height)
        is_round = self.tool == "airbrush" or (self.tool == "noise_brush" and self.settings.noise_shape == "circle")
        draw_outline = pygame.draw.ellipse if is_round and width > 2 else pygame.draw.rect
        draw_outline(surface, (0, 0, 0), rectangle.inflate(2, 2), width=1)
        draw_outline(surface, (255, 255, 255), rectangle, width=1)

    def options_text(self) -> str:
        if self.tool in DIAMETER_TOOLS:
            return f"Diameter {self.settings.diameters[self.tool]}     [ ] change"
        if self.tool == "select":
            if self.floating is not None:
                return "Drag to move     Enter places     Esc throws away"
            return "Ctrl+A all    Ctrl+C copy    Ctrl+X cut    Ctrl+V paste    Delete clear"
        if self.tool in BRUSH_TOOLS:
            width, height = self.settings.brush_sizes[self.tool]
            return f"Size {width} x {height}     [ ] both    Shift+[ ] width    Alt+[ ] height"
        if self.tool in LINE_TOOLS:
            return f"Width {self.settings.line_width}     [ ] change"
        return ""

    def draw_tools(self, surface: pygame.Surface) -> None:
        for tool, rectangle in self.areas.tool_buttons.items():
            hovered = rectangle.collidepoint(self.pointer)
            draw_button(surface, rectangle, TOOL_LABELS[tool], self.fonts.small, hovered)
            if tool == self.tool:
                pygame.draw.rect(surface, Theme.ACCENT, rectangle, width=2, border_radius=4)
        draw_text(surface, self.fonts.small, self.options_text(), Theme.TEXT_DIMMED, self.areas.options.midleft, "midleft")
        labels = dict(SLIDERS.get(self.tool, ()))
        for name, rectangle in self.control_rectangles().items():
            if name == "noise_shape":
                draw_button(surface, rectangle, f"Shape: {self.settings.noise_shape.title()}", self.fonts.small, rectangle.collidepoint(self.pointer))
                tip = rectangle.right - scaled(12)
                middle = rectangle.centery
                pygame.draw.polygon(surface, Theme.TEXT_DIMMED, [(tip - 4, middle - 2), (tip + 4, middle - 2), (tip, middle + 3)])
            else:
                draw_slider(surface, rectangle, labels[name], self.settings.fractions[name], self.fonts.small)

    def draw_shape_menu(self, surface: pygame.Surface) -> None:
        if not self.shape_dropdown_open:
            return
        for shape, rectangle in self.shape_option_rectangles().items():
            draw_button(surface, rectangle, shape.title(), self.fonts.small, rectangle.collidepoint(self.pointer))
            if shape == self.settings.noise_shape:
                pygame.draw.rect(surface, Theme.ACCENT, rectangle, width=2, border_radius=4)

    def draw_swatch(self, surface: pygame.Surface, rectangle: pygame.Rect, color: Color) -> None:
        surface.blit(checkerboard(max(rectangle.size)), rectangle, (0, 0, rectangle.width, rectangle.height))
        swatch = pygame.Surface(rectangle.size, pygame.SRCALPHA)
        swatch.fill(color)
        surface.blit(swatch, rectangle)
        pygame.draw.rect(surface, Theme.PANEL_BORDER, rectangle, width=1)

    def draw_colors(self, surface: pygame.Surface, caret_visible: bool) -> None:
        areas = self.areas
        hue, saturation, value, alpha = self.colors.get_hsva()
        key = (int(hue * 255), areas.square.width)
        if key not in self._squares:
            if len(self._squares) > 64:
                self._squares.clear()
            self._squares[key] = saturation_value_square(key[0] / 255.0, areas.square.width)
        surface.blit(self._squares[key], areas.square)
        pygame.draw.rect(surface, Theme.PANEL_BORDER, areas.square, width=1)
        marker = (areas.square.x + int(saturation * (areas.square.width - 1)), areas.square.y + int((1.0 - value) * (areas.square.height - 1)))
        pygame.draw.circle(surface, (0, 0, 0), marker, scaled(6), 2)
        pygame.draw.circle(surface, (255, 255, 255), marker, scaled(5), 1)
        gradient_key = (areas.hue_bar.width, areas.hue_bar.height)
        if gradient_key not in self._hue_gradients:
            self._hue_gradients[gradient_key] = hue_gradient(*gradient_key)
        surface.blit(self._hue_gradients[gradient_key], areas.hue_bar)
        y = areas.hue_bar.y + int(hue * (areas.hue_bar.height - 1))
        pygame.draw.line(surface, (255, 255, 255), (areas.hue_bar.x - 2, y), (areas.hue_bar.right + 1, y), 2)
        self.draw_alpha_bar(surface, areas.alpha_bar, alpha)
        for slot, rectangle in (("primary", areas.primary_swatch), ("secondary", areas.secondary_swatch)):
            self.draw_swatch(surface, rectangle, self.colors.get(slot))
            if slot == self.colors.active_slot:
                pygame.draw.rect(surface, Theme.ACCENT, rectangle.inflate(4, 4), width=2)
        if not self.hex_focused:
            self.hex_field.set_text(format_hex_color(self.colors.get()))
        self.hex_field.draw(surface, areas.hex_field, self.fonts.normal, self.hex_focused, caret_visible, self.hex_invalid)
        for color, rectangle in zip(self.colors.history, areas.history):
            self.draw_swatch(surface, rectangle, color)

    def draw_alpha_bar(self, surface: pygame.Surface, rectangle: pygame.Rect, alpha: float) -> None:
        surface.blit(checkerboard(max(rectangle.size)), rectangle, (0, 0, rectangle.width, rectangle.height))
        red, green, blue, _ = self.colors.get()
        ramp = numpy.zeros((rectangle.height, rectangle.width, 4), dtype=numpy.uint8)
        ramp[..., 0], ramp[..., 1], ramp[..., 2] = red, green, blue
        ramp[..., 3] = (numpy.linspace(255, 0, rectangle.height)[:, None]).astype(numpy.uint8)
        surface.blit(pygame.image.frombuffer(ramp.tobytes(), rectangle.size, "RGBA"), rectangle)
        y = rectangle.y + int((1.0 - alpha) * (rectangle.height - 1))
        pygame.draw.line(surface, (255, 255, 255), (rectangle.x - 2, y), (rectangle.right + 1, y), 2)


# ----------------------------------------------------------------------------------------------------------
# The window
# ----------------------------------------------------------------------------------------------------------


@dataclasses.dataclass
class WindowLayout:
    atlas_title: pygame.Rect
    atlas: pygame.Rect
    unplaced_title: pygame.Rect
    unplaced: pygame.Rect
    editor: pygame.Rect
    status: pygame.Rect


def compute_layout(size: tuple[int, int]) -> WindowLayout:
    """The window is divided in half: the atlas (with the unplaced tiles below it) on the left, the editor on the right."""
    width, height = size
    margin = scaled(8)
    title_height = scaled(24)
    status_height = scaled(26)
    body_bottom = height - status_height
    left_x = margin
    left_width = width // 2 - margin - margin // 2
    right_x = width // 2 + margin // 2
    right_width = width - right_x - margin
    unplaced_height = max(scaled(110), (body_bottom - margin) * 3 // 10)
    unplaced_bottom = body_bottom - margin
    unplaced = pygame.Rect(left_x, unplaced_bottom - unplaced_height, left_width, unplaced_height)
    unplaced_title = pygame.Rect(left_x, unplaced.y - title_height, left_width, title_height)
    atlas_title = pygame.Rect(left_x, margin, left_width, title_height)
    atlas = pygame.Rect(left_x, atlas_title.bottom, left_width, max(1, unplaced_title.y - margin - atlas_title.bottom))
    editor = pygame.Rect(right_x, margin, right_width, body_bottom - 2 * margin)
    return WindowLayout(atlas_title, atlas, unplaced_title, unplaced, editor, pygame.Rect(0, body_bottom, width, status_height))


def checkerboard(size: int) -> pygame.Surface:
    surface = pygame.Surface((size, size))
    square = max(4, scaled(8))
    for row in range(0, size, square):
        for column in range(0, size, square):
            light = ((row // square) + (column // square)) % 2 == 0
            surface.fill(Theme.CHECKER_LIGHT if light else Theme.CHECKER_DARK, (column, row, square, square))
    return surface


class TileSurfaceCache:
    """Pictures of tiles at the size they are drawn, made once and reused until the tile changes."""

    LIMIT = 4000

    def __init__(self) -> None:
        self._surfaces: dict[tuple, pygame.Surface] = {}
        self._checkerboards: dict[int, pygame.Surface] = {}

    def clear(self) -> None:
        self._surfaces.clear()

    def get(self, key: Hashable, pixels: numpy.ndarray, cell: int) -> pygame.Surface:
        full_key = (key, cell)
        surface = self._surfaces.get(full_key)
        if surface is None:
            if len(self._surfaces) > self.LIMIT:
                self._surfaces.clear()
            surface = self._render(pixels, cell)
            self._surfaces[full_key] = surface
        return surface

    def _render(self, pixels: numpy.ndarray, cell: int) -> pygame.Surface:
        height, width = pixels.shape[:2]
        tile = pygame.image.frombuffer(numpy.ascontiguousarray(pixels).tobytes(), (width, height), "RGBA")
        background = self._checkerboards.get(cell)
        if background is None:
            background = self._checkerboards[cell] = checkerboard(cell)
        surface = background.copy()
        surface.blit(pygame.transform.scale(tile, (cell, cell)), (0, 0))
        return surface


class Application:
    """The window: draws everything and turns events into calls on the project."""

    def __init__(self, project: AtlasProject, surface: pygame.Surface):
        self.project = project
        self.surface = surface
        self.fonts = Fonts()
        self.cache = TileSurfaceCache()
        self.atlas_panel = AtlasPanel(project)
        self.unplaced_panel = UnplacedPanel(project)
        self.interaction = AtlasInteraction(project, self.atlas_panel, self.unplaced_panel)
        self.editor = EditorPanel(project, self.fonts)
        self.tile_size_field = TextField(str(project.tile_size), lambda character: character.isdigit(), 4)
        self.tile_size_field_focused = False
        self.tile_size_field_invalid = False
        self.message = ""
        self.pointer = (-1, -1)
        self.needs_redraw = True
        self._last_caret_visible = True
        self.layout = compute_layout(surface.get_size())
        self.update_layout()

    # Layout -----------------------------------------------------------------------------------------------

    def update_layout(self) -> None:
        self.layout = compute_layout(self.surface.get_size())
        self.atlas_panel.rect = self.layout.atlas
        self.unplaced_panel.rect = self.layout.unplaced
        self.editor.set_rect(self.layout.editor)
        self.atlas_panel.clamp_scroll()
        self.unplaced_panel.clamp_scroll()

    def empty_state_rectangles(self) -> dict[str, pygame.Rect]:
        """Where the controls shown in an empty atlas are."""
        area = self.layout.atlas
        row_height = scaled(30)
        top = area.centery - scaled(60)
        rectangles = {"message": pygame.Rect(area.x, top, area.width, scaled(26))}
        next_top = top + scaled(46)
        if not self.project.tile_size_is_saved:
            label_width, field_width, button_width, spacing = scaled(78), scaled(64), scaled(150), scaled(8)
            left = area.centerx - (label_width + field_width + button_width + 2 * spacing) // 2
            rectangles["label"] = pygame.Rect(left, next_top, label_width, row_height)
            rectangles["field"] = pygame.Rect(left + label_width + spacing, next_top, field_width, row_height)
            rectangles["create"] = pygame.Rect(rectangles["field"].right + spacing, next_top, button_width, row_height)
            next_top += scaled(48)
        import_width = scaled(230)
        rectangles["import"] = pygame.Rect(area.centerx - import_width // 2, next_top, import_width, row_height)
        return rectangles

    # Events -----------------------------------------------------------------------------------------------

    def handle_event(self, event: pygame.event.Event) -> bool:
        """Deal with one event. Returns False when the program should close."""
        if event.type == pygame.QUIT:
            if self.project.has_unsaved_file_changes:
                # Until the dialog that asks what to do exists, closing must not throw changes away.
                self.message = "Unsaved changes: save them with Ctrl+Shift+S before closing."
                self.needs_redraw = True
                return True
            return False
        try:
            self._handle_event(event)
        except OSError as error:
            self.interaction.cancel()
            self.message = f"Could not save {ATLAS_FILE_NAME}: {error}"
        if self.editor.message:
            self.message, self.editor.message = self.editor.message, ""
        if event.type in (
            pygame.MOUSEBUTTONDOWN, pygame.MOUSEBUTTONUP, pygame.MOUSEMOTION, pygame.MOUSEWHEEL,
            pygame.KEYDOWN, pygame.TEXTINPUT, pygame.WINDOWSIZECHANGED, pygame.WINDOWEXPOSED, pygame.VIDEORESIZE,
        ):
            self.needs_redraw = True
        return True

    def _handle_event(self, event: pygame.event.Event) -> None:
        if event.type in (pygame.VIDEORESIZE, pygame.WINDOWSIZECHANGED):
            self.surface = pygame.display.get_surface() or self.surface
            self.update_layout()
        elif event.type == pygame.MOUSEMOTION:
            self.pointer = event.pos
            self.interaction.mouse_move(event.pos)
            self.editor.mouse_move(event.pos)
        elif event.type == pygame.MOUSEBUTTONDOWN and event.button in (1, 2, 3):
            self.pointer = event.pos
            if self.editor.contains(event.pos):
                self._leave_tile_size_field()
                self.editor.mouse_down(event.button, event.pos)
            elif event.button == 1:
                self.editor.lose_focus()
                self._handle_press(event.pos)
        elif event.type == pygame.MOUSEBUTTONUP and event.button in (1, 2, 3):
            self.pointer = event.pos
            self.editor.mouse_up(event.button, event.pos)
            if event.button == 1:
                self.interaction.mouse_up(event.pos)
        elif event.type == pygame.MOUSEWHEEL:
            if self.editor.contains(self.pointer):
                self.editor.wheel(event.y, self.pointer)
            else:
                self._handle_wheel(event)
        elif event.type == pygame.TEXTINPUT and self.tile_size_field_focused:
            self.tile_size_field.insert_text(event.text)
            self.tile_size_field_invalid = False
        elif event.type == pygame.TEXTINPUT and self.editor.hex_focused:
            self.editor.hex_field.insert_text(event.text)
            self.editor.hex_invalid = False
        elif event.type == pygame.KEYDOWN:
            self._handle_key(event)

    def _handle_press(self, position: tuple[int, int]) -> None:
        self.message = ""
        if self.project.is_empty:
            rectangles = self.empty_state_rectangles()
            if "field" in rectangles and rectangles["field"].collidepoint(position):
                self.tile_size_field_focused = True
                return
            if "create" in rectangles and rectangles["create"].collidepoint(position):
                self.create_atlas_file()
                return
        self._leave_tile_size_field()
        # Placing a pasted picture must happen while its tiles are still the ones shown.
        self.editor.finish_selection()
        self.interaction.mouse_down(position, bool(pygame.key.get_mods() & pygame.KMOD_CTRL))

    def _leave_tile_size_field(self) -> None:
        if self.tile_size_field_focused:
            self.apply_tile_size_field()
            self.tile_size_field_focused = False

    def _handle_wheel(self, event: pygame.event.Event) -> None:
        modifiers = pygame.key.get_mods()
        for panel in (self.atlas_panel, self.unplaced_panel):
            if panel.rect.collidepoint(self.pointer):
                if modifiers & pygame.KMOD_CTRL:
                    panel.zoom_by(event.y, self.pointer)
                    self.cache.clear()
                elif modifiers & pygame.KMOD_SHIFT:
                    panel.scroll_by(-event.y * scaled(60), 0)
                else:
                    panel.scroll_by(-event.x * scaled(60), -event.y * scaled(60))
                return

    def _handle_key(self, event: pygame.event.Event) -> None:
        if self.tile_size_field_focused:
            if event.key in (pygame.K_RETURN, pygame.K_KP_ENTER):
                self.apply_tile_size_field()
                self.tile_size_field_focused = False
            elif event.key == pygame.K_ESCAPE:
                self.tile_size_field.set_text(str(self.project.tile_size))
                self.tile_size_field_focused = False
            else:
                self.tile_size_field.handle_key_down(event.key, event.mod)
                self.tile_size_field_invalid = False
        elif self.editor.hex_focused:
            self.editor.hex_key_down(event)
        elif self.editor.key_down(event.key, event.mod, getattr(event, "scancode", 0)):
            pass
        elif event.key == pygame.K_ESCAPE:
            self.interaction.cancel()
            self.editor.session = None

    # Actions ----------------------------------------------------------------------------------------------

    def apply_tile_size_field(self) -> None:
        text = self.tile_size_field.text
        if text.isdigit() and MINIMUM_TILE_SIZE <= int(text) <= MAXIMUM_TILE_SIZE:
            if int(text) != self.project.tile_size and self.project.set_tile_size(int(text)):
                for panel in (self.atlas_panel, self.unplaced_panel):
                    panel.set_tile_size(self.project.tile_size)
                self.cache.clear()
            self.tile_size_field_invalid = False
        else:
            self.tile_size_field_invalid = True
            self.message = f"The tile size must be a whole number from {MINIMUM_TILE_SIZE} to {MAXIMUM_TILE_SIZE}."

    def create_atlas_file(self) -> None:
        self.apply_tile_size_field()
        if self.tile_size_field_invalid:
            return
        self.tile_size_field_focused = False
        self.project.save_atlas_file()
        self.message = f"Created {ATLAS_FILE_NAME} with tile size {self.project.tile_size}."

    # Drawing ----------------------------------------------------------------------------------------------

    def caret_visible(self) -> bool:
        return (pygame.time.get_ticks() // 500) % 2 == 0

    def wants_redraw(self) -> bool:
        if (self.tile_size_field_focused or self.editor.hex_focused) and self.caret_visible() != self._last_caret_visible:
            return True
        return self.needs_redraw

    def tile_picture(self, key: Hashable, pixels: numpy.ndarray, cell: int) -> pygame.Surface:
        return self.cache.get(key, pixels, cell)

    def draw(self) -> None:
        surface = self.surface
        self._last_caret_visible = self.caret_visible()
        surface.fill(Theme.BACKGROUND)
        layout = self.layout
        count = len(self.project.tiles)
        draw_text(surface, self.fonts.normal, f"Atlas ({count} tile{'' if count == 1 else 's'})", Theme.TEXT, layout.atlas_title.midleft, "midleft")
        draw_text(
            surface, self.fonts.normal, f"Not in the atlas ({len(self.project.unplaced)})", Theme.TEXT,
            layout.unplaced_title.midleft, "midleft",
        )
        self.draw_atlas()
        self.draw_unplaced()
        self.editor.draw(self.surface, self.caret_visible())
        self.draw_drag_feedback()
        self.draw_status()
        self.draw_tooltip()
        self.needs_redraw = False

    def draw_panel_frame(self, rectangle: pygame.Rect) -> None:
        pygame.draw.rect(self.surface, Theme.PANEL, rectangle)
        pygame.draw.rect(self.surface, Theme.PANEL_BORDER, rectangle, width=1)

    def draw_tile(self, rectangle: pygame.Rect, record: Optional[TileRecord], pixels: Optional[numpy.ndarray], key: Hashable) -> None:
        surface = self.surface
        if record is not None and record.error is not None:
            pygame.draw.rect(surface, Theme.ERROR_BACKGROUND, rectangle)
            pygame.draw.rect(surface, Theme.ERROR, rectangle, width=2)
            size = rectangle.height
            triangle = [
                (rectangle.centerx, rectangle.y + size // 5),
                (rectangle.x + size // 5, rectangle.bottom - size // 5),
                (rectangle.right - size // 5, rectangle.bottom - size // 5),
            ]
            pygame.draw.polygon(surface, Theme.ERROR, triangle, width=max(2, size // 24))
            draw_text(surface, self.fonts.large, "!", Theme.ERROR, (rectangle.centerx, rectangle.centery + size // 8), "center")
        else:
            surface.blit(self.tile_picture(key, pixels, rectangle.width), rectangle)
        if record is not None and record.is_modified:
            radius = max(3, rectangle.width // 12)
            center = (rectangle.right - radius - 2, rectangle.y + radius + 2)
            pygame.draw.circle(surface, Theme.BACKGROUND, center, radius + 1)
            pygame.draw.circle(surface, Theme.UNSAVED, center, radius)

    def draw_atlas(self) -> None:
        panel = self.atlas_panel
        self.draw_panel_frame(panel.rect)
        self.surface.set_clip(panel.rect)
        for coordinate in panel.visible_coordinates():
            record = self.project.tiles.get(coordinate)
            if record is None:
                continue
            rectangle = panel.tile_screen_rect(coordinate)
            self.draw_tile(rectangle, record, record.pixels, (id(record), record.revision))
            if coordinate in self.project.selection:
                pygame.draw.rect(self.surface, Theme.ACCENT, rectangle.inflate(6, 6), width=2)
        if self.project.is_empty:
            self.draw_empty_state()
        self.surface.set_clip(None)

    def draw_empty_state(self) -> None:
        rectangles = self.empty_state_rectangles()
        draw_text(self.surface, self.fonts.large, "The atlas is empty.", Theme.TEXT, rectangles["message"].center, "center")
        if "field" in rectangles:
            draw_text(self.surface, self.fonts.normal, "Tile size", Theme.TEXT_DIMMED, rectangles["label"].midleft, "midleft")
            self.tile_size_field.draw(
                self.surface, rectangles["field"], self.fonts.normal, self.tile_size_field_focused,
                self.caret_visible(), self.tile_size_field_invalid,
            )
            draw_button(
                self.surface, rectangles["create"], f"Create {ATLAS_FILE_NAME}", self.fonts.normal,
                rectangles["create"].collidepoint(self.pointer),
            )
        draw_button(self.surface, rectangles["import"], "Import image as atlas", self.fonts.normal, False, enabled=False)

    def draw_unplaced(self) -> None:
        panel = self.unplaced_panel
        self.draw_panel_frame(panel.rect)
        self.surface.set_clip(panel.rect)
        if not self.project.unplaced:
            draw_text(
                self.surface, self.fonts.small, "Tile images in this folder that are not in the atlas appear here.",
                Theme.TEXT_DIMMED, (panel.rect.centerx, panel.rect.centery), "center",
            )
        for index, tile in enumerate(self.project.unplaced):
            coordinate = panel.coordinate_of_index(index)
            rectangle = panel.tile_screen_rect(coordinate)
            if rectangle.colliderect(panel.rect):
                self.draw_tile(rectangle, None, tile.pixels, (id(tile), 0))
        self.surface.set_clip(None)

    def draw_drag_feedback(self) -> None:
        drag = self.interaction.drag
        if drag is None or not drag.started:
            return
        surface = self.surface
        color = Theme.VALID if drag.drop_is_valid else Theme.INVALID
        if drag.source == "atlas":
            offset = (drag.pointer[0] - drag.start_position[0], drag.pointer[1] - drag.start_position[1])
            if drag.drops_on_unplaced:
                pygame.draw.rect(surface, color, self.unplaced_panel.rect, width=3)
            elif drag.drop_coordinate is not None:
                shift = (drag.drop_coordinate[0] - drag.grabbed_coordinate[0], drag.drop_coordinate[1] - drag.grabbed_coordinate[1])
                self.atlas_panel_clip()
                for column, row in self.project.selection:
                    destination = (column + shift[0], row + shift[1])
                    if destination[0] >= 0 and destination[1] >= 0:
                        pygame.draw.rect(surface, color, self.atlas_panel.tile_screen_rect(destination).inflate(6, 6), width=3)
                surface.set_clip(None)
            for coordinate in self.project.selection:
                record = self.project.tiles[coordinate]
                if record.error is None:
                    picture = self.tile_picture((id(record), record.revision), record.pixels, self.atlas_panel.geometry.cell).copy()
                    picture.set_alpha(150)
                    surface.blit(picture, self.atlas_panel.tile_screen_rect(coordinate).move(offset))
        else:
            tile = self.project.unplaced[drag.unplaced_index]
            cell = self.unplaced_panel.geometry.cell
            picture = self.tile_picture((id(tile), 0), tile.pixels, cell).copy()
            picture.set_alpha(150)
            surface.blit(picture, picture.get_rect(center=drag.pointer))
            if drag.drop_coordinate is not None:
                self.atlas_panel_clip()
                pygame.draw.rect(surface, color, self.atlas_panel.tile_screen_rect(drag.drop_coordinate).inflate(6, 6), width=3)
                surface.set_clip(None)

    def atlas_panel_clip(self) -> None:
        self.surface.set_clip(self.atlas_panel.rect)

    def draw_status(self) -> None:
        rectangle = self.layout.status
        if self.message:
            draw_text(self.surface, self.fonts.normal, self.message, Theme.ERROR if "Could not" in self.message else Theme.TEXT_DIMMED, (rectangle.x + scaled(10), rectangle.centery), "midleft")

    def hover_text(self) -> Optional[str]:
        if self.interaction.dragging:
            return None
        if self.atlas_panel.rect.collidepoint(self.pointer):
            coordinate = self.atlas_panel.tile_at(self.pointer)
            if coordinate is not None:
                record = self.project.tiles[coordinate]
                return record.name if record.error is None else f"{record.name}\n{record.error}"
            if self.project.is_empty and self.empty_state_rectangles()["import"].collidepoint(self.pointer):
                return "Importing a picture is not built yet."
        elif self.unplaced_panel.rect.collidepoint(self.pointer):
            index = self.unplaced_panel.tile_at(self.pointer)
            if index is not None:
                return self.project.unplaced[index].name
        return None

    def draw_tooltip(self) -> None:
        text = self.hover_text()
        if not text:
            return
        font = self.fonts.small
        lines = wrap_text(font, text, scaled(360))
        width = max(font.size(line)[0] for line in lines) + scaled(12)
        height = len(lines) * font.get_linesize() + scaled(8)
        size = self.surface.get_size()
        x = min(self.pointer[0] + scaled(14), size[0] - width - 2)
        y = min(self.pointer[1] + scaled(18), size[1] - height - 2)
        rectangle = pygame.Rect(x, y, width, height)
        pygame.draw.rect(self.surface, Theme.TOOLTIP_BACKGROUND, rectangle, border_radius=3)
        pygame.draw.rect(self.surface, Theme.PANEL_BORDER, rectangle, width=1, border_radius=3)
        for number, line in enumerate(lines):
            draw_text(self.surface, font, line, Theme.TEXT, (x + scaled(6), y + scaled(4) + number * font.get_linesize()))


# ----------------------------------------------------------------------------------------------------------
# Entry point
# ----------------------------------------------------------------------------------------------------------


def initialise_pygame() -> pygame.Surface:
    """Start pygame and open the window, using the screen's real pixels (not the blurry scaled ones) on Windows."""
    global USER_INTERFACE_SCALE
    if sys.platform == "win32":
        try:
            import ctypes

            ctypes.windll.user32.SetProcessDPIAware()
            USER_INTERFACE_SCALE = max(1.0, min(3.0, ctypes.windll.user32.GetDpiForSystem() / 96.0))
        except (AttributeError, OSError):
            pass
    pygame.init()
    pygame.key.start_text_input()
    pygame.display.set_caption("VirtualAtlas")
    return pygame.display.set_mode((scaled(1280), scaled(800)), pygame.RESIZABLE)


def run_application(project: AtlasProject, surface: Optional[pygame.Surface] = None) -> None:
    surface = surface if surface is not None else initialise_pygame()
    pygame.display.set_caption(f"VirtualAtlas - {project.folder.name}")
    application = Application(project, surface)
    clock = pygame.time.Clock()
    running = True
    while running:
        for event in pygame.event.get():
            if not application.handle_event(event):
                running = False
        if application.wants_redraw():
            application.draw()
            pygame.display.flip()
        clock.tick(60)
    pygame.quit()


def show_refusal_window(problems: list[str]) -> None:
    """Tell the user why atlas.json cannot be used, in a window, until it is closed."""
    surface = initialise_pygame()
    pygame.display.set_caption("VirtualAtlas - cannot start")
    fonts = Fonts()
    clock = pygame.time.Clock()
    running = True
    while running:
        for event in pygame.event.get():
            if event.type == pygame.QUIT or (event.type == pygame.KEYDOWN and event.key == pygame.K_ESCAPE):
                running = False
        surface.fill(Theme.BACKGROUND)
        width = surface.get_width() - 2 * scaled(24)
        draw_text(surface, fonts.large, f"{ATLAS_FILE_NAME} cannot be used, so VirtualAtlas did not start.", Theme.ERROR, (scaled(24), scaled(20)))
        y = scaled(64)
        for problem in problems:
            for number, line in enumerate(wrap_text(fonts.normal, problem, width - scaled(16))):
                draw_text(surface, fonts.normal, ("- " if number == 0 else "  ") + line, Theme.TEXT, (scaled(24), y))
                y += scaled(22)
        draw_text(surface, fonts.small, "Fix or remove the entries above in atlas.json, then start the program again. Close this window to quit.", Theme.TEXT_DIMMED, (scaled(24), y + scaled(12)))
        pygame.display.flip()
        clock.tick(30)
    pygame.quit()


def main() -> int:
    logging.basicConfig(level=logging.INFO, format="%(levelname)s: %(message)s")
    folder = Path(__file__).resolve().parent
    try:
        project = AtlasProject.open(folder)
    except AtlasFileError as error:
        for problem in error.problems:
            print(f"{ATLAS_FILE_NAME}: {problem}", file=sys.stderr)
        show_refusal_window(error.problems)
        return 1
    run_application(project)
    return 0


if __name__ == "__main__":
    sys.exit(main())
