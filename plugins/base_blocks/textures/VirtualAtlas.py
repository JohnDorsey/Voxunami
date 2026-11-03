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
algorithms, edit history, then the user interface. Everything before the user interface is plain functions
with no window, so it can be tested without one (tools/virtual_atlas/test_virtual_atlas.py).
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
import re
import sys
import tempfile
from collections import deque
from fractions import Fraction
from pathlib import Path
from typing import Callable, Hashable, Iterable, Optional

try:
    import numpy
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
            tiles[coordinate] = TileRecord(name, None, None, f"the stored pixel data {error}")
            continue
        if actual_name is None:
            tiles[coordinate] = TileRecord(name, data_pixels, None)
        elif disk_pixels is None:
            tiles[coordinate] = TileRecord(
                name, None, None, f"stored pixel data exists, but the file on disk {disk_problem}"
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
# Entry point
# ----------------------------------------------------------------------------------------------------------


def main() -> int:
    print("VirtualAtlas: the user interface is not built yet (phase 1 holds the logic only).", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
