# Failure isolation: the constants and bounds behind implementation goal 5

Source notes for `docs/design/failure-isolation.md` (scheduler implementation goal 5, S8). They add,
to `proximity-and-failure-containment.md` (cited `PF[n]`), the primary sources each tuning constant and
each fault class's bound rests on, quoted where they say it. Gathered 2026-11-05 for kiln research
`01a1a9977d9574c8ad8072d4c2ea8c2b` (plan `01a1a9977d8a7153b1ee3c08e3240793`), where kiln checked each
quote at its locator; PDF quotes were taken with `pdftotext -layout` from the copies named.

Kinds as in the comparison: `MV` measured-vendor, `MI` measured-independent, `CL` claimed, `DE` design.

## 1. Membership: SWIM with Lifeguard

- **FI[1]** Dadgar, Phillips, Currey, "Lifeguard: Local Health Awareness for More Accurate Failure
  Detection", arXiv:1707.00788v2 (2018). Abstract: "slow message processing can cause SWIM to mark
  healthy members as failed (so called false positive failure detection), despite inclusion of a
  mechanism to avoid this". p.8: Lifeguard reduces "false positives by a factor of between 50x and
  100x." p.9: "The median increases 0.1 seconds (less than 0.1%)", and the 99th and 99.9th percentiles
  rise by about a "second (6-7%) for first detection and 1.5-1.8 seconds (7-9%)". `MV`.
- **FI[2]** hashicorp/memberlist v0.7.0, `config.go` (DefaultLANConfig):
  - :312 `IndirectChecks:          3,                      // Use 3 nodes for the indirect ping`
  - :314 `SuspicionMult:           4,                      // Suspect a node for 4 * log(N+1) * Interval`
  - :315 `SuspicionMaxTimeoutMult: 6,                      // For 10k nodes this will give a max timeout of 120 seconds`
  - :317 `ProbeTimeout:            500 * time.Millisecond, // Reasonable RTT time for LAN`
  - :318 `ProbeInterval:           1 * time.Second,        // Failure check every second`
  - :320 `AwarenessMaxMultiplier:  8,                      // Probe interval backs off to 8 seconds`
  `DE`.
- **FI[3]** memberlist v0.7.0, the suspicion timeout: `util.go:71`
  `nodeScale := math.Max(1.0, math.Log10(math.Max(1.0, float64(n))))` and `:73`
  `timeout := time.Duration(suspicionMult) * time.Duration(nodeScale*1000) * interval / 1000`;
  `state.go:1210` `k := m.config.SuspicionMult - 2`, `:1221` the minimum from `suspicionTimeout`, `:1222`
  `max := time.Duration(m.config.SuspicionMaxTimeoutMult) * min`; `suspicion.go:90`
  `frac := math.Log(float64(n)+1.0) / math.Log(float64(k)+1.0)` and `:91`
  `raw := max.Seconds() - frac*(max.Seconds()-min.Seconds())`. The code scales by log10(n), though the
  config comment says log(N+1). `DE`.

## 2. Gray failure and fail-slow

- **FI[4]** Huang et al., "Capturing and Enhancing In Situ System Observability for Failure Detection"
  (Panorama), OSDI '18. Abstract: it detects the "gray failures that we reproduced in less than 7 s,
  whereas existing approaches detect only one of them in under 300 s". `MV`.
- **FI[5]** Hayashibara, Défago, Yared, Katayama, "The φ Accrual Failure Detector", JAIST
  IS-RR-2004-010 (2004), p.9: "10 %. The likeliness is about 1 % with Φ = 2, 0.1 % with Φ = 3, and so
  on."; p.8: each application sets its own threshold on the one output. `DE`.
- **FI[6]** Gunawi et al., "Fail-Slow at Scale", FAST '18, p.6: "1% of the cases are detected in
  minutes, 13% in hours," "13% in days, 11% in weeks, and 17% in months". `MI`.
- **FI[7]** Panda et al., "IASO", USENIX ATC '19, abstract: "IASO can quickly and accurately isolate a
  slow node within minutes", deployed "5 years across 39,000 nodes in our customer sites". `MV`.
- **FI[8]** RFC 6298, "Computing TCP's Retransmission Timer": §2.1 the initial RTO of 1 second, §2.2 and
  §2.3 `SRTT`, `RTTVAR` (α = 1/8, β = 1/4) and `RTO = SRTT + max (G, K*RTTVAR)` with K = 4, §2.4 "if it
  is less than 1 second, then the RTO SHOULD be rounded up to 1 second", §2.5 "A maximum value MAY be
  placed on RTO provided it is at least 60 seconds", §3 Karn's algorithm (no sample from a retransmitted
  segment), §5.5 "back off the timer" (double it) on expiry. hyper-swim times its peers the same way
  (`crates/hyper-swim/src/detector.rs:30-31`, hyper-raft 6a236ae). `DE` (a standard).

## 3. Antagonists

- **FI[9]** Zhang et al., "CPI²: CPU performance isolation for shared compute clusters", EuroSys '13
  (`cpi2.pdf`). §3: CPI is gathered "for a 10 second period once a minute"; no CPI management "for
  applications with fewer than 5 tasks or fewer than 100 CPI samples per task"; an outlier is "larger
  than the 2σ point on the predicted CPI distribution"; "We ignore CPI measurements from tasks that use
  less than 0.25 CPU-sec/sec"; a victim "is flagged as an outlier at least 3 times in a 5 minute
  window". §4.1: the correlation over "a 10-minute window" with the suspect's usage normalised to sum to
  1, adding `u·(1 − c_threshold/c)` above the threshold and `u·(c/c_threshold − 1)` below; "requiring a
  correlation value of at least 0.35 works well". §4.2: "at most one of these attempts is performed each
  second"; caps "applied for 5 minutes at a time", "0.01 CPU-sec/sec for low-importance ("best effort")
  batch jobs and 0.1 CPU-sec/sec for other job types". `DE`, rolled out at Google scale (PF[16], `MV`).

## 4. Penalties: ejection, eviction rate, crash loops, retries

- **FI[10]** Envoy v1.36.0, outlier detection, ejection algorithm: "If the number of ejected hosts is
  above the threshold, the host is not ejected."; the ejection lasts the base time "multiplied by the
  number of times the host has been ejected in a row", and past `max_ejection_time` "it does not increase
  any more". `api/envoy/config/cluster/v3/outlier_detection.proto` at v1.36.0: :42 "Defaults to 30000ms
  or 30s."; :45 "The maximum % of an upstream cluster that can be ejected due to outlier detection.
  Defaults to 10% ."; :157 "the default value (300000ms or 300s)"; :177 "If enabled, at least one host
  is ejected regardless of the value of" `max_ejection_percent`. `DE`.
- **FI[11]** Kubernetes, kube-controller-manager reference: `--node-eviction-rate` (default 0.1)
  "Number of nodes per second on which pods are deleted in case of node failure when a zone is
  healthy"; `--secondary-node-eviction-rate` (default 0.01) the same "when a zone is unhealthy";
  `--unhealthy-zone-threshold` (default 0.55) "Fraction of Nodes in a zone which needs to be not Ready
  (minimum 3) for zone to be treated as unhealthy."; `--large-cluster-size-threshold` (default 50):
  "--secondary-node-eviction-rate is implicitly overridden to 0 for clusters this size or smaller."
  `DE`.
- **FI[12]** Verma et al., "Large-scale cluster management at Google with Borg", EuroSys '15, §3.3: "If
  a Borglet does not respond to several poll messages its machine is marked as down and any tasks it
  was running" are rescheduled; §3.5: Borg "rate-limits finding new places for tasks from machines that
  become unreachable, because it cannot distinguish" large-scale failure from a partition. `DE`.
- **FI[13]** Kubernetes, Pod Lifecycle, container restarts: the kubelet "restarts them with an
  exponential backoff delay (10s, 20s, 40s, …), that is capped at 300 seconds (5 minutes). Once a
  container has executed for 10 minutes without any" problems the backoff resets. An alpha gate lowers
  the cap to "60s (instead of 300s which is 5 minutes)"; Hex keeps the default. `DE`.
- **FI[14]** Google, Site Reliability Engineering, ch. 21 "Handling Overload", Deciding to Retry: "If a
  request has already failed three times, we let the" failure bubble up; a client's requests "will only
  be retried as long as this ratio is below 10%"; together this "reduces the growth to just 1.1x in the
  general case". `DE`.
- **FI[15]** Brooker, "Timeouts, retries, and backoff with jitter", Amazon Builders' Library: three
  retries at each of five layers and "the load on the database will increase 243x"; the AWS SDK's token
  bucket lets "all calls to retry as long as there are tokens, and then retry at a fixed rate when the
  tokens are exhausted." `DE`, a single vendor source (kiln grades it D): background only; Hex's budget
  rests on FI[14].

## 5. Agent faults and providers

- **FI[16]** Cemri, Pan, Yang et al., "Why Do Multi-Agent LLM Systems Fail?", arXiv:2503.13657v3, MAST:
  FM-1.3 "Step repetition - Unnecessary reiteration of previously completed steps in a process,
  potentially causing delays or errors in task completion", "step repetitions (FM-1.3, 15.7%)". `MI`.
- **FI[17]** Anthropic, Claude API errors: 429 `rate_limit_error`, and a spend-cap 429 "keeps failing
  until access resumes"; 529 `overloaded_error` "The API is temporarily overloaded." and "A 529 error can
  occur when the API experiences high traffic across all users." Rate limits: the retry-after header,
  "describing which rate limit was exceeded". OpenAI error codes: 429 "You are sending requests too
  quickly." against 429 credit balance exhausted, "Your organization has no prepaid credits remaining."
  `DE`.

## 6. Dangerous workloads

- **FI[18]** seccomp(2), Linux man-pages 6.19: `SECCOMP_RET_KILL_PROCESS` "results in immediate
  termination of the process"; `SECCOMP_RET_ERRNO` passes the filter's return value "to user space as
  the" errno, so the call fails and the program goes on; `SECCOMP_RET_LOG` runs the call and logs it;
  `SECCOMP_FILTER_FLAG_LOG` logs "All filter return actions except SECCOMP_RET_ALLOW"; the kernel can
  log the "actions returned by seccomp filters in the audit log". `DE`.
- **FI[19]** Agache et al., "Firecracker: Lightweight Virtualization for Serverless Applications", NSDI
  '20: "The traditional view is that there is a choice between virtualization with strong security and
  high overhead, and container technologies with weaker security and minimal overhead"; in a microVM,
  workloads "run in their own VMs under a hypervisor". gVisor's documentation: a "distinct, per-sandbox
  application kernel in order to minimize the risk of a" container escape. `DE`.

## 7. The disk-stall drill

- **FI[20]** Linux v6.12, `Documentation/admin-guide/device-mapper/delay.rst`: :5 "Device-Mapper's
  "delay" target delays reads and/or writes", :10 the table `<device> <offset> <delay> [<write_device>
  <write_offset> <write_delay>`…, :26 "Delays are specified in milliseconds." dmsetup(8): `reload`
  "Loads table or table_file into the inactive table slot", and on `resume` the new table "becomes live.
  Postponed I/O then gets re-queued". `DE`.

## 8. Gaps

- The repeat limit of a stuck tool loop: MAST (FI[16]) names step repetition and its share, but no
  source gives a threshold. Hex's is chosen and stated as such; kiln's recorded agent traces set it.
- Which provider errors on how many nodes make an outage: the providers say overload is "across all
  users" (FI[17]), not at what share of callers a scheduler should conclude it. Hex uses a majority of
  callers in a window, stated as its own choice.
- Security signals' trip rules: seccomp's actions are typed (FI[18]), but no source read says which
  should move a workload to a stronger class. Hex trips on kills, ungranted egress and refused privilege
  gains, not on errno'd calls, as its own choice with its falsifier.
