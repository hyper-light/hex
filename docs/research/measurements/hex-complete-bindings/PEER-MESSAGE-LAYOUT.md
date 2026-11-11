# Exact peer receipt stays in the bounded message value

Source-design decision for ROOT's actual f7a strict lint, 2026-11-11. L1 implementer;
policy7d7e0855/rulesdf2 unchanged. ROOT explicitly approved exactly one
`clippy::large_enum_variant` exception on `peer::Msg` for this representation.
It does not authorize a panic, unchecked allocation or bounds-lint exception.

The source is a nonrecursive Copy protocol value. Start carries the original Demand,
fixed-size Allocation (including complete RuntimeCost evidence/profile), and exact
RouteReceipt. Remote admission and immutable record/recovery comparison use those
original values; present capacity cannot reconstruct them. Other variants do not
need those fields. The actual Linux x86_64 Rust1.99.1 diagnostic reports the largest
variant at least350 bytes, second at least112, and complete enum at least352 bytes.
Those are compiler observations for that target, not asserted cross-target constants.

Actual evidence: source `f7a08e5748e999ca6dba855143e8c7e581cfa44f`, strict logSHA256
`ebdfe32afa6fb41f6b5d911ea3cd21d1ee0f4e59edc36c49ea267b4e895802fa`, independently
collected rawSHA256 `30469cc35cc04fd1d00ea2b86ebc1c74b04dc3844dc5385f410ecc312377a147`.
It is a style diagnostic, not a demonstrated native or throughput defect.

The chosen exception is attached only to Msg and names this record in its reason.
It preserves current copied exact receipt semantics without introducing a heap owner,
refcount, additional inventory, authority or DTO just to cross a generic style
threshold. Msg and Ran remain Copy; Start's wire/text representation is unchanged.
Peer Owned/Ran count limits, bounded decode/text and shell admission remain the
existing mechanisms. This decision does not prove every Vec/map/queue allocation
seam safe; their fallibility, memory charge and failure atomicity stay audit obligations.

A credible alternative is a fallibly allocated immutable Start receipt shared by its
existing queue/record/retry owners. That requires replacing Copy message transfers,
propagating allocation failure before publication, and reviewing all disconnect,
retransmission, replay/recovery and cancellation ownership. It cannot be implemented
by an unchecked Box::new/Arc::new or a detached receipt cache. No measurement currently
justifies that ownership change solely for the lint; it is outside this mechanism's
required behavior. Padding smaller variants or dropping exact profile fields is not
a valid remedy. ROOT owns any future allocation/fault/profile-driven representation
change and equivalent workloads/security comparison.

There is no size, speed, allocation-free, leak-free or performance-readiness claim.
Current allowed payload is fixed-size but queue/record populations remain bounded
separately. Supported-target lint/build, actual memory/copy/allocation/page-fault cost,
latency distributions, backpressure/retry/cancel and full native effects remain ROOT
checks. This exception changes none of their required evidence.
