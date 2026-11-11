# Original configured-observer CPU isolation control

CP-T7.4 / AC-T7.4.1. Source first, implementation lane; no production remedy.
The previous minimal typed observation compiler correction is retained rather than
silently dropping its failure. This control asks whether an independently initialized
native CPU plane can still refresh and make progress when a configured Firecracker GPU
observation fails. It is a hypothesis until ROOT executes the exact control.

The test calls the real `Service::refresh_snapshot`, `runtime_facts`, `Runtimes`
normalization, and file-backed Driver. Its existing CPU SimRuntime is the sole synthetic
native authority. Both endpoints forward that same empty report while healthy. The FC
fixture carries only a Prepared provisioning identity and explicit synthetic test cost;
it creates no DeviceInfo/model/VRAM/load or native provider. After healthy normalization,
the configured observer returns typed Backing/Uncertain for capacity and report queries.
The required result is fresh CPU physical creation followed by deletion, an empty GPU fact
list, a retained diagnostic, and the same native authority. Merely tracking a pending
workload is not CPU progress. Both observation results are captured before assertions.

One new Linux-only test is registered; the existing Service test helper gains sibling
visibility only. No production behavior, SDK, provider, inventory or ownership table is
changed. There is no dispatch, new wait, cache, retry or timing constant. Synthetic resource
numbers are fixture inputs, not measured runtime budgets or capacity claims.

R01/R04/R06/R07/R13: test-only assertions/allocation and one explicit injected state;
production bodies unchanged. R02/R03/R05/R12: existing real seams and the user-authorized
bounded control, no new architecture; counters/models are not invented. R09/R10: original
control sealed before any remedy; actual red/green/guard mutation remain unrun. R08/R14:
no performance claim or profiling; coordinator owns them. R11: actual FC host seam is
Linux; zero selected cases on another platform is not a pass. R15/R16: no test/native
job started, named signed source only; private fixture state cleans via existing test RAII.

ROOT must execute the exact Linux control after coherent source intake and retain source,
command, selected count, log and cleanup evidence. Compilation, actual outcome and native
acceptance are unverified. Do not repeat unrelated previously passing controls.
