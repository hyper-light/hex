# Agentic and LLM workloads: what they look like, how they are scheduled, what limits them, and what hardware they need

Hex places AI agents and their work. This note is the evidence base for that placement: how agent
workloads behave (burstiness, session shape, tool calls, idle time), what the LLM-serving and
agent-scheduling literature has measured, what the model providers' own documents say about the limits
a scheduler must respect, and which runtime classes can expose a GPU. It follows the method in
[`../README.md`](../README.md#method): every claim and number is cited to a source opened and read for
this note (accessed 2026-10-22), every number carries a kind, and a gap is written as a gap.

Kinds: `MI` MEASURED-INDEPENDENT (a third party measured it), `MV` MEASURED-VENDOR (the authors or the
operator measured their own system or service), `CL` CLAIMED (no stated method), `DE` DESIGN (a fact
from a document, code or specification, not a speed). `n/p` = not published, or not found in what was
read. Citations are `[n]` (References at the end). Limits quoted from provider documents are `DE` as of
the access date: they change, and Hex must read them at run time (response headers, limits APIs), not
compile them in.

Two cautions run through the whole note. First, almost every serving-research number is `MV`, from a
small GPU testbed with replayed datasets and synthetic (Poisson) arrivals; none of it measures a provider
quota. Second, the 2026 agent-trace studies are single-product (GitHub Copilot, Claude Code, Codex, one
Alibaba coding-agent product) and coding-heavy; Anthropic itself says it has "no reliable way" to link
API requests into agent sessions, so no provider-side trace of mixed agent fleets exists in what was read
[1].

*Verified (2026-10-22):* 318 figures and quoted phrases in this note were script-checked against the saved
text of their sources: 308 matched as written and the other 10 once formatting was allowed for (`≥ 75%`,
`2494×`, `1.4m`, table cells split across lines). Three quotations were corrected to the sources' exact
words, and three citations were repointed: DistServe for the goodput definition, Anthropic's errors page
for the 529 code, and Firecracker's VFIO issue and pull request for its passthrough status.

## Summary

1. **The unit of work is the session, not the request.** In GitHub Copilot's coding agent (one week of
   June 2026: 3.2M users, 13M sessions, 761M LLM calls) 87% of LLM calls are issued by the agent loop,
   so "user request arrivals alone do not predict LLM load" [2] (MV). Sessions are heavy-tailed: median
   3 user turns, 15 LLM calls, 4.2 minutes; mean 6.1, 40.6 and 62.6 minutes; P90 over three hours [2] (MV).
2. **Load is bursty at every scale, and mostly caused by a few tenants.** Diurnal peaks reach about
   4-5x the baseline [2] (MV). Short-term arrivals have a coefficient of variation above 1, with no single
   best-fit distribution, and the burstiness itself shifts hour to hour [3][4] (MV, MI). In one provider
   workload the top 29 of 2,412 clients sent 90% of requests [3] (MV). Failures start retry loops that
   raise compute by up to 4x [2] (MV).
3. **Agents spend most of their lifetime waiting, while holding state.** Human think time is 92.3% of
   session wall-clock in a coding-agent trace [5] (MI); the median production session executes for 20% of
   its lifetime, with idle gaps mostly 1-10 minutes and its sandbox still allocated [6] (MV); model
   inference occupies 5.6% of the time a session's KV state is retained [7] (MV). A session's sandbox
   working set has a median of 0.8 GB and peaks at 28 GB [6] (MV).
4. **Cost and capacity are about context, not generation.** Prefix-cache reads are 59.5% of API list-price
   cost; output is 11.2% [5] (MI, computed at list prices). A fixed 5-minute cache TTL that human gaps
   outlast caused evictions worth 31.5% of aggregate cost in a production coding agent [6] (MV).
5. **Serving research moved from the request to the program.** Autellix: 4-15x throughput at the same
   latency [8]; Parrot: up to 11.7x [9]; Llumnix: tail latency "by an order of magnitude", up to 36% lower
   cost [10]; SAGA: 1.64x task completion time [11]; SMetric: 9-15% peak throughput [12]. All `MV`, on small
   testbeds, mostly with synthetic arrivals. Only a handful of client-side schedulers address provider
   rate limits, and those are evaluated on mock providers or tiny real-API runs [13][14][15].
6. **Provider limits are multi-dimensional, hierarchical, token-bucketed and time-windowed.** Anthropic
   meters requests, input tokens and output tokens per minute per model class with a token bucket,
   discounts cache reads, and adds unquantified "acceleration limits" on sharp ramps [16]. OpenAI meters
   RPM, TPM and daily caps, shares limits across model families, and gives a ramp rule of at most +50%
   every 15 minutes above 1M TPM [17]. Google meters RPM, input TPM and RPD per project, adds rolling
   10-minute spend limits, and publishes live values only in AI Studio [18]. Subscription plans add a
   five-hour session limit and a weekly limit shared across products, scaled by model and effort [19][20].
7. **Placement must keep state warm, and the price of moving is measured.** When the next call of a
   session lands on another deployment of the same model, reuse of cached context falls from 88.9% to
   29.0% [7] (MV); a model switch leaves an 8% cache hit rate and is mostly a reaction to errors or rate
   limiting [2] (MV).
8. **GPU access is a property of the (node, runtime class) pair.** Kubernetes treats a GPU as an integer
   extended resource that cannot be overcommitted [21]; gVisor supports a short list of GPUs and a
   strict driver-version window [22]; Kata and Cloud Hypervisor assign whole devices through VFIO
   [23][24]; Firecracker's mainline has no device passthrough as of 2026-10-22 (tracking issue open, a
   community patch closed unmerged on 2026-09-30) [25][26][27][28].
9. **The objective is goodput, with early refusal under overload.** The literature converges on counting
   only finished, SLO-meeting work [29][14] and on rejecting early with a stated reason rather than
   timing out [30][14].

## 1. What agent workloads look like

### 1.1 The evidence base and its limits

| Source | Workload and scale | Whose data | Kind |
|---|---|---|---|
| Liu et al., Microsoft Azure Research, 30 Jul 2026 [2] | GitHub Copilot coding agent, sampled, one week of June 2026: 3.2M users, 13M sessions, 761M LLM calls, 95T tokens, 775M tool invocations | Microsoft's own telemetry | MV |
| Arya et al., Microsoft M365 Research and IISc, 16 Oct 2026 [7] | Four production M365 traces, over 121M model calls; Deep Research: 17M calls in 691k sessions over eight days of June 2026; Enterprise Chat: 104M calls in 13.2M sessions over 24 hours | Microsoft's own telemetry | MV |
| Wang et al., SJTU and Alibaba, 13 Sep 2026 [12] | Two clusters dedicated to one commercial coding-agent product (Alibaba Bailian), 10:00-12:00 on 29 May 2026; each cluster has "at least thousands of accelerators" | Alibaba's own telemetry | MV |
| Zhu et al., University of Washington, 30 Jun 2026 [5] | 4,265 sessions, 357,161 LLM steps, 432,510 tool calls from 43 developers, Sep 2025-Jun 2026, Claude Code and Codex | The authors' lab (not the providers); small, research-workflow sample | MI |
| Chang et al., HKUST, Alibaba, ByteDance, 15 Aug 2026 [6] | Ten applications in controlled runs (4,641 requests, 64,924 LLM calls, 118,274 tool calls) plus 24-hour production traces of three deployed applications (35,037 coding-agent, 141,376 search-QA and 2,386 office-automation sessions) | Controlled: the authors; production: operator not named | MI (controlled), MV (production) |
| Anthropic, Feb 2026 [1] | Claude Code interactive turns; a 500k-session sample for interruption analysis | Anthropic's own telemetry | MV |
| Xiang et al., Peking University and Alibaba, v3 11 May 2026 [3] | Alibaba Cloud Model Studio (Bailian) request logs, 2,412 clients in the client analysis | Alibaba's own telemetry | MV |
| Wang et al., HKUST-GZ and Huawei, v5 26 May 2025 [4] | 10.31M traces over 213 days from one regional Azure OpenAI service with over 3,000 users (ChatGPT and GPT-4; pre-agent) | One regional provider's logs, collected by third parties | MI |
| Patel et al., Microsoft, Splitwise [31] | Azure LLM inference traces of 11 Nov 2023, 20 minutes each: coding and conversation | Microsoft's own telemetry | MV |

### 1.2 Session shape: heavy tails everywhere

- **Sessions.** Copilot: median 3 user turns, 15 LLM calls, 13 tool invocations and 4.2 minutes; mean
  6.1, 40.6, 43.6 and 62.6 minutes; P90 15 turns, over 100 LLM calls and a duration over three hours; the
  duration's mean-to-median ratio is 14.9x [2] (MV). TraceLab: per user request an agent does on average
  8.8 LLM calls and 10.8 tool calls, taking on average 4.3 minutes (median 38 s, p90 6.4 min); per session
  on average 9.2 requests (p99 137) and 101.4 tool calls (p50 25, p99 1,438) [5] (MI). M365 Deep Research:
  median session depth 13 model calls, P99 160, against 3 and 52 for the lower-agency Enterprise Chat [7]
  (MV).
- **Turns.** Anthropic: the median Claude Code turn lasts about 45 seconds (between 40 and 55 s over the
  past months), while the 99.9th-percentile turn grew from under 25 minutes (late September 2025) to over
  45 minutes (early January 2026) and was about 42 minutes in February 2026 [1] (MV). Experienced users let
  Claude run unattended more: full auto-approve in roughly 20% of sessions for new users, over 40% for
  experienced ones [1] (MV).
- **Who consumes.** In Copilot five user archetypes span a 50x range in per-turn token consumption (23K
  to 1.1M tokens) [2] (MV). In Bailian's agent clusters the top 25% of sessions carry over 80% of tokens
  [12] (MV).
- **Datasets used by schedulers.** Autellix's workloads, replayed rather than traced from production:
  ShareGPT conversations mean 6.66 LLM calls (max 80), BFCL tool-use mean 10.75 (max 70), LATS tree search
  mean 159.7 calls per program with parallel threads [8] (MV). Parrot's table of four applications: 2-40
  calls for long-document analytics, 14 for MetaGPT (17k tokens) and 17 for AutoGen (57k tokens) [9] (MV).
- **Benchmarks are not sessions.** TraceLab argues that benchmark tasks capture "one request in isolation"
  and underestimate context growth over a session [5] (MI). The UIUC/Intel study finds agentic workloads
  long-tailed in two distinct ways, many turns or large contexts with few turns, in benchmark runs of
  Claude Code against vLLM-served Gemma and Qwen [32] (MI).

### 1.3 Tokens and context: long input, short output, growth

- Median Copilot call: 68K prompt tokens and 247 output tokens (Table 2 of [2], MV). Median TraceLab step:
  about 119K prefix (cache-read) tokens, 875 appended tokens and 214 output tokens; the Claude prefix p99
  is 918K tokens [5] (MI).
- Context grows with depth: median 10K tokens at the first call to 68K by the 30th in M365 Deep
  Research; about 80% of context tokens come from the prefix cache [7] (MV). Prompts are 37x to 2,494x
  longer than the generated sequences, with 85% to 97% of prompt tokens shared with other prompts, across
  five workloads including tool calling and embodied agents [33] (MV).
- Compaction is the release valve and a cost event: it is triggered in 7.8% of Copilot sessions but touches
  44% of total tokens, dropping over 70% of prompt tokens and resetting the cache like a model switch
  [2] (MV). In a production coding agent, 3,170 compaction events averaged 156 s (p95 161 s, p99 775 s)
  and 99% of them cut the context by more than 70% [6] (MV).
- **Cost.** At list prices prefix-cache reads are 59.5% of session cost, appended input 29.2%, output
  11.2%; a session costs $9.70 on average but $0.61 at the median (p99 $178) [5] (MI, computed).

### 1.4 Arrival patterns and burstiness

- **Diurnal and weekly.** Copilot activity "peaks during daytime hours, reaching approximately 4-5x the
  baseline level, and declines substantially overnight (and during the weekend)", all metrics moving
  together [2] (MV). Bailian shows afternoon peaks and early-morning troughs [3] (MV). BurstGPT: chat
  volumes peak on weekdays and at working hours, but API-service traffic is aperiodic and bursty, and its
  higher day-to-day variance "suggests these requests may be automated" [4] (MI).
- **Short-term burstiness.** Bailian request inter-arrivals have a coefficient of variation (CV) above 1,
  so Poisson (CV = 1) fits poorly; Gamma or Weibull fit better but "there is not a universally best
  modeling", and one general-purpose workload was bursty for two days and stable by Thursday [3] (MV).
  BurstGPT models burstiness every 20 minutes with a Gamma whose shape parameter changes sharply within
  working hours [4] (MI); its mean arrival rates were 0.019 requests/s (conversation) and 0.21 requests/s
  (API) for ChatGPT [4] (MI).
- **Concentration.** Out of 2,412 clients the top 29 sent 90% of requests; the burstiness of the top
  clients is "mostly stable" over 48 hours, and workload-level bursts trace back to a few top clients [3]
  (MV).
- **The agent loop sets the arrival process.** "Takeaway 2: 87% of LLM calls are agent-initiated. User
  request arrivals alone do not predict LLM load; capacity planning requires session- or turn-level
  modeling" [2] (MV). In Bailian's agent clusters about 90% of KV-cache reuses recur within roughly 100 s
  of the previous request sharing that cache, because the agent sends the next request as soon as it sees
  the tool result [12] (MV).
- **Statistical multiplexing helps providers, not tenants.** In Bailian a one-minute window holds on average 43x
  (Trace 1) and 48x (Trace 2) as many requests as there are serving instances, so token load stays within
  a median max/mean imbalance of 1.7 and 1.5 even though sessions are skewed [12] (MV). A single tenant
  with a few agents does not get that smoothing.
- **Retry bursts.** Tool failures occur in 9% of Copilot turns and start autonomous retry loops with
  growing contexts, amplifying compute up to 4x; a failure-heavy turn averages 36 LLM calls, four times the
  median [2] (MV).
- **Fan-out inside a session is shallow, with a tail.** In Copilot 36.7% of turns are strictly sequential;
  median LLM-call concurrency is 1.15 and P90 1.4; 93% of tool batches hold one tool, parallel batches have
  median width 2, and the tail reaches 108 concurrent tool invocations [2] (MV). TraceLab: 1.2 tool calls
  per step on average [5] (MI). Tree-search programs fan out far more (LATS: 159.7 calls per program) and
  one engine could serve only 0.2 such programs per second [8] (MV). The larger fan-out in practice is
  across sessions: one user starting 11 parallel coding agents on one API key lost 3 of them to
  connection resets and 502s (an anecdote, n = 1) [13] (MV).

### 1.5 Tool calls

- Tool use is skewed: over 80 distinct tools, but the top three account for over 80% of calls; calls
  longer than 1 minute are 4% of calls and 85% of all tool-call time, with latencies from milliseconds to
  hours [5] (MI).
- Non-LLM components dominate latency in 5 of 10 applications; component task latencies diverge by up to
  32x; a production search tool had mean, median and p90 latencies of 1.349 s, 1.258 s and 1.708 s [6]
  (MI controlled, MV production). Tool calls were 30-85% of the latency to first rendered token in
  production Microsoft agentic requests [34] (MV).
- The wait for an interception varies by orders of magnitude: a calculator under 1 ms, a human over 1
  minute; recomputing already-processed context at each interception was 37-40% of model forwarding time
  in the systems studied [35] (MV).
- Tool results are shareable: across 373,678 production search calls 27% of distinct queries recurred and
  accounted for 67.3% of invocations; caching removed 35.2% of redundant search calls and 19.3% of
  aggregate search latency [6] (MV).
- Agents shift from read/explore early to execute/write later in a run [32] (MI).

### 1.6 Idle time: "waiting" is the common state

- Human think time is 92.3% of coding-session wall-clock, LLM generation 3.3% and tool execution 4.8%
  (human gaps capped at one hour would make it 64.3%, 14.5% and 21.2%); positive human-input waits have
  median 1.4 minutes and p90 20.6 minutes; session p99 elapsed time is about 206 hours [5] (MI).
- Copilot idle time is bimodal: more than 90% of idle intervals are inside a turn, with medians of 5.8 s
  (tool container) and 1.2 s (KV cache); the 8-9% of intervals that span a turn boundary have medians of
  243 s and 172 s; the median user idle time between turns is 1,512 s (25.2 minutes), with a tail past a
  day [2] (MV). A LightGBM predictor of the idle time captured 86-90% of total idle time, with ROC-AUC 0.73
  for "idle over 60 s", and under 3 ms per prediction [2] (MV).
- In production coding-agent sessions the median session executes for only 20% of its lifetime and 70%
  execute for less than half; idle gaps run from seconds to hours, "most intervals fall between 1 and 10
  minutes", and the per-session sandbox stays allocated throughout. The authors argue a binary
  running/finished lifecycle is insufficient and ask for a third, quiescent "waiting" state [6] (MV).
- In M365 Deep Research, inference occupies only 5.6% of the time a session's KV state is retained;
  holding every live session's KV state at its peak would need an estimated 501.8 TB of GPU HBM (under a
  stated Llama-2-70B KV layout), with peak concurrently live sessions of 11,358 [7] (MV).
- Sandbox state: median peak DRAM about 0.8 GB, per-session peak 28 GB during compilation and unit tests;
  checkpointing is best done at quiescent boundaries rather than at the peak [6] (MV).

## 2. LLM serving and agent-scheduling research

What each system schedules, what it optimizes, and what it measured. All numbers are as reported by the
authors for their own system (`MV`) unless the row says otherwise, on the workload and testbed in the
last column.

| System | What it schedules | Objective | Reported result | Workload and testbed |
|---|---|---|---|---|
| vLLM / PagedAttention [36] | KV-cache memory in fixed-size blocks; continuous batching | Throughput at equal latency | "2-4x" higher throughput than FasterTransformer and Orca at the same latency; only 20.4%-38.2% of KV memory held real token states in the older systems | Single-engine; OPT/LLaMA models; SOSP 2023 |
| Sarathi-Serve [37] | Chunked prefill, stall-free batches inside an engine | Throughput under tail-latency bounds | 2.6x serving capacity (Mistral-7B, one A100), up to 3.7x (Yi-34B, two A100), up to 5.6x (Falcon-180B, pipeline) vs vLLM | Microsoft Research India |
| SGLang [38] | Programs with many calls; RadixAttention prefix-cache reuse | Throughput on multi-call programs | "up to 6.4x" higher throughput than state-of-the-art | Agent control, reasoning, JSON decoding, RAG, multi-turn chat |
| DistServe [29] | Prefill and decode on different GPUs, placement by bandwidth | Goodput: rate served within TTFT and TPOT SLOs | "7.4x more requests or 12.6x tighter SLO", with over 90% of requests within the latency constraints | Several LLMs and applications |
| Splitwise [31] | Phases on separate machines, including mixed GPU generations | Throughput, cost, power | "up to 1.4x higher throughput at 20% lower cost", or "2.35x more throughput under the same power and cost budgets" | Azure coding and conversation traces |
| Andes [39] | Token-level preemptive scheduling | Per-user quality of experience in streaming | Average QoE "up to 4.7x" at the same GPU resource, or "up to 61%" fewer GPUs at the same QoE | Text-streaming services |
| Mooncake [30] | KVCache-centric conductor, disaggregated KV cache, early rejection of requests under overload | Effective throughput within TTFT/TBT SLOs | "up to a 525% increase in throughput" in simulated long-context scenarios; "75% more requests" handled by Kimi under real workloads | Moonshot AI's Kimi production service |
| Preble [33] | Global prompt-prefix-aware routing across GPUs (explore/exploit) | Latency with load balance | "1.5x to 14.5x" average and "2x to 10x" p99 latency vs state-of-the-art serving | Five workloads incl. tool calling, embodied agents; Azure trace arrival patterns |
| InferCept [35] | Requests paused by tool calls: preserve, swap or discard KV by interception duration | Throughput of tool-augmented generation | Recomputation was 37-40% of forwarding time; 1.6x-2x throughput and 2x more requests/s | UCSD; ICML 2024 |
| Llumnix [10] | Requests across model instances, by live migration of KV state | Tail latency, priorities, cost | P99 first-token latency up to 15x, P99 per-token latency up to 2x better than INFaaS; high-priority requests 1.2x-1.5x faster; up to 36% cost saving at similar tail latency | 16 A10 GPUs; LLaMA-7B and -30B; Poisson and Gamma arrivals, ShareGPT and BurstGPT lengths |
| Parrot [9] | LLM-based applications as dataflow (Semantic Variable): co-scheduling of dependent calls, prefix sharing | End-to-end application latency, derived per request | "up to 11.7x" (multi-agent MetaGPT, one A100); "12x" higher request rate (GPT-store apps, 4 A6000); chain summaries 1.38x faster than vLLM, 2.38x with background load | SJTU and Microsoft Research |
| Autellix [8] | Agent programs (DAGs of calls); PLAS and ATLAS priority by cumulative service; locality-aware multi-engine routing | Program latency and throughput, with P95/P99 fairness | "4-15x" throughput at the same latency vs vLLM; up to 1.5x (intro) or 1.4x (section 6.4) over round-robin or least-used routing | 8x A100-80GB; LLaMA-3.1 8B and 70B, Falcon-180B; ShareGPT, BFCL, LATS; Poisson arrivals |
| Continuum [40] | KV-cache time-to-live pinned across tool calls plus program-level FCFS | Job completion time of multi-turn agents | Delay "1.12x to 3.66x" lower and throughput "1.10x to 3.22x" higher; up to 8.18x on a company's internal testbed | SWE-Bench, BFCL, OpenHands; Llama-3.1 8B/70B, Gemma-3 12B, GLM-4.5 355B |
| SAGA [11] | Agent workflow graphs; KV reuse across tool calls; session-affinity batching with work stealing; "Agent Fair Share" | Task completion time, fairness | 1.64x faster task completion vs vLLM with prefix caching and affinity routing; 99.2% SLO attainment; about 30% lower peak throughput than throughput-optimal scheduling | 64 A100-80GB; SWE-bench, WebArena; BurstGPT-derived tenants |
| Sutradhara [34] | Orchestrator and engine co-design: overlap tool execution with prefill | First-token-rendered latency | Up to 77% higher load at the same median latency, or median latency down up to 15% and end-to-end down up to 11% | A100; Microsoft M365 Research |
| SMetric [12] | Session-centric routing: first turn balanced, follow-ups stick to the instance with the best local hit unless the SLO would break | Tokens/s within SLO | 9-15% more peak throughput than the best baseline (colocation); 9% prefill (disaggregation) | Replay of two Bailian agent traces |
| Helium [41] | Agent workflows as query plans; cache-aware scheduling | Makespan | "up to 1.56x" over state-of-the-art agent serving | NUS; several workloads |

### 2.1 The three that matter most for Hex

**Autellix [8]** treats the agent program as the schedulable unit and is non-clairvoyant: it assumes no
knowledge of a program's future calls. Its PLAS policy prioritizes a call by the cumulative service time of
the program's completed calls (long programs sink, short ones finish first); ATLAS, for multi-threaded
programs, uses the maximum cumulative service across threads, a critical-path proxy. Anti-starvation
promotes calls whose wait-to-service ratio passes a threshold. Its load balancer routes short requests
(2,048 tokens or fewer, which hit the cache at 75% or more on any engine because of common system prompts) to the least-loaded engine and
pins long requests to their program's engine, because within-program cache-hit rates stay above 90% while across programs the cache-hit rate "decays exponentially" with input length. Measured, on top of vLLM: single-threaded
workloads up to 8x vLLM and 1.5x an MLFQ baseline; LATS up to 5x vLLM, 2.5x MLFQ; mixed up to 15x vLLM,
5.5x MLFQ and 4x an optimized vLLM; offline makespan 10-40% lower, and MLFQ ran out of memory at 4,000
programs. Programs spent most of their time waiting as load rose; Poisson program arrivals were
synthesized, and tool-call "interceptions" are outside what Autellix schedules [8] (MV).

**Parrot [9]** exposes the application's data flow (Semantic Variables) to the service, so dependent
calls are co-scheduled on the server side, and a per-request objective is deduced from the application's
goal (the map stage of a map-reduce wants throughput, the reduce stage wants latency). Its motivating
measurement from a production chain-style application: 30-50% of API-call latency, over 70% in the worst
cases, arises outside the engine, from network and queueing between dependent calls; and 94% of tokens in
the chat-search application's requests, 72% in MetaGPT and 99% in AutoGen are repeated across calls
(Table 1) [9] (MV). The evaluation emulated 200-300 ms of network delay, and used A100 and A6000 GPUs.

**Llumnix [10]** reschedules running requests across instances by live migration of KV state, pipelining
the copy with decoding so downtime is "roughly 20-30 ms", nearly constant in sequence length, against up
to 111x more for blocking copy or recompute (recomputing an 8k sequence for LLaMA-30B took 3.5 s, about 54
decode steps). One policy (virtual usage) unifies load balancing, defragmentation, priorities and
auto-scaling. Motivation: at 62% average memory load 8% of requests were preempted, the P99 per-token
latency was 3.8x the P50, and preemption accounted for 70% of the P99 request's loss. Priorities were
tested with 10% of requests marked high-priority under Gamma arrivals of rising CV. Measured on 16 A10
GPUs [10] (MV); the 64-instance scalability test replaced GPU execution with a calibrated sleep.

### 2.2 What this literature does not do

- It schedules **GPU engines the authors control**. Hosted-API consumers cannot reorder or preempt in-flight
  requests and see only success or 429 [14]. Hex, placing agents that call providers, is in that position
  for hosted models and in the engine-operator position only for any models it hosts itself.
- It rarely models **quota**. The exceptions are small: HiveMind (a transparent proxy with admission
  control, header-driven rate tracking, AIMD backpressure with circuit breaking, token budgets and
  priorities) cuts failures of 5-50 concurrent mock agents from 72-100% to 0-18%, and found transparent
  retry the single most critical primitive, but runs against a mock API server; its one real incident is
  11 agents on one key with 3 dying [13] (MV, mock). A Zhihu client-side study decomposes a black-box API
  scheduler into allocation (deficit round robin across classes), ordering and overload control (a
  defer/reject cost ladder) and reports, on a mock provider, 100% completion and deadline satisfaction at
  4.2 +/- 1.6 SLO-meeting requests/s under balanced high congestion, a P95 inflation of up to 5.8x for short
  requests when size priors are removed, and graceful degradation to 60% multiplicative error in the size
  prior [14] (MV, mock). A Stanford course project (unreviewed) built a dual RPM/TPM token-bucket proxy
  with progress-aware priority: about 34-35% lower mean session time than FIFO when RPM binds, 18-22% when
  TPM binds, but with real API calls at 5,000 RPM and 2,000,000 TPM the gain was nil when queues were
  shallow (FIFO 94.1 s vs 99.1 s mean) and 9-15% only once arrivals made queues deep [15] (MI, student
  project).
- Its arrival processes are **synthetic**: Poisson (Autellix, Llumnix) or Gamma with chosen CV (Llumnix),
  or replays of chat datasets; the agent traces of section 1 are newer than most of these systems.

## 3. Scheduling against provider limits

### 3.1 What the providers publish

| | Anthropic API [16] | OpenAI API [17] | Google Gemini API [18] |
|---|---|---|---|
| Metered dimensions | Requests, input tokens and output tokens per minute (RPM, ITPM, OTPM), per model class | RPM, RPD, TPM, TPD, images per minute, audio minutes per minute, as applicable; "hit across any of the options depending on what occurs first" | RPM, input TPM, RPD; images per minute or TPD for some models |
| Scope | Organization; optional lower workspace limits; org limits always apply | Organization and project, "not user level"; some model families share one limit; separate limit for long-context requests (GPT-5.5) | Per project, "not per API key" |
| Time basis | Token bucket: "continuously replenished up to your maximum limit, rather than being reset at fixed intervals" | Reset times returned in headers (sample: 1s for requests, 6m0s for tokens) | RPD resets at midnight Pacific; spend limits on a rolling 10 minutes |
| Cache effect | Cache reads do not count toward ITPM on most models (example: 2M ITPM and 80% hit rate allows 10M total input tokens/min); cache writes count; OTPM counts real output only, not `max_tokens` | n/p on the page read | n/p on the page read |
| Ramp control | "Acceleration limits" on sharp usage increases (value n/p); "increase your traffic gradually" | `slow_down` 429 when traffic rises too fast even within RPM/TPM; rule of thumb: above 1M TPM, raise by no more than 50% every 15 minutes | Spend-based limits per 10 minutes: Tier 1 $10, Tier 2 $50, Tier 3 $200 |
| Tiers | Evaluation, Start, Build, Scale, Custom by usage history; monthly spend caps $500, $1,000, $200,000 | Free, Build, Launch, Grow by credit purchases; monthly usage limits $100, $500, $5,000, $200,000 | Free, Tier 1-3 by cumulative spend ($0, $100 + 3 days, $1,000 + 30 days) |
| Published values | Full table (below) | Table for four model families by tier | Values only in AI Studio; "Specified rate limits are not guaranteed and actual capacity may vary" |
| Error and header signals | 429 `rate_limit_error` with `retry-after` (not every 429 has it); `anthropic-ratelimit-{requests,tokens,input-tokens,output-tokens}-{limit,remaining,reset}`; 529 `overloaded_error` for capacity [42] | 429 and `Retry-After`; `x-ratelimit-{limit,remaining,reset}-{requests,tokens}` plus project-token headers; 503 `server_is_overloaded` | 429 `RESOURCE_EXHAUSTED` for spend-based limits; headers n/p on the page read |
| Asynchronous and priority classes | Batch API with its own RPM and queue limits; Priority Tier only for existing commitments; Standard is best effort [43] | Batch queue limits by queued input tokens; Scale Tier and Reserved Tier mentioned | Priority consumption at 0.3x the standard limit by default; Batch API: 100 concurrent batch requests, 2 GB input file |

All cells are `DE`, read 2026-10-22. Anthropic's published table (RPM / ITPM / OTPM) is, for Claude Opus
5.5, Sonnet 5.5 and Haiku 5.5: Start 1,000 / 2,000,000 / 400,000; Build 5,000 / 5,000,000 / 1,000,000; Scale
10,000 / 10,000,000 / 2,000,000. The Fable 5.x class has lower limits: Start 1,000 / 500,000 / 100,000;
Build 2,000 / 1,500,000 / 300,000; Scale 4,000 / 4,000,000 / 800,000. Some rows are pooled across model
generations (all Opus 4.x share one bucket; Opus 5.5 and Opus 5 are separate) [16]. OpenAI's standard
table lists, for its first three model names, 5,000 RPM / 1,000,000 TPM (Build), 10,000 / 4,000,000
(Launch) and 15,000 / 40,000,000 (Grow), and a fourth family at 5,000 / 2,000,000, 10,000 / 10,000,000 and
30,000 / 180,000,000 [17]. Anthropic's Managed Agents endpoints are limited separately: 300 create requests
and 1,200 read requests per minute per organization [16].

What this means for a scheduler, from the documents alone:

- **The binding constraint is a multi-dimensional, hierarchical bucket**, not one number: a call is
  admitted only if requests, input tokens and output tokens all have budget at the org, workspace/project
  and model-class levels, and tokens must be estimated before the call (Anthropic estimates ITPM at the
  start of a request and corrects it during the request) [16].
- **Ramp rate is a separate constraint** from level. Both Anthropic and OpenAI throttle sharp increases
  even when under their per-minute limits [16][17]. A burst of agents starting together is exactly this
  shape.
- **A 429 can mean four different things**: a per-minute limit (retry after the header), an acceleration
  or `slow_down` limit (reduce, then ramp), a tier spend cap (no `retry-after`; retrying "fails until
  access resumes") and, on Google, a rolling-spend limit [16][17][18]. Treating them alike wastes tokens and
  time. Capacity overload is a different code (Anthropic 529, OpenAI 503) [42][17].
- **Failed requests still count**: OpenAI states that unsuccessful requests contribute to the per-minute
  limit [17]. Retries must be budgeted, and SDK retries and application retries must not multiply (OpenAI
  advises disabling one or accounting for both) [17].
- **Quota is shared by everything under the same org/project/key**, including other teams' agents; Google
  and OpenAI scope by project, Anthropic by organization and workspace [16][17][18].

### 3.2 Subscription windows (human-plan agents)

Anthropic's Pro and Max plans have a five-hour session limit and a weekly limit (Max also has a separate
weekly limit for one model class), shared across claude.ai, Claude Desktop and Claude Code; usage depends
on the length and complexity of the work, the features used, the model and the effort level; there is a
"wrap-up" allowance when a response is in progress at the limit [19][20] (DE). No numeric allowance is
published on those pages (n/p). A scheduler that places agents driven by a person's subscription therefore
has a per-person, multi-window budget it cannot read ahead of time, only observe as warnings and limit
errors, and that is drained by every product the person uses. OpenAI's equivalent (a Codex window plus a
weekly quota) could not be verified: OpenAI's help-center page returned HTTP 403 to my fetch, and a web
search returned only third-party reports (not opened, so not relied on).

### 3.3 Rate-limit-aware routing in gateways

| Gateway | What it does with limits | Notes |
|---|---|---|
| LiteLLM Router [44] | Default `simple-shuffle` picks deployments by weight, or by configured `rpm`/`tpm`; optional usage-based, latency-based, least-busy and cost-based strategies; a pre-call check filters deployments over their configured tpm/rpm; cooldown of a deployment on a 429 (default 5 s), after over 50% failures in a minute, or on 401/404/408; per-deployment `max_parallel_requests` with no queue: excess requests "either run or get the 429"; ordered deployments with fallbacks; session affinity pins a conversation to one deployment, recommended "for example, [for] provider-side prompt caching" | The docs warn usage-based routing "is not recommended for production due to performance impacts" (it tracks usage in Redis); tpm/rpm are operator-configured, not read from provider headers |
| OpenRouter [45] | Default: providers without significant outages in the last 30 s, then weighted by the inverse square of price; `sort` by price, throughput or latency disables load balancing; latency and throughput percentiles (p50-p99) over a rolling 5-minute window; `preferred_*` thresholds only deprioritize ("should therefore never prevent your request from being executed"); `max_price`, `require_parameters`, `zdr`, `data_collection`, `only`/`ignore` are hard filters | A clean split between hard filters and soft preferences |
| Portkey [46][47][48] | Weighted load balancing across providers, models or API keys ("Distribute load across rate limits from different accounts"); sticky routing keyed on metadata fields with a 3,600 s default TTL; fallback on any non-2xx by default, or on listed status codes such as 429; conditional routing by request metadata; strategies compose | Weights are static; limits are not tracked |

`DE` throughout. What these have in common: static or operator-configured limits, retry-and-cooldown on
429, weighted shuffles, and an explicit affinity feature for provider-side caches. What none documents:
reading the provider's own remaining-budget headers into the routing decision, ramp-rate control, or
token-estimate-based admission. HiveMind's design does read those headers (proactive pause at a
configurable fraction of limit remaining) [13].

### 3.4 What production data says about limits and routing

Copilot's "auto mode" re-routes a session when the selected model is throttled: model switches affect about
6.4% of sessions, are "predominantly reactive" to errors (36% non-success rate before a switch against 8%
baseline) or rate limiting, and manual-to-auto switches are 52% downgrades, 35% lateral and 13% upgrades
[2] (MV). The cost of switching is paid in cache: the hit rate falls to 8% after a switch (a drop of 67%
points from the same-turn level), so the paper recommends "session-to-model pinning and proactive cache
staging on the target model" [2] (MV).

## 4. Context-window and KV-cache-aware placement

- **Affinity is worth a lot, and it is session-shaped.** Within a program Autellix measured cache-hit rates
  above 90% at every input length, against exponential decay across programs [8]. In Bailian agent traces
  over 80% of KV is reusable and more than 65% of reuse is later turns of the same session; the first turn
  still hits the shared system prompt for 18-20% of reuse [12] (MV). In M365 the mean reuse of a
  same-deployment continuation was 88.9% against 29.0% when the same model was served by another
  deployment in the region and 23.5% across regions; a Deep Research session touched 4.5 deployments on
  average, raising the unavoidable-recompute floor from 5.4% to 20.2% [7] (MV). Parrot's affinity
  scheduling sustained 12x the request rate of the no-sharing baseline, but only 3x with affinity turned
  off [9] (MV).
- **Affinity has a limit: load.** SMetric finds cache-aware schedulers crowd requests onto the few
  instances that hold the cache, and balanced schedulers lose reuse; it balances first turns, sticks
  follow-ups to the best-hit instance only if its SLO holds, and otherwise migrates the session (9-15%
  more peak throughput) [12] (MV). Preble uses explore/exploit: reuse a prefix when the tokens saved
  exceed the tokens to compute, otherwise explore another GPU [33] (MV). Autellix pins only requests long
  enough that recomputation hurts [8].
- **Retention windows are short and the human gap outlasts them.** TraceLab: misses begin at gaps over 5
  minutes and "after 1 hour, almost all steps miss"; the global hit rate is 95.7% but cache misses cause
  3.8x more tokens to be prefilled than truly new input; user-initiated steps hit 84.4% against 97.5% for
  tool-result steps [5] (MI). A production deployment's fixed 5-minute TTL meant 59.4% of 35,037 sessions
  saw at least one eviction [6] (MV). Anthropic's default cache TTL is 5 minutes, refreshed at no charge on
  each use, with a 1-hour TTL at 2x the base input price (5-minute writes 1.25x); cache hits cost 0.05x the
  base input price on Opus 5.5 and Sonnet 5.5 (0.025x on Fable 5.1); caches are isolated per workspace
  [49] (DE). A scheduler cannot move a provider-side cache, but it can keep an agent on the same
  workspace/key and time its calls inside the TTL.
- **Policy for waiting state.** Continuum pins KV in GPU memory for a TTL set by reload cost and queueing
  delay, expiring it when the tool call overruns [40]; InferCept chooses preserve, swap or discard by
  expected interception time [35]; Copilot's idle predictor gives the survival curve a resource manager
  needs [2]; with a one-tenth memory budget, LRU eviction made 63% of Deep Research calls recompute, a
  clairvoyant furthest-next-use policy 31%, and demoting the LRU session's KV to int4 13.6% [7] (MV).
- **Gateway standards are moving the same way.** Kubernetes' Gateway API Inference Extension defines an
  endpoint picker that chooses a model-server endpoint from the server's reported metrics and
  capabilities, "like Prefix Cache status or LoRA Adapters availability", with model-aware routing and
  serving priority [50] (DE).

## 5. Model, effort and cost-aware routing

- **Effort is a first-class cost and latency dial.** Anthropic's `effort` parameter (`low`, `medium`,
  `high`, `xhigh`, `max`) "affects all tokens in the response", including tool calls: "Lower effort also
  means fewer and terser tool calls"; the docs recommend `low` for subagents and `xhigh` for "long-running
  agentic and coding tasks (over 30 minutes) with token budgets in the millions"; changing effort
  mid-conversation through a per-message setting preserves the prompt cache [51] (DE). The consumer-plan
  article says usage depends on the model and the effort level [20] (DE).
- **Cheaper-model routing is established for chat.** RouteLLM: routers trained on preference data "reduce
  costs by over 2 times without sacrificing response quality" between a strong and a weak model [52] (MV);
  FrugalGPT: a cascade matched the best single model with "up to 98% cost reduction" or improved accuracy
  by 4% at equal cost, and list prices differed by up to two orders of magnitude across 12 commercial
  APIs [53] (MV). Neither evaluates multi-step tool-using agents; routing mid-session also costs cache
  (section 3.4) and, for Anthropic, changes which rate-limit bucket pays.
- **Rate limits are per model class** (section 3.1), so model choice is also a quota-allocation decision:
  the same org can have headroom on one class and none on another.

## 6. Hardware capability: GPUs and devices per runtime class

Hex's classes (docs/design/README.md) are `process`, `oci`, `firecracker` and `shards`. What each can
expose is below, from the projects' own documents (`DE`).

| Class or runtime | GPU and device exposure | Constraints that must become placement facts |
|---|---|---|
| Container (OCI) with a device plugin or CDI | Kubernetes exposes a GPU as an extended resource such as `nvidia.com/gpu`, specified only under limits; extended resources are integer and cannot be overcommitted, and devices cannot be shared between containers [21][54]. NVIDIA's toolkit generates a CDI spec (`/var/run/cdi/nvidia.yaml`) that CDI-aware runtimes read [55] | GPU count and model per node; driver and toolkit present; label GPU type; taint GPU nodes so that "only pods that request those features can be scheduled on those nodes" (via Node Feature Discovery) [21] |
| gVisor (`runsc --nvproxy`) | A proxy driver forwards allowed `ioctl`s to the host NVIDIA driver, with no emulation of the kernel driver; supported GPUs: T4, A100, A10G, L4, H100 (same-architecture cards "will likely work"); runs most CUDA workloads including PyTorch and LLMs [22] | Strict driver-version match: supported versions track GKE's, unsupported ones need a flag, unknown ones "will always fail to start"; supported device files, ioctls and platforms are selected; driver updates are part of the security plan [22] |
| Kata Containers (VFIO) | Whole-GPU passthrough ("not shared among VMs"), or NVIDIA vGPU (time-sliced or MIG-backed, a licensed product) [23] | Host needs VFIO kernel options, IOMMU on, large-BAR firmware settings ("Above 4G decoding"), the GPU bound to `vfio-pci` and an IOMMU group handed to the VM [23] |
| Cloud Hypervisor (VFIO) | "only supports assigning PCI devices"; the device must first be unbound from its native driver; every device in an IOMMU group goes together; GPUDirect P2P between passed-through NVIDIA GPUs supported on Turing, Ampere, Hopper and Lovelace [24] | IOMMU group topology, driver unbinding, P2P clique settings |
| Firecracker | Six emulated devices only (virtio-net, balloon, block, vsock, serial, a minimal keyboard controller) [25]. The 2019 reply that closed the GPU issue: passthrough "implies pinning physical memory which would remove our memory oversubscription", and one customer workload per physical GPU at a time, so "no known path to supporting GPUs" [56]. A 2024 pull request proposing PCIe community guidelines and a roadmap (closed unmerged on 2025-08-12) sets as its MVP the "ability to pass through and use a single physical GPU", aims at one GPU rather than multi-GPU P2P, and defers SR-IOV/vGPU passthrough and GPUDirect Storage until after the MVP [57]. VFIO tracking issue #5679 (opened 2026-02-11) is still open; a community pull request adding VFIO passthrough (#6245, opened 2026-09-28) was closed unmerged on 2026-09-30 with "We are already working on VFIO support" [26][27]; the main-branch changelog read on 2026-10-22 has no VFIO or GPU entry [28] | Treat Firecracker as having no GPU on mainline today; once VFIO lands: pinned guest memory (no oversubscription), no snapshot of an active passed-through device, no balloon [57][27] |
| Shards microVM | n/p in the public sources read; Hex reaches it through Shards' Engine API | Ask Shards for its device model before assuming either way |
| `process` box | Host GPU access is whatever the OS gives the process; not researched here | |

Three further points from the evidence:

- **Hardware generations matter for cost.** In Splitwise's characterization, token generation is
  memory-bound and does not need the newest GPU: A100 had "better or equal inference cost and energy
  overall compared to H100" in their runs, and the best clusters mixed GPU generations by phase [31] (MV).
- **Agent workloads mix resource affinities.** One agentic application combines GPU-bound LLMs,
  memory-bound retrieval and CPU-bound sandboxes; giving each sandbox 8 to 96 CPU cores moved the
  bottleneck between sandbox and LLM; agent-aware co-location improved results by up to 4.5x [6] (MV).
  Capability matching is not only "GPU or not": CPU cores for builds and tests are placement facts too.
- **An engine knob can flip the bottleneck**: raising SGLang's batch size from 1 to 4 moved decode time
  per token from 7 ms to 30 ms and made the LLM, not the desktop tool, the bottleneck [6] (MV).

## 7. Lessons for Hex

Each lesson is a recommendation (`DE`, a design input) with the evidence behind it and what would falsify
it. They are inputs to the scheduler design (CP-T7.4 plugins and scoring, CP-T7.5 quotas and fair share,
CP-T7.6 agents as workloads, CP-T7.9 autoscaling), not decisions: the decisions go in `docs/design/` with
Hex's own measurements.

### 7.1 Placement dimensions

| Dimension | Role | Evidence | Falsified if |
|---|---|---|---|
| **Hardware capability** (GPU model and count, driver window, CPU cores, memory, runtime class that can expose it) | **Hard filter.** A GPU workload must never land where the (node, class) cannot give it a GPU; the reverse (CPU work on GPU nodes) is a taint-style soft penalty | GPU is an integer, non-overcommittable resource [21]; gVisor needs a driver in a window [22]; Firecracker mainline exposes none [25][28]; sandboxes need up to 28 GB peaks and many cores [6] | A runtime class is found that exposes GPUs without a node-side device fact |
| **Agent identity and permissions** | **Hard filter** on which quota accounts, keys, workspaces, networks and tools an agent may use | Limits are per org, workspace, project or person; caches are per workspace; subscription limits are per person and shared across products [16][17][18][49][19] | Providers stop scoping limits and caches by account |
| **Provider, model, effort** | **Quota account selection** (hard: must have budget) and **cost/latency score** (soft) | Per-model-class buckets [16]; effort changes tokens per task and tool-call counts [51]; usage depends on model and effort [20] | Effort has no measurable effect on tokens or tool calls in Hex's own workloads |
| **Provider quota state** (RPM, ITPM, OTPM remaining and refill; ramp state; spend caps) | **Hard admission gate**, read from response headers and limits APIs, with the ramp rate as its own budget | Multi-dimensional token buckets, acceleration and `slow_down` limits [16][17]; uncoordinated agents fail at 72-100% under contention in a mock and 3 of 11 on a real key [13] | Provider limits become effectively unbounded for Hex's tenants |
| **Context window and token estimate** | **Filter** (the call must fit the model's window) and **score** (large estimated tokens consume more quota and KV) | Output length and size priors drive useful control: removing coarse magnitude inflates short-request P95 up to 5.8x [14]; Anthropic counts ITPM from an estimate corrected in flight [16] | Token estimates are too noisy to help (Zhihu's results degrade at 60% error) |
| **Session state locality** (warm KV on a hosted engine; provider-cache warmth; sandbox state) | **Soft score with an explicit price**, bounded by load | 88.9% to 29.0% reuse on a deployment change [7]; 8% hit after a model switch [2]; SMetric's stick-unless-SLO-breaks rule [12] | Cache reuse stops mattering (provider changes pricing or retention so a miss is cheap) |
| **Lifecycle phase** (running, waiting-for-tool, waiting-for-human, quiescent) | **Resource-holding policy**: what a waiting agent keeps (sandbox memory, KV, a quota slot) and when it is checkpointed or reclaimed | 92.3% of time human wait [5]; median session active 20% of lifetime [6]; cross-turn idle 243 s and 172 s [2]; 5.6% of retention time is inference [7] | Idle gaps become short (below the cost of suspend and resume) |
| **Priority and deadline class** | **Queue discipline**: interactive and short work ahead of batch, with anti-starvation | PLAS/ATLAS and anti-starvation [8]; Llumnix priorities [10]; Zhihu's DRR plus ordering [14]; Anthropic and Google offer batch and priority tiers [43][18] | Tail (p95/p99) is not improved by priority classes on Hex workloads |
| **Burst class** (fan-out width, retry state) | **Admission and gang decisions**: admit a fan-out whole or not at all; pace retry storms | Tail fan-out of 108 tools and retry amplification up to 4x [2]; arrival is agent-driven [2]; HiveMind's finding that retry is the most critical primitive, with admission alone leaving 81.8% failure in its ablation [13] | Fan-out stays shallow (median 2) and retries are rare at Hex scale |

### 7.2 The objective

The evidence supports one objective, stated as a recommendation: **maximize completed, useful agent
work that meets its stated deadline or class SLO, per unit of the scarce resources it consumes (provider
quota tokens, GPU-seconds, sandbox memory-seconds), subject to the hard filters above, with fairness
across tenants and bounded tail latency.** Concretely:

1. **Goodput, not utilization.** DistServe defines goodput as the rate served within the latency SLOs [29];
   Mooncake maximizes "effective throughput" with an early-rejection policy because overload is real [30];
   the Zhihu client-side scheduler counts "throughput counting only finished, SLO-meeting work" and notes
   that tail latency among completions "can improve for the wrong reason" if requests are dropped [14].
   Hex's metric must therefore count refused and abandoned work against itself.
2. **Program-level, non-clairvoyant.** Autellix shows a scheduler can use only cumulative service of
   completed calls, with no model of the future DAG, and still gain 4-15x [8]; SAGA adds a task-completion
   fairness measure and states the price (about 30% lower peak throughput) [11]. The `session` or `agent
   run` is the unit that carries priority, quota debit and affinity.
3. **Typed refusal over silent timeout.** Overload control that "replace[s] implicit timeout failures with
   explicit, objective-aligned shedding" [14] and Mooncake's early rejection [30] match Hex's rule that
   what cannot be done is refused with a reason (CLAUDE.md section 3). Pending agents should say whether
   they wait for quota, a GPU, a warm slot or a ramp window.
4. **Fairness across the hierarchy.** Provider quotas are shared across tenants under one key; a few
   clients dominate load (top 29 of 2,412 sent 90% of requests [3]) and a few users dominate tokens (a 50x
   spread across archetypes [2]). Per-tenant shares of quota tokens and GPU time need DRF-style accounting
   with borrowing, which maps onto CP-T7.5.
5. **Waiting is a state with a price.** The scheduler should treat "waiting" as a lifecycle state with
   predicted duration, reclaim sandbox or KV only across boundaries where the idle time repays the restore
   cost (Copilot: intra-turn gaps of seconds are too short, cross-turn gaps of minutes are not [2]), and
   expose the TTL math (provider cache TTL, sandbox suspend cost) as a score term.

### 7.3 What to measure before deciding

The decisions need Hex's own numbers (CLAUDE.md sections 5 and 7), not these: (a) the per-agent token and
request arrival process on Hex's recorded workloads (kiln CI legs and agent runs), including burst
width and retry rate; (b) sandbox active and idle fractions and working-set sizes under Hex's classes;
(c) provider 429 and 529 rates per dimension under admission control, with and without header-driven
pacing; (d) the cost of a cache miss per provider and the gap distribution against the TTLs; (e) GPU
node facts (driver, model, MIG state) and runtime-class support on the real fleet.

## 8. Open questions and gaps

- **No provider-side agent-fleet trace.** No source read gives the sub-second arrival process of agent
  fleets at the API, per-tenant burst structure, or sub-agent fan-out distributions across products; the
  traces are single-product and coding-heavy, and Anthropic says it cannot link API requests into sessions
  [1]. The 2026 studies are also recent and unreplicated.
- **Provider quota behaviour is under-specified.** Anthropic's "acceleration limits" are unquantified;
  OpenAI gives only a rule of thumb for ramp; Google publishes live limits only in AI Studio and says they
  are not guaranteed [16][17][18]. No source measures how limits behave under agent-shaped bursts.
- **Subscription windows.** Numeric allowances for Claude Pro/Max are not published on the pages read;
  OpenAI's Codex window could not be verified from a first-party page (HTTP 403); Google's consumer-plan
  limits were not researched.
- **Quota-aware scheduling is evidenced only weakly.** HiveMind and the Zhihu study use mock providers; the
  Stanford project is an unreviewed course project with small real-API runs [13][14][15]. No production
  or peer-reviewed evaluation of an admission scheduler against real Anthropic/OpenAI/Google quotas was
  found.
- **Hosted-model cache effects of placement.** Provider-side caches are keyed by org or workspace, not by
  Hex node; how much placement can improve hit rates beyond keeping an agent on one workspace and inside
  the TTL is unmeasured. OpenAI and Google prompt-cache semantics were not read.
- **Hardware.** Shards' device model is not public in the sources read. Non-NVIDIA accelerators (AMD,
  Intel, TPU), Apple Silicon GPUs on macOS and Windows hosts were not researched, though CLAUDE.md section
  12 requires a portable path or a typed refusal naming the platform. Firecracker's VFIO status is a
  moving target (open tracking issue; a closed PR); re-check before any design depends on it. MIG and
  vGPU slice scheduling were only touched through Kata's description of them.
- **Hex-specific unknowns.** How much of Hex's agent traffic will be hosted-API versus self-hosted models;
  whether sandbox suspend and resume (CP-T7.2 snapshots) are cheap enough to repay the 172-243 s median
  cross-turn idle times reported by Copilot; whether Hex's scale (a million agents in a cluster) hits
  provider quotas first or GPU capacity first.

## References

Accessed 2026-10-22 unless stated; every source was opened and read for this note. arXiv entries name
the version read.

[1] Anthropic, "Measuring AI agent autonomy in practice", research report, 18 Feb 2026. https://www.anthropic.com/research/measuring-agent-autonomy

[2] B. Liu, H. Qiu, Í. Goiri et al. (Microsoft), "Agentic Coding in the Wild: Characterizing GitHub Copilot at Production Scale", arXiv:2608.00101v1, 30 Jul 2026. https://arxiv.org/abs/2608.00101

[3] Y. Xiang et al. (Peking University, Alibaba), "ServeGen: Workload Characterization and Generation of Large Language Model Serving in Production", arXiv:2505.09999v3, 11 May 2026. https://arxiv.org/abs/2505.09999

[4] Y. Wang, Y. Chen et al. (HKUST (Guangzhou), Huawei), "BurstGPT: A Real-World Workload Dataset to Optimize LLM Serving Systems", arXiv:2401.17644v5, 26 May 2025. https://arxiv.org/abs/2401.17644

[5] K. Zhu et al. (University of Washington), "TraceLab: Characterizing Coding Agent Workloads for LLM Serving", arXiv:2606.30560v2, 30 Jun 2026. https://arxiv.org/abs/2606.30560

[6] C. Chang, Y. Zhou et al. (HKUST, Alibaba, ByteDance), "From LLM Inference to Agentic Workloads: Characterization and Implications for Serving Systems", arXiv:2608.15127v1, 15 Aug 2026. https://arxiv.org/abs/2608.15127

[7] M. Arya, A. Bari et al. (Microsoft M365 Research, IISc), "Understanding KV Residency in Production-Scale Agentic LLM Workloads", arXiv:2610.22863v1, 16 Oct 2026. https://arxiv.org/abs/2610.22863

[8] M. Luo et al. (UC Berkeley, Google DeepMind, SJTU), "Autellix: An Efficient Serving Engine for LLM Agents as General Programs", arXiv:2502.13965v1, 19 Feb 2025. https://arxiv.org/abs/2502.13965

[9] C. Lin et al. (SJTU, Microsoft Research), "Parrot: Efficient Serving of LLM-based Applications with Semantic Variable", arXiv:2405.19888v1, 30 May 2024. https://arxiv.org/abs/2405.19888

[10] B. Sun, Z. Huang et al. (Alibaba Group), "Llumnix: Dynamic Scheduling for Large Language Model Serving", arXiv:2406.03243v1, 5 Jun 2024. https://arxiv.org/abs/2406.03243

[11] D. Guo, J. Wu, S.-M. Yiu, "SAGA: Workflow-Atomic Scheduling for AI Agent Inference on GPU Clusters", arXiv:2605.00528v2, 19 Jun 2026. https://arxiv.org/abs/2605.00528

[12] J. Wang, K. Lin et al. (SJTU, Alibaba), "SMetric: Rethink LLM Scheduling for Serving Agents with Balanced Session-centric Scheduling", arXiv:2607.08565v2, 13 Sep 2026. https://arxiv.org/abs/2607.08565

[13] J. Owusu Agyemang et al., "HiveMind: OS-Inspired Scheduling for Concurrent LLM Agent Workloads", arXiv:2604.17111v1, 18 Apr 2026. https://arxiv.org/abs/2604.17111

[14] R. Yuan et al. (SotaLab, Zhihu Inc.), "Scheduling the Unschedulable: Taming Black-Box LLM Inference at Scale", arXiv:2604.06970v1, 8 Apr 2026. https://arxiv.org/abs/2604.06970

[15] B. Yalcinkaya, F. Henriksson, A. De, "DAG-Aware Request Scheduling for LLM Agent Workloads Under External Rate Limits", Stanford CS244c course project (unreviewed), winter 2026. https://www.scs.stanford.edu/26wi-cs244c/proj/dag_sched_agent.pdf

[16] Anthropic, "Rate limits", Claude Platform Docs. https://platform.claude.com/docs/en/api/rate-limits

[17] OpenAI, "Rate limits", OpenAI API docs. https://developers.openai.com/api/docs/guides/rate-limits

[18] Google, "Rate limits", Gemini API docs. https://ai.google.dev/gemini-api/docs/rate-limits

[19] Anthropic, "Use Claude Code with your Pro or Max plan", Claude Help Center, article 11145838. https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-or-max-plan

[20] Anthropic, "How do usage and length limits work?", Claude Help Center, article 11647753. https://support.claude.com/en/articles/11647753-how-do-usage-and-length-limits-work

[21] Kubernetes, "Schedule GPUs", Kubernetes documentation. https://kubernetes.io/docs/tasks/manage-gpus/scheduling-gpus/

[22] gVisor, "GPU Support", gVisor documentation. https://gvisor.dev/docs/user_guide/gpu/

[23] Kata Containers, "Using NVIDIA GPU device with Kata Containers", docs/use-cases/NVIDIA-GPU-passthrough-and-Kata.md, main branch. https://github.com/kata-containers/kata-containers/blob/main/docs/use-cases/NVIDIA-GPU-passthrough-and-Kata.md

[24] Cloud Hypervisor, "Cloud Hypervisor VFIO HOWTO", docs/vfio.md, main branch. https://github.com/cloud-hypervisor/cloud-hypervisor/blob/main/docs/vfio.md

[25] Firecracker, "Firecracker Frequently Asked Questions", FAQ.md, main branch. https://github.com/firecracker-microvm/firecracker/blob/main/FAQ.md

[26] Firecracker issue #5679, "[Feature] VFIO device passthrough support" (opened 2026-02-11, open on 2026-10-22). https://github.com/firecracker-microvm/firecracker/issues/5679

[27] Firecracker pull request #6245, "Add PCI device passthrough with VFIO" (opened 2026-09-28, closed unmerged 2026-09-30, closing comment linking #5679). https://github.com/firecracker-microvm/firecracker/pull/6245

[28] Firecracker, CHANGELOG.md, main branch, read 2026-10-22. https://github.com/firecracker-microvm/firecracker/blob/main/CHANGELOG.md

[29] Peking University, StepFun and UC San Diego, "DistServe: Disaggregating Prefill and Decoding for Goodput-optimized Large Language Model Serving", arXiv:2401.09670v3, 6 Jun 2024. https://arxiv.org/abs/2401.09670

[30] R. Qin, Z. Li, W. He et al., "Mooncake: A KVCache-centric Disaggregated Architecture for LLM Serving", arXiv:2407.00079v5, 8 Oct 2026. https://arxiv.org/abs/2407.00079

[31] P. Patel et al. (University of Washington, Microsoft), "Splitwise: Efficient Generative LLM Inference Using Phase Splitting", arXiv:2311.18677v2, 20 May 2024. https://arxiv.org/abs/2311.18677

[32] Y. Yuan, A. Nayak, S. Kundu, N. Talati (UIUC, Intel), "Agentic AI Workload Characterization", arXiv:2605.26297v2, 21 Sep 2026. https://arxiv.org/abs/2605.26297

[33] V. Srivatsa, Z. He et al. (UC San Diego), "Preble: Efficient Distributed Prompt Scheduling for LLM Serving", arXiv:2407.00023v2, 3 Oct 2024. https://arxiv.org/abs/2407.00023

[34] Microsoft M365 Research and Microsoft Research India, "Sutradhara: An Intelligent Orchestrator-Engine Co-design for Tool-based Agentic Inference", arXiv:2601.12967v3, 22 Apr 2026. https://arxiv.org/abs/2601.12967

[35] R. Abhyankar, Z. He et al. (UC San Diego), "InferCept: Efficient Intercept Support for Augmented Large Language Model Inference", ICML 2024, arXiv:2402.01869v2, 30 May 2024. https://arxiv.org/abs/2402.01869

[36] W. Kwon et al. (UC Berkeley et al.), "Efficient Memory Management for Large Language Model Serving with PagedAttention", SOSP '23, arXiv:2309.06180v1, 12 Sep 2023. https://arxiv.org/abs/2309.06180

[37] A. Agrawal et al. (Microsoft Research India, Georgia Tech), "Taming Throughput-Latency Tradeoff in LLM Inference with Sarathi-Serve", arXiv:2403.02310v3, 17 Jun 2024. https://arxiv.org/abs/2403.02310

[38] L. Zheng et al., "SGLang: Efficient Execution of Structured Language Model Programs", arXiv:2312.07104v2, 6 Jun 2024. https://arxiv.org/abs/2312.07104

[39] J. Liu et al. (University of Michigan), "Andes: Defining and Enhancing Quality-of-Experience in LLM-Based Text Streaming Services", arXiv:2404.16283v2, 13 Dec 2024. https://arxiv.org/abs/2404.16283

[40] H. Li, R. He, Q. Mang et al., "Continuum: Efficient and Robust Multi-Turn LLM Agent Scheduling with KV Cache Time-to-Live", arXiv:2511.02230v7, 8 Sep 2026. https://arxiv.org/abs/2511.02230

[41] N. Wadlom, J. Shen, Y. Lu (National University of Singapore), "Efficient LLM Serving for Agentic Workflows: A Data Systems Perspective (Extended)" (the Helium system), arXiv:2603.16104v1, 17 Mar 2026. https://arxiv.org/abs/2603.16104

[42] Anthropic, "Claude API errors", Claude Platform Docs. https://platform.claude.com/docs/en/api/errors

[43] Anthropic, "Service tiers", Claude Platform Docs. https://platform.claude.com/docs/en/api/service-tiers

[44] LiteLLM, "Router - Load Balancing", LiteLLM docs. https://docs.litellm.ai/docs/routing

[45] OpenRouter, "Provider Routing", OpenRouter docs. https://openrouter.ai/docs/features/provider-routing

[46] Portkey, "Load Balancing", Portkey AI Gateway docs. https://portkey.ai/docs/product/ai-gateway/load-balancing

[47] Portkey, "Fallbacks", Portkey AI Gateway docs. https://portkey.ai/docs/product/ai-gateway/fallbacks

[48] Portkey, "Conditional Routing", Portkey AI Gateway docs. https://portkey.ai/docs/product/ai-gateway/conditional-routing

[49] Anthropic, "Prompt caching", Claude Platform Docs. https://platform.claude.com/docs/en/build-with-claude/prompt-caching

[50] Kubernetes SIG Network, "Introduction", Gateway API Inference Extension. https://gateway-api-inference-extension.sigs.k8s.io/

[51] Anthropic, "Effort", Claude Platform Docs. https://platform.claude.com/docs/en/build-with-claude/effort

[52] I. Ong, A. Almahairi et al., "RouteLLM: Learning to Route LLMs with Preference Data", ICLR 2025, arXiv:2406.18665v4, 23 Feb 2025. https://arxiv.org/abs/2406.18665

[53] L. Chen, M. Zaharia, J. Zou (Stanford), "FrugalGPT: How to Use Large Language Models While Reducing Cost and Improving Performance", arXiv:2305.05176v1, 9 May 2023. https://arxiv.org/abs/2305.05176

[54] Kubernetes, "Device Plugins", Kubernetes documentation. https://kubernetes.io/docs/concepts/extend-kubernetes/compute-storage-net/device-plugins/

[55] NVIDIA, "Support for Container Device Interface", NVIDIA Container Toolkit documentation. https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/cdi-support.html

[56] Firecracker issue #849, "GPU Support" (opened 2019-01-14, closed 2019-02-18), closing reply of 2019-02-18. https://github.com/firecracker-microvm/firecracker/issues/849#issuecomment-464731628

[57] Firecracker pull request #4894, "[PCIe] Community Guidelines and Roadmap" (opened 2024-11-06, closed unmerged 2025-08-12). https://github.com/firecracker-microvm/firecracker/pull/4894
