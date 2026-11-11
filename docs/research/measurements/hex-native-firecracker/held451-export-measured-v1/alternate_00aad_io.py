"""Bounded file primitives for the source-only 00aad fixture successor.

Not executed in this checkpoint. No process, device or network API lives here.
Every output is exclusive; failed partial bytes remain under the owned root.
"""
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import time

MAX_JSON = 16777216
MAX_FILE = 536870912
MAX_ENTRIES = 262144
CHUNK = 65536


def require(ok, cause):
    if not ok:
        raise ValueError(cause)


def tick(deadline):
    require(time.monotonic() < deadline, 'whole_scope_budget_exhausted')


def relative(name):
    require(isinstance(name, str) and 0 < len(name) <= 1024 and
            re.fullmatch(r'[A-Za-z0-9._/+:~\-]+', name), 'relative_path_grammar')
    value = PurePosixPath(name)
    require(not value.is_absolute() and str(value) == name and
            all(part not in ['', '.', '..'] for part in value.parts) and len(value.parts) <= 64,
            'canonical_relative_path')
    return value


def read_json(path):
    with Path(path).open('rb') as stream:
        data = stream.read(MAX_JSON + 1)
    require(len(data) <= MAX_JSON, 'json_byte_budget')
    return json.loads(data)


def write_json(path, value):
    data = (json.dumps(value, sort_keys=True, indent=2, allow_nan=False) + '\n').encode()
    require(len(data) <= MAX_JSON, 'output_json_byte_budget')
    with Path(path).open('xb') as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())


def regular_hash(path, limit=MAX_FILE):
    digest = hashlib.sha256()
    size = 0
    with Path(path).open('rb') as stream:
        state = os.fstat(stream.fileno())
        require(stat.S_ISREG(state.st_mode) and 0 <= state.st_size <= limit, 'bounded_regular_file')
        while chunk := stream.read(CHUNK):
            size += len(chunk)
            require(size <= limit, 'input_grew_past_bound')
            digest.update(chunk)
    return {'source': str(path), 'bytes': size, 'sha256': digest.hexdigest()}


def verify(path, row):
    require(isinstance(row['bytes'], int) and not isinstance(row['bytes'], bool) and
            0 <= row['bytes'] <= MAX_FILE and re.fullmatch(r'[0-9a-f]{64}', row['sha256']), 'input_pin')
    actual = regular_hash(path, row['bytes'] + 1)
    require(actual['bytes'] == row['bytes'] and actual['sha256'] == row['sha256'],
            'input_pin_mismatch:' + str(path))
    return Path(path)


def private_child(root, name):
    parts = relative(name).parts
    parent = Path(root)
    for part in parts[:-1]:
        parent = parent / part
        try:
            parent.mkdir(mode=0o755)
        except FileExistsError:
            pass
        state = parent.lstat()
        require(stat.S_ISDIR(state.st_mode) and state.st_uid == os.getuid(), 'owned_directory_no_links')
    return parent / parts[-1]


def private_input(path):
    """Reject links anywhere below the reviewed private cache import root."""
    path = Path(path)
    cache = Path('/home/adalundhe/.cache/hex-native-firecracker')
    require(path.is_absolute() and cache in path.parents, 'private_import_root')
    for item in [path, *path.parents]:
        if item == cache:
            break
        state = item.lstat()
        require(not stat.S_ISLNK(state.st_mode) and state.st_uid == os.getuid(), 'private_import_owner_or_link')
    return path


def publish_stream(stream, destination, row, deadline, mode=0o444):
    """Publish verified bytes without replacing any existing final/partial name."""
    destination = Path(destination)
    partial = destination.with_name(destination.name + '.partial')
    fd = os.open(partial, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW, 0o600)
    digest = hashlib.sha256()
    size = 0
    with os.fdopen(fd, 'wb') as output:
        while size < row['bytes']:
            tick(deadline)
            chunk = stream.read(min(CHUNK, row['bytes'] - size))
            require(bool(chunk), 'short_object_source')
            output.write(chunk)
            size += len(chunk)
            digest.update(chunk)
        require(not stream.read(1), 'long_object_source')
        require(digest.hexdigest() == row['sha256'], 'object_hash_mismatch')
        output.flush()
        os.fchmod(output.fileno(), mode)
        os.fsync(output.fileno())
    # link() is exclusive. rename() would silently replace a pre-existing file.
    os.link(partial, destination, follow_symlinks=False)
    os.unlink(partial)  # Only this recorded successful private partial link.
    directory_fd = os.open(destination.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)
    return regular_hash(destination, row['bytes'] + 1)


def copy_pin(source, destination, row, deadline, mode=0o444):
    verify(source, row)
    with Path(source).open('rb') as stream:
        require(stat.S_ISREG(os.fstat(stream.fileno()).st_mode), 'regular_copy_source')
        return publish_stream(stream, destination, row, deadline, mode)


def usage(root):
    count = logical = allocated = 0
    inodes = set()
    for here, directories, files in os.walk(root, followlinks=False):
        for name in files + directories:
            state = (Path(here) / name).lstat()
            count += 1
            require(count <= MAX_ENTRIES, 'owned_tree_entry_budget')
            logical += state.st_size
            identity = (state.st_dev, state.st_ino)
            if identity not in inodes:
                inodes.add(identity)
                allocated += state.st_blocks * 512
    space = os.statvfs(root)
    return {'entries': count, 'logical_bytes': logical, 'allocated_file_bytes': allocated,
            'unique_inodes': len(inodes),
            'available_bytes': space.f_bavail * space.f_frsize,
            'filesystem_metadata': 'not a hard quota or measured RuntimeCost'}
