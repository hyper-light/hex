# ROOT-owned exporter profile, one complete run

AC-T7.4.1 / R14; coordinator policy7d7e0855, rulesdf2acf47. Exact measurement source
7d436e58d60d68158a27a02540ba4e453284631f, intakenROOT15e173fc.
Exporter SHA1f89937a6171918295932c4aa3dbba5658b55218c19bbb7d8a4d902e41bab3ec;
source seal27102cb1c12a42dd247d97e820895cb1b63e961bcf6d67924400e4c4b75c031c.
One actual fresh LB quiet ACK was retained verbatim and hash-pinned. The original
175s worker/180s scope/200+3s outer/210s Kiln guards and per-event synchronization
frequency were unchanged. Normal1CPU/4GiB/12GiB admission, nice10, swap0/pids256;
actual limits were checked. This was byte acquisition, not executable dispatch.

Actual export0:1596 objects/472712455 content bytes; complete archive474576896B,
SHAb5919c38358cbe1c57cd9b4968a04fe447c431356b4a7a4ca21ccb1c3bcd1507.
Export receipt1437B/93a93fbb1a8f00f9e213a338248178a79d5cde3c746eb8f385c450af41ce6b6c;
launcher2852B/547bcc96f6dfbd25ed147f00d08407406871164be29d9cb57ef75f256511b662.
Parent2594269/birth54900180 reaped and birth absent; owned scope absent/members[],
no observation errors. Local profile CLI38481 exited0/reaped. The quiet window was
closed on actual terminal. Old failed175s partial428191232B and evidence remain.
No retry, archive collection, materialization, build or GPU permission follows.

| Actual observation | Value |
| --- | --- |
| Source-event fsync API attempts/errors |3192/0 (two per verified object)|
| Source-event API duration total |36.429024748s|
| Attempt p50/p90/p99/max |9.390403/17.280464/29.731613/388.422648ms|
| Archive/manifest/output-directory fsync |1 each;18.632449/20.945387/11.994996ms|
| Operation phase |39.506268167s|
| Whole measured worker interval |39.593534884s|
| Own worker user/system CPU delta |1.09/0.85s|

Raw samples and exact phase counters remain in worker-final.json. ROOT recomputed
the nearest-rank percentiles, sample count, maximum and sum. These3192 correlated
attempts are from ONE run, not3192 independent workload trials. Operation includes
nested fsync time; do not add them as disjoint costs. These are Python API durations
and own worker CPU, excluding parent/child/cgroup-wide/kernel-device accounting.
Instrumentation, earlier cache/workload state and timing uncertainty prevent a
causal speedup claim against the old deadline failure. No4–5x ratio, zero-cost
instrumentation, accepted RuntimeCost or full platform/GPU result is established.

The bounded metadata-only retrieval did not read original source objects or the
474MB archive. Its compressed raw350046B SHA6e278ae5b647062ecb80b957b744c2064e6e42da84b5d710394ae40394b047c3
contains12 original receipt/log/manifest files; collection.json pins each member.
All members and fixed launcher/source-event/archive receipt pins were independently
verified. Source-event log has exactly3192 rows. A future archive collection needs
its separately reviewed source/explicit fresh quiet window and normal admission.
No fsync reduction or production resource-policy change was made.
