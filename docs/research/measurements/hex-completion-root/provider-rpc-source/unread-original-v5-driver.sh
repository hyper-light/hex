set -eu
check_scope="$HOME/.cache/hex-complete-bindings/red696-pure-v3"
cat > "$check_scope/overlay-v5.tar.gz"
shasum -a 256 "$check_scope/overlay-v5.tar.gz"
tar -xzf "$check_scope/overlay-v5.tar.gz" -C "$check_scope/source"
check_toolchain="$HOME/.rustup/toolchains/1.99.1-aarch64-apple-darwin"
export RUSTUP_AUTO_INSTALL=0 RUSTUP_TOOLCHAIN=1.99.1
export RUSTC="$check_toolchain/bin/rustc" RUSTDOC="$check_toolchain/bin/rustdoc"
export CARGO_HOME="$check_scope/cargo" CARGO_TARGET_DIR="$check_scope/target"
export CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_PROFILE_DEV_STRIP=none CARGO_PROFILE_TEST_STRIP=none
export TMPDIR="$check_scope/temp"
check_fixture="$check_scope/source/docs/research/measurements/hex-complete-bindings/unread-control"
cd "$check_scope/source"
python3 "$check_fixture/prepare.py" --source-sha256 d1049cc9661bb5544758603cf94c0ddfb99ba9467b78bbe8839a04c03d22a6f0 > "$check_scope/control-v5.log"
cat "$check_scope/control-v5.log"
nice -n 10 "$check_toolchain/bin/cargo" metadata --offline --no-deps --format-version 1 --manifest-path "$check_fixture/Cargo.toml" > "$check_scope/metadata-v5.json" 2> "$check_scope/metadata-v5.log"
python3 - "$check_scope/source/Cargo.lock" "$check_fixture/Cargo.lock" <<'PYLOCK'
import re,sys,pathlib
def registry(path):
 out=set()
 for block in pathlib.Path(path).read_text().split('[[package]]')[1:]:
  fields=dict(re.findall(r'^(name|version|source) = "([^"]+)"',block,re.M))
  if fields.get('source','').startswith('registry+'):
   out.add((fields['name'],fields['version'],fields['source']))
 return out
new=registry(sys.argv[2])-registry(sys.argv[1])
if new: raise SystemExit('unexpected dependency version(s): '+repr(sorted(new)))
print('registry_version_diff=[]')
PYLOCK
shasum -a 256 "$check_fixture/Cargo.lock"
"$check_toolchain/bin/rustc" --version
set +e
nice -n 10 "$check_toolchain/bin/cargo" test --locked --offline --manifest-path "$check_fixture/Cargo.toml" --lib device_offer_tests::unread_native_counters_never_become_a_fresh_idle_free_offer -- --exact --nocapture --test-threads=1 > "$check_scope/red-v5.log" 2>&1 &
check_pid=$!
printf 'owned_cargo_pid=%s\n' "$check_pid"
wait "$check_pid"
check_exit=$?
set -e
printf '%s\n' "$check_exit" > "$check_scope/red-v5.exit"
printf 'cargo_exit=%s reaped=true\n' "$check_exit"
shasum -a 256 "$check_scope/red-v5.log"
find "$check_scope/target/debug/deps" -maxdepth 1 -name 'hex_unread_control-*' -type f -perm -111 -exec shasum -a 256 {} \;
tail -90 "$check_scope/red-v5.log"
find "$check_scope/temp" -mindepth 1 -maxdepth 1 -print
exit "$check_exit"
