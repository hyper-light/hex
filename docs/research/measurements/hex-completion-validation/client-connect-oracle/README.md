# Constructor framing oracle correction (AC-T7.4.1)

Fixed successor parent: ROOT `28f571a29ecf43a74846076645c678130276b226`.
The exact original ROOT source was `02f7c4f1ce6989303e307e0a8328fe3f3295aa30`.
Its full client-constructor-three.log was read and independently hashed:
`3b04ee6e4ac8861cb688f5200fd691dada999bdba243af3ba611d72a7b20e9aa`.
The unchanged original bytes are copied here. Actual original result: 1 pass,
2 fail, 0 ignored. SDK3 / API23 Clippy results are separate ROOT evidence, not
attributed to this constructor batch or reexecuted by L4.

The positive constructor returned successfully with a complete 56-byte actual
frame. The old oracle called Deframer::take only once and wrongly required all
56 bytes consumed. The exact Deframer contract intentionally consumes the
four-byte prefix first, returning no message, and requires another take for the
body. Thus the 4-vs-56 assertion was a fixture framing error.

The negative whole-token row read all 8 actual token bytes and returned a typed
send expiry from the existing smaller learned 590.582 microsecond bound. The
old helper then unwrapped a missing message. Since the prior consumed-length
assertion passed, that row's trace was at most four bytes; the original raw did
not log the exact bytes/length, so whether it was empty or prefix-only is not
invented. A complete Hello send, and therefore withheld-answer proof, was not
established. The partial-token row independently read one byte and reached the
original caller cap. These are fixture/oracle observations, not a reproduced
production deadline failure. No timing seed or guard is enlarged or disabled.

The source-only correction iterates take with strict byte progress bounded by
the actual capture length, verifies exactly one complete Hello/build with no
trailing bytes, and preserves incomplete bytes/pending state as evidence. The
positive case still REQUIRES a full actual Hello. The negative keeps both
constructor forms and both token conditions, closing/joining all four owned
rows before assertions. It distinguishes committed Hello (the peer supplied
no answer) from learned-before-send or learned-during-incomplete-send. New
logs retain captured bytes and mark the latter withheld_answer_proof=false,
status=UNVERIFIED. A missing committed Hello still fails as a harness witness
miss; it never becomes a skip/pass or withheld-answer proof. It does not assert
that an OS wait cannot overshoot: the typed learned source distinguishes that
bound, while returned timestamps are retained for observation.

Only the two previously failed exact names in source.json change. The third
passed case is byte-identical and is not requested again. Production guest,
Call/Core, Deframer, Patience/census and the original Drop source are unchanged.
ROOT alone may run the two changed selectors on a joined pin. No builds, tests,
VM/native/GPU effects or check replay were executed here.

Policy receipt: implementer; fully read Shards7d7, unchanged rulesdf2;
exact parent Hex CLAUDE compared byte-identical to fully read e852. Signed
source is not a current native acceptance result or proof of all constructor
phases if a legitimate smaller bound preempts the intended witness.
