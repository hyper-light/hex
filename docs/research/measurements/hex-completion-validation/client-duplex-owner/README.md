# Fallible unique duplex facade (AC-T7.4.1)

Fixed parent `dc057f2f7f38b971e072ce9ac8ccc9c6bd677f16` includes the separately
signed 5c10 constructor and its three source controls; no original Drop remedy
is imported. ROOT ports this exact delta into its current joined cohort.

The existing Link/Duplex return and retention seam uses one public alias:
`DuplexOwner = triomphe::UniqueArc<dyn Duplex>`. Public
`own_duplex<D: Duplex>(D) -> Result<DuplexOwner, Refusal>` uses the already locked
triomphe 0.1.17 `UniqueArc::try_new` then its existing safe unsize support with
unsize 1.1.0. The sole constructor error is typed allocation exhaustion. No
version, feature or Cargo change is needed beyond ROOT's existing locked edges.
The primary source and any available locked archive comparison are in source.json.

Native stream cloning remains explicit `Duplex::try_clone`, now returning the
same unique facade type. Facade-allocation failure maps to the existing I/O
contract's allocation-free `ErrorKind::OutOfMemory`; the temporary cloned handle
is dropped on failure. No new Box::new, unsafe, custom refcount, shared native
owner, worker or manager is introduced. Shards still gives its existing boxed
SDK Stream; this removes Hex's additional infallible facade Box, and does not
claim to remediate allocations inside that upstream Stream contract or std.

Control read/write/end handles and named stdio retain the same streams. The
UniqueArc type has no std Read/Write impl, and Rust's orphan rule precludes
adding one to that foreign alias. A private one-field DuplexIo value adapter
moves the SAME owner into the existing generic copier; it allocates nothing,
clones no handle and changes no copier loop/thread or deadline. Staging only
changes its owner type and borrows through DerefMut instead of Box::as_mut.
Existing protocol, framing, event, deadline and timeout algorithms are unchanged.

Owned client-test ports are mechanical return/constructor changes only, including
the three 5c10 controls. The complete original 6753 Drop test function is
byte-identical. ROOT owns external/benchmark ports; L1 owns FC Endpoint and
StagingStream. source.json inventories unported outside paths at this parent.
The guest/shards.rs delta is facade-only: ROOT must preserve its separately
owned SDK connect_before/dial_before integration rather than overwrite with
this older whole blob. No new SDK connect override is claimed by this source.

Policy: implementer, fully read Shards7d7/rulesdf2 unchanged. Parent Hex CLAUDE
fully read. This is SOURCE ONLY: builds/tests/native by L4 are 0. Focused owner
I/O/drop control source follows separately; ROOT owns joined compilation and
any allocator-fault/native/lifetime acceptance. No native readiness claim.
