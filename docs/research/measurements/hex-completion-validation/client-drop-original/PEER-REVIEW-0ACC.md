# Read-only peer findings at exact0acc

This records L4's already-delivered implementation peer review of exact
0acc562d897140b88275314adcc72c3cece9ea86 and its CONSUMER-SOURCE.json dependency
pins. All ten declared guest-source hashes were checked and matched. Client
references below are exact910146edb088bd1d91a23bde257e9af7f57db618 unless noted.
No edits, tests, native work, delegation or formal audit campaign formed this
review. ROOT independently reads, integrates and owns R01–R16 audits/full gates.
These findings are source traces, not reproduced defects or readiness claims.

1. **Concrete deadline-aware Shards dial is missing from the listed closure.**
   0acc `guest/dedicated.rs:1151–1157` constructs ShardsLink. `observer.rs:479–490`
   calls run_before with stdout/stderr asked under the original deadline.
   Client `guest.rs:304–314` dials each named stream through
   Link::dial_before(Some(at)). But inherited `guest/shards.rs:105–126` implements
   only identity/dial, while `guest.rs:112–123`'s default rejects a finite
   deadline. Without a separately sealed/integrated override, the first named
   stream refuses before Run is sent. Keep the default fail-closed; no success
   from metadata/label/compile is inferred.

2. **Unfinished Proc destruction can renew authority after original expiry.**
   0acc `observer.rs:541–550` returns an error after its bounded delete attempt;
   its unfinished process local then drops. Client `calls.rs:2588–2600` sends
   Release through send_now. `calls.rs:1499` makes a NEW relative transport cap;
   line1507 can wait on sender ownership and line1527 can wait for a partial
   frame. With None or a later relative cap, final return can outlive the original
   Budget. The explicit run_before/delete_before calls themselves retain exact
   at; the escape is the preexisting destructor interaction. The newly authorized
   original test in this checkpoint targets the held-sender branch only. No
   production fix precedes ROOT's original result.

3. **Intent plus already-published native init blocks recovery cleanup.**
   0acc `gpu_pool.rs:264` persists Intent and line265 invokes native boot.
   Native publication persists canonical Pool init before launch returns; the
   local receipt becomes Running only at303–304. A crash/error between those
   writes leaves valid Granted/initSome plus Intent. Recovery at412–415 accepts
   Intent only if canonical init is None, otherwise returns Uncertain before
   attach_managed at417. The canonical original scope/init is still known; no
   new VM should be launched or claims refunded. This is a missing original-scope
   cleanup path, not proof of a native resource leak. Root1a9 core
   `record/pool.rs:213–217` expressly allows first native init publication during
   Granted and retains it thereafter.

4. **Failure records discard known exit and typed causes.**
   0acc `observer.rs:45–50`'s Attempt stores config/phase/streams but not actual
   exit, original failure or cleanup outcome. Lines512–516 convert an observed
   nonzero Exit to generic Invalid, losing the code/signal. If cleanup also
   fails, lines493–499 and541–549 discard both typed errors in favor of generic
   Backing Uncertain. Line545 persists the partial bytes/phase but not those known
   facts. This remains conservative about absence/claims, but does not preserve
   the distinct original observation and cleanup uncertainty for recovery or
   diagnosis. No textual error-parser remedy or new ledger is proposed here.

Positive traced boundaries: `observer/io.rs:198–216` requires independent actual
EOFs and removes completed sockets from subsequent native waits; receive retains
bound+1 bytes on overrun. `observer.rs:479–490,521–526` uses the same Budget
deadline, matches exact NoSuchBox identity and rechecks remaining authority.
`pool.rs:155–175` requires Complete, matching config/queues/report and both bounded
EOFs before readiness. `books.rs:172–213` persists original Creating/spec/scope,
releases Box turns before Pool handoff, then revalidates the same Box fields.
`gpu_pool.rs:118–136` retains claims until the same native owner reports Released
and terminal evidence is persisted. No alternate picker/holder/new host grant
was identified in these paths. These observations are not execution proofs.

Further artifact/execution-context/native acceptance remains explicitly open in
the consumer manifest: staged complete loader/ICD/transitive/dlopen/image pins,
selector-honoring context, actual guest execution/EOF/driver/compute facts,
original-scope recovery/termination and measured resource restoration. A read-only
host share or matching environment variable alone cannot substitute for ROOT's
artifact/context proof. Missing-PCI Windows physical-node proof stays unverified.
