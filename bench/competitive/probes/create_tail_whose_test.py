#!/usr/bin/env python3
"""create_tail_whose.bt and create_tail_whose.sh, read as text (goal 16; the lead, 2026-11-09).
Run: python3 -B -I bench/competitive/probes/create_tail_whose_test.py

The record host hides kernel addresses (adacachy2: kernel.kptr_restrict=2, so /proc/kallsyms reads zero even to root,
and bpftrace's kaddr() failed there, whose-noowner-18eb3979/bt.log). So the bt names nothing by address and prints
none; it tells a preemption by the switch's state bits; a dying task's last switch leaves nothing in its maps; and its
wrapper holds the timed lock from before bpftrace attaches, since attaching patches kernel text on every CPU.
"""
import os
import re
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))
# What a bt reads that is a kernel address: a lock's, a work item's and its function's, and a lock's owner word.
POINTERS = re.compile(r"args\.lock_addr|args\.work\b|args\.function|args\.sem\b|\bowner\b|\bretval\b|args\.s\b|args\.lru\b")
AGGREGATIONS = re.compile(r"^(count|sum|hist|lhist|max|min|avg|stats)\(")
WRITE = re.compile(r"(@\w+|\$\w+)\s*((?:\[(?:[^\[\]]|\[[^\[\]]*\])*\])?)\s*(\+\+|--|[-+|&]?=(?!=))\s*([^;{}]*)")


def read(name):
    with open(os.path.join(HERE, name)) as f:
        return f.read()


def code(bt):
    """The bt without its comments."""
    return re.sub(r"//[^\n]*", "", bt)


def unindexed(expr):
    """An expression with each map read's index taken out: a map read is the map's value, whatever indexes it."""
    while True:
        out = re.sub(r"\[[^\[\]]*\]", "", expr)
        if out == expr:
            return out
        expr = out


def value_of(expr):
    """An expression with each dereference to a member that isn't a pointer read as a plain value."""
    return re.sub(r"(?:\$\w+|\))\s*->\s*(\w+)", lambda m: m.group(0) if POINTERS.search(m.group(1)) else "VALUE", expr)


def writes(bt):
    """Each write of a map or a variable: (name, its index, its operator, what is written)."""
    return [(m.group(1), m.group(2), m.group(3), m.group(4).strip()) for m in WRITE.finditer(code(bt))]


def holding_addresses(bt):
    """The maps and variables some write gives a kernel address as their value (to a fixed point)."""
    held = set()
    while True:
        more = {name for name, _, op, value in writes(bt) if op not in ("++", "--")
                and (POINTERS.search(unindexed(value_of(value))) or any(re.search(re.escape(h) + r"\b", unindexed(value_of(value))) for h in held))}
        if more <= held:
            return held
        held |= more


def end_block(bt):
    m = re.search(r"\bEND\s*\{", code(bt))
    if not m:
        return ""
    depth, i = 1, m.end()
    while depth and i < len(code(bt)):
        depth += {"{": 1, "}": -1}.get(code(bt)[i], 0)
        i += 1
    return code(bt)[m.end():i]


class TheBt(unittest.TestCase):
    def setUp(self):
        self.bt = read("create_tail_whose.bt")
        self.code = code(self.bt)

    def test_it_names_nothing_by_a_kernel_address(self):
        # kaddr() and ksym() read /proc/kallsyms in bpftrace's own process, and kstack's frames are named from it too.
        for call in ("kaddr(", "ksym(", "kstack"):
            self.assertFalse(call in self.code, f"{call} needs the addresses the host hides")

    def test_it_prints_no_kernel_address(self):
        # bpftrace prints every map at its end: each that holds state is cleared, and no aggregation is keyed by an address.
        aggregated = {name for name, _, _, value in writes(self.bt) if name.startswith("@") and AGGREGATIONS.match(value)}
        state = {name for name, _, _, _ in writes(self.bt) if name.startswith("@")} - aggregated
        cleared = set(re.findall(r"clear\((@\w+)\)", end_block(self.bt)))
        self.assertTrue(state, "a bt with no state map isn't this one")
        self.assertEqual(state - cleared, set(), "state maps its END doesn't clear")
        held = holding_addresses(self.bt)
        for name, index, _, value in writes(self.bt):
            if name in aggregated:
                for key in index[1:-1].split(","):
                    key = unindexed(value_of(key))
                    self.assertFalse(POINTERS.search(key) or any(re.search(re.escape(h) + r"\b", key) for h in held),
                                     f"{name}{index} is keyed by a kernel address")

    def test_it_tells_a_preemption_by_the_switchs_state_bits(self):
        # A preempted task's prev_state is TASK_REPORT_MAX, 0x100 ("R+"), not 0: its state bits are the low eight
        # (sched_switch's format: prev_state & (TASK_REPORT_MAX - 1)). Compared to 0 whole, only a yield matched.
        self.assertIsNone(re.search(r"prev_state\s*==", self.code), "prev_state compared whole")
        self.assertIsNotNone(re.search(r"prev_state\s*&\s*0xff\b", self.code), "prev_state's state bits not taken")

    def test_no_string_variable_is_given_a_second_string(self):
        # bpftrace copies a string to its end, so a variable given a shorter one keeps the old one's tail: a comm folded
        # to its kind printed as one key many times over (whose-noowner-1f705e80-r2, @preempted_by's 3,781 keys). Names
        # go into keys straight from their maps and the comms themselves, never through a variable.
        given = re.findall(r"(\$\w+)\s*=\s*(\"|comm\b|args\.\w*comm|@kname\[|@pname\[)", self.code)
        self.assertEqual(given, [], "variables given a comm or a name")

    def test_it_names_the_rest_of_what_earlier_boxes_leave_the_host(self):
        # The workqueue "events" ran 14.0 s on the create CPUs, its function unnamed (whose-noowner-1f705e80-r2).
        for probe in ("kernfs_notify_workfn", "delayed_mntput", "cleanup_mnt", "cleanup_net"):
            self.assertIn(f"fentry:vmlinux:{probe}", self.code, probe)

    def test_it_counts_superblocks_and_memcg_list_lrus_alive_by_type_and_maker(self):
        # Lever (b): a memcg's offline walks every memcg-aware list_lru on the host under list_lrus_mutex
        # (mm/list_lru.c, memcg_reparent_list_lrus), two a superblock, so what it costs follows how many are alive, and
        # whose. The census at the wrapper's start and end missed every box (whose-noowner-1f705e80-r3: 1 cgroup2
        # superblock, the node gone at both). A new superblock is sget_fc's while its root isn't made yet; one ends
        # where deactivate_locked_super drops its last active reference.
        for probe in ("fexit:vmlinux:sget_fc", "fentry:vmlinux:deactivate_locked_super",
                      "fexit:vmlinux:__list_lru_init", "fentry:vmlinux:list_lru_destroy"):
            self.assertIn(probe, self.code, probe)
        self.assertRegex(self.code, r"retval->s_root\s*==\s*0", "a superblock sget_fc found made counted as new")
        self.assertRegex(self.code, r"s_active\.counter\s*==\s*1", "a superblock counted ended before its last put")
        # Counted exactly: bpftrace's ++ on a map is a read and a write, which CPUs making superblocks at once undo
        # (thousands a second), where sum() is per CPU. Alive at each second, so the most alive at once is read off.
        key = r"\[(?:[^\[\]]|\[(?:[^\[\]]|\[[^\[\]]*\])*\])*\]"
        for alive in ("@sb_alive", "@lru_alive"):
            self.assertRegex(self.code, re.escape(alive) + key + r"\s*=\s*sum\(1\)", f"{alive} made")
            self.assertRegex(self.code, re.escape(alive) + key + r"\s*=\s*sum\(-1\)", f"{alive} ended")
            self.assertRegex(self.code, r"interval:s:1\s*\{[^}]*print\(" + re.escape(alive) + r"\)", f"{alive} each second")
        self.assertNotRegex(self.code, r"@(sb|lru)_alive" + key + r"\s*(\+\+|--)", "an alive count by ++ or --")

    def test_it_times_the_list_lru_walk_within_a_memcgs_offline(self):
        self.assertIn("fentry:vmlinux:memcg_reparent_list_lrus", self.code)
        self.assertIn("fexit:vmlinux:memcg_reparent_list_lrus", self.code)

    def test_it_names_the_events_workqueues_candidates(self):
        # "events" ran 17.0 s on the create CPUs, its function unnamed and not kernfs notify's (r3). Each candidate is
        # a work function that a box's end queues there, named as it runs (its workqueue_execute_start's function):
        # those adacachy2's 7.3 kernel traces (bpftrace -l: proc_cleanup_work, drain_local_stock, rht_deferred_worker,
        # vmpressure_work_fn and free_work aren't there to trace).
        for probe in ("destroy_super_work", "free_ipc", "drain_local_memcg_stock", "drain_local_obj_stock",
                      "pcpu_balance_workfn", "delayed_fput", "high_work_func", "fqdir_work_fn", "fqdir_free_fn"):
            self.assertRegex(self.code, rf"fentry:vmlinux:{probe}\s*/\s*@wf\[tid\]\s*/", probe)
        # A candidate missing from this kernel (inlined, renamed) is warned of, not fatal to the record.
        self.assertRegex(self.code, r"config\s*=\s*\{[^}]*missing_probes\s*=\s*\"warn\"")

    def test_a_dying_tasks_last_switch_leaves_nothing_behind(self):
        # A task's last switch reports X (0x10) or Z (0x20) and it never runs again: what it left would stay till the end.
        self.assertIsNotNone(re.search(r"&\s*0x30\)\s*==\s*0", self.code), "a switch out of X or Z is kept")


class TheWrapper(unittest.TestCase):
    def test_it_holds_the_timed_lock_from_before_bpftrace_attaches(self):
        lines = read("create_tail_whose.sh").splitlines()
        lock = [i for i, l in enumerate(lines) if re.search(r"\bflock\b", l) and not l.lstrip().startswith("#")]
        attach = [i for i, l in enumerate(lines) if "docker run -d" in l]
        self.assertTrue(lock and attach)
        self.assertLess(lock[0], attach[0], "bpftrace attaches before the timed lock is held")
        self.assertEqual(len(lock), 1, "the lock is taken once, around everything timed")

    def test_it_records_the_hosts_superblocks_by_type_and_owner_at_its_start_and_end(self):
        sh = read("create_tail_whose.sh")
        for when in ("start", "end"):
            self.assertRegex(sh, rf"(?m)^superblocks {when}$", f"the superblocks at its {when}")
        self.assertIn('"$O/superblocks-$1.txt"', sh)

    def test_it_runs_its_siblings_from_any_directory(self):
        # The lead's run of a8e243f6 from the tree's root: "bench/competitive/probes/create_tail.sh: No such file or
        # directory", the wrapper having changed to bench/ first. Its siblings are named by its own directory.
        code = [l for l in read("create_tail_whose.sh").splitlines() if not l.lstrip().startswith("#")]
        relative = [l.strip() for l in code if "bench/competitive/probes/" in l or "cd " in l]
        self.assertEqual(relative, [], "lines that name a sibling from a working directory")
        for sibling in ("create_tail.sh", "create_tail_attr.py"):
            self.assertTrue(any(f'"$HERE/{sibling}"' in l for l in code), f"{sibling} named by $HERE")


if __name__ == "__main__":
    unittest.main()
