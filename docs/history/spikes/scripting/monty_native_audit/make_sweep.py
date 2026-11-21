"""Generates sweep.tsv: every common method of str, bytes, list, tuple, dict, set, deque on a 10**6-element receiver."""
NL = chr(92) + "n"
TAB = chr(9)
REPS = 5

SETUPS = {
    "str": "s = 'ab' * 500000\nt = 'ab' * 500000",
    "bytes": "s = b'ab' * 500000\nt = b'ab' * 500000",
    "list": "s = list(range(1000000))\nt = list(range(1000000))",
    "tuple": "s = tuple(range(1000000))\nt = tuple(range(1000000))",
    "dict": "s = {i: i for i in range(1000000)}\nt = {i: i for i in range(1000000)}",
    "set": "s = set(range(1000000))\nt = set(range(1000000))",
    "deque": "import collections\ns = collections.deque(range(1000000))\nt = collections.deque(range(1000000))",
}

STR_LIKE = [
    "capitalize()", "center(2000000)", "count(X1)", "endswith(X1)", "find(X1)", "index(X2)", "isalnum()", "isalpha()",
    "isascii()", "isdigit()", "islower()", "isspace()", "istitle()", "isupper()", "ljust(2000000)", "lower()",
    "lstrip()", "partition(X1)", "removeprefix(X1)", "removesuffix(X1)", "replace(X2, X1)", "rfind(X1)", "rindex(X2)",
    "rjust(2000000)", "rpartition(X1)", "rsplit(X1)", "rstrip()", "split(X1)", "splitlines()", "startswith(X2)",
    "strip()", "swapcase()", "title()", "upper()", "zfill(2000000)", "expandtabs()", "join(JARG)", "decode()",
    "encode()", "hex()", "casefold()", "isidentifier()", "isprintable()", "isnumeric()", "isdecimal()",
]
OPERATORS = [
    "s == t", "s != t", "s < t", "s + t", "s * 2", "s[::-1]", "s[1:]", "s[:-1]", "len(s)", "hash(s)", "X2 in s",
    "X1 in s", "repr(s)", "str(s)", "list(s)", "s[500000]", "bool(s)", "sorted(s)", "max(s)", "reversed(s)",
    "iter(s)", "enumerate(s)", "s % ()",
]
LIST_METHODS = [
    "append(1)", "extend([1])", "insert(0, 1)", "insert(len(s), 1)", "pop()", "pop(0)", "remove(999999)", "index(999999)",
    "count(5)", "reverse()", "sort()", "copy()", "clear()", "extend(t)", "__len__()", "__contains__(5)",
]
SEQ_OPERATORS = [
    "s == t", "s != t", "s < t", "s + t", "s * 2", "s[::-1]", "s[1:]", "s[:-1]", "len(s)", "hash(s)", "5 in s",
    "-1 in s", "repr(s)", "list(s)", "tuple(s)", "set(s)", "sorted(s)", "max(s)", "min(s)", "sum(s)", "any(s)", "all(s)",
    "reversed(s)", "s[500000]", "bool(s)", "s[5:10]", "s[::2]", "s[0:len(s)]",
]
DICT_METHODS = [
    "get(5)", "setdefault(5, 1)", "pop(5, None)", "popitem()", "keys()", "values()", "items()", "copy()", "update(t)",
    "update({})", "clear()", "fromkeys(range(10))",
]
DICT_OPERATORS = [
    "s == t", "s | t", "len(s)", "5 in s", "list(s)", "list(s.items())", "list(s.values())", "sorted(s)", "dict(s)",
    "repr(s)", "s[5]", "max(s)", "sum(s.values())", "bool(s)", "{**s}", "set(s)",
]
SET_METHODS = [
    "add(5)", "remove(5)", "discard(5)", "pop()", "copy()", "clear()", "union(t)", "intersection(t)", "difference(t)",
    "symmetric_difference(t)", "issubset(t)", "issuperset(t)", "isdisjoint(t)", "update(t)", "difference_update(t)",
    "intersection_update(t)", "union({1})", "intersection({1})", "difference({1})", "isdisjoint({1})",
]
SET_OPERATORS = [
    "s == t", "s | t", "s & t", "s - t", "s ^ t", "s <= t", "len(s)", "5 in s", "list(s)", "sorted(s)", "set(s)",
    "frozenset(s)", "repr(s)", "max(s)", "sum(s)", "bool(s)",
]
DEQUE_METHODS = [
    "append(1)", "appendleft(1)", "pop()", "popleft()", "rotate(1)", "rotate(500000)", "copy()", "count(5)", "index(999999)",
    "extend([1])", "extend(t)", "reverse()", "clear()", "remove(999999)",
]


def sub(expr, kind):
    if kind == "str":
        x1, x2, jarg = "'x'", "'ba'", "['a', 'b']"
    elif kind == "bytes":
        x1, x2, jarg = "b'x'", "b'ba'", "[b'a', b'b']"
    else:
        return expr
    return expr.replace("X1", x1).replace("X2", x2).replace("JARG", jarg)


rows = []


def add(kind, name, op):
    # Mutators change the receiver; each case rebuilds it. The loop runs the operation REPS times.
    body = "for _ in range(%d):%s    %s" % (REPS, NL, op)
    rows.append((kind + "." + name, SETUPS[kind].replace("\n", NL), body))


for kind in ("str", "bytes"):
    for m in STR_LIKE:
        add(kind, m, "x = s." + sub(m, kind))
    for m in OPERATORS:
        add(kind, "op " + m, "x = " + sub(m, kind))
for kind in ("list", "deque"):
    for m in (LIST_METHODS if kind == "list" else DEQUE_METHODS):
        add(kind, m, "x = s." + m)
for m in SEQ_OPERATORS:
    add("list", "op " + m, "x = " + m)
    add("tuple", "op " + m, "x = " + m)
for m in DICT_METHODS:
    add("dict", m, "x = s." + m)
for m in DICT_OPERATORS:
    add("dict", "op " + m, "x = " + m)
for m in SET_METHODS:
    add("set", m, "x = s." + m)
for m in SET_OPERATORS:
    add("set", "op " + m, "x = " + m)

with open("E:/s/vx/monty-native-audit/sweep.tsv", "w", encoding="utf-8", newline="\n") as out:
    for name, setup, op in rows:
        out.write(TAB.join([name, setup, op]) + "\n")
print(len(rows), "cases")
