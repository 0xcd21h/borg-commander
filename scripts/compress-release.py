import gzip
import sys
from pathlib import Path

LEGACY_UPDATE_LIMIT = 10 * 1024 * 1024
ZOPFLI_ITERATIONS = 3

source, destination = map(Path, sys.argv[1:])
contents = source.read_bytes()
compressed = gzip.compress(contents, compresslevel=9, mtime=0)
if len(compressed) > LEGACY_UPDATE_LIMIT:
    try:
        import zopfli.gzip
    except ModuleNotFoundError:
        sys.exit("Install zopfli in the packaging Python environment to compress this release for older updaters")
    compressed = zopfli.gzip.compress(contents, numiterations=ZOPFLI_ITERATIONS)
if len(compressed) > LEGACY_UPDATE_LIMIT:
    sys.exit(f"Release archive is {len(compressed)} bytes; older updaters accept at most {LEGACY_UPDATE_LIMIT}")
destination.write_bytes(compressed)
