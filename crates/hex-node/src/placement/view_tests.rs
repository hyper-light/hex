//! The node's capacity view (joint plan step 13; AC-T7.4.1): its shell over the node's record files held
//! to the conformance suite, and the node's own admission and a holder's reserve checking one view.

use std::collections::VecDeque;
use std::sync::Arc;

use hex_api::capacity::{
    Amounts, CapacityView, Event, Holder, Host, Out, ToHolder, ToKeeper, Why, conformance,
};
use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Refusal, Runtime};
use hex_sim::index::Pick;
use hex_sim::model::{Ask, Class, Demand, Kind, NodeId, NodeSpec, Resources};
use hex_sim::peer::{Budget, Stop};

use super::View;
use crate::placement::{Driver, Notice, Payload, Runtimes, Step};

const HOLDERS: usize = 2;
const RAN: usize = 8;

fn refusal(stop: Stop) -> Refusal {
    Refusal::Broken(format!("{stop:?}"))
}

fn spec(room: Amounts) -> NodeSpec {
    NodeSpec::new(
        NodeId(0),
        Resources {
            cpu_milli: room.cpu_milli,
            mem_bytes: room.mem_bytes,
            disk_bytes: room.disk_bytes,
            gpus: u32::try_from(room.gpus).unwrap(),
        },
    )
}

fn cpu(milli: u64) -> Amounts {
    Amounts {
        cpu_milli: milli,
        ..Amounts::ZERO
    }
}

/// The node's view as the conformance suite drives it: its records a folder of their own on disk, the
/// node's own held set by the suite.
struct NodeHost {
    dir: tempfile::TempDir,
    view: View,
    local: Amounts,
    room: Amounts,
}

impl NodeHost {
    fn new() -> Result<Self, Refusal> {
        let dir = tempfile::tempdir().map_err(|e| Refusal::Broken(e.to_string()))?;
        let mut view = View::open(dir.path(), HOLDERS, RAN).map_err(refusal)?;
        view.refresh(&spec(conformance::ROOM)).map_err(refusal)?;
        Ok(Self {
            dir,
            view,
            local: Amounts::ZERO,
            room: conformance::ROOM,
        })
    }
}

impl CapacityView for NodeHost {
    fn hear(&mut self, link: u64, msg: ToKeeper) -> Result<ToHolder, Refusal> {
        self.view
            .hear(link, msg, self.local)
            .map(|(answer, _)| answer)
            .map_err(refusal)
    }

    fn link_down(&mut self, link: u64) -> Result<(), Refusal> {
        self.view.link_down(link);
        Ok(())
    }

    fn hints(&mut self) -> Result<Vec<(u64, ToHolder)>, Refusal> {
        self.view.hints(self.local).map_err(refusal)
    }
}

impl Host for NodeHost {
    fn set_room(&mut self, room: Amounts) -> Result<(), Refusal> {
        self.room = room;
        self.view.refresh(&spec(room)).map(drop).map_err(refusal)
    }

    fn set_local(&mut self, local: Amounts) -> Result<(), Refusal> {
        self.local = local;
        Ok(())
    }

    fn restart(&mut self) -> Result<(), Refusal> {
        self.view = View::open(self.dir.path(), HOLDERS, RAN).map_err(refusal)?;
        self.view
            .refresh(&spec(self.room))
            .map(drop)
            .map_err(refusal)
    }
}

/// AC-T7.4.1, CLAUDE.md §9: run the capacity view's conformance suite against the node's view over its
/// record files, restarted from them; expect every case to pass.
#[test]
fn the_nodes_view_passes_the_conformance_suite() {
    let mut make = || -> Result<Box<dyn Host>, Refusal> { Ok(Box::new(NodeHost::new()?)) };
    let failures = conformance::view(&mut make);
    assert!(failures.is_empty(), "{failures:#?}");
}

fn demand_of(cpu_milli: u64) -> (Demand, Payload) {
    let need = Resources {
        cpu_milli,
        mem_bytes: hex_api::MIN_MEMORY,
        ..Resources::default()
    };
    let demand = Demand {
        need,
        kind: Kind::Plain,
        class: Class::Oci,
        ask: Ask::default(),
    };
    let payload = Payload {
        spec: BoxSpec {
            route: None,
            allocation: None,
            devices: Vec::new(),
            id: BoxId::new("view-box").unwrap(),
            bounds: Bounds {
                cpu_max_milli: Some(u32::try_from(cpu_milli).unwrap()),
                memory_max: Some(need.mem_bytes),
                ..Bounds::default()
            },
            class: ClassSpec::Process,
            owner: None,
        },
        start: false,
    };
    (demand, payload)
}

/// Delivers a step's messages to this one node until it has nothing more to say: its notices.
fn quiesce(driver: &Driver, step: Step) -> Vec<Notice> {
    let mut send = VecDeque::from(step.send);
    let mut notices = step.notices;
    while let Some((to, envelope)) = send.pop_front() {
        assert_eq!(to, NodeId(0));
        let next = driver.hear(NodeId(0), envelope).unwrap();
        send.extend(next.send);
        notices.extend(next.notices);
        assert!(notices.len() < 64, "the node did not quiesce");
    }
    notices
}

fn started(notices: &[Notice]) -> bool {
    notices.iter().any(|n| matches!(n, Notice::Started { .. }))
}

/// Says `msgs` to the node's keeper on link 1, and its answers to `holder`, until neither has more to say.
fn talk(driver: &Driver, holder: &mut Holder, msgs: Vec<ToKeeper>) -> Vec<Event> {
    let mut queue = VecDeque::from(msgs);
    let mut events = Vec::new();
    while let Some(msg) = queue.pop_front() {
        let answer = driver.view_hear(1, msg).unwrap();
        let mut out = Out::default();
        holder.hear(answer, &mut out);
        queue.extend(out.send);
        events.extend(out.events);
    }
    events
}

/// AC-T7.4.1, CV3: a holder reserves on a node before and after its facts give the view its room, the
/// node places a box of its own beside the reservation, then after it ends, and the holder asks again
/// beside the node's box; expect nothing admitted before the room is known, Hex's box refused while the
/// holder's reservation leaves too little and started once it is released, the holder then refused for
/// the room Hex's box holds with the view saying what is free, and the holder owed a hint of Hex's box.
#[test]
fn the_nodes_admission_and_a_holders_reserve_check_one_view() {
    let dir = tempfile::tempdir().unwrap();
    let room = Resources {
        cpu_milli: 2000,
        mem_bytes: 8 * hex_api::MIN_MEMORY,
        ..Resources::default()
    };
    let runtime: Arc<dyn Runtime> = Arc::new(hex_api::sim::SimRuntime::default());
    let driver = Driver::boot(
        NodeId(0),
        &[NodeSpec::new(NodeId(0), room)],
        &[],
        Budget { owned: 4, ran: 4 },
        Pick::default(),
        dir.path(),
        Runtimes::new([(Class::Oci, runtime)]).unwrap(),
    )
    .unwrap();
    driver.open_view(&dir.path().join("capacity"), 1).unwrap();
    let mut kiln = Holder::start("kiln-runner".into(), 1, 4, &[]).unwrap();
    let hello = kiln.connect().unwrap();
    let joined = talk(&driver, &mut kiln, vec![hello]);
    assert!(
        matches!(joined.as_slice(), [Event::Synced { agree: true, .. }]),
        "{joined:?}"
    );
    let (early, msg) = kiln.reserve(cpu(1)).unwrap();
    assert_eq!(
        talk(&driver, &mut kiln, vec![msg]),
        vec![Event::Refused(early, Why::Room)]
    );

    driver.refresh_node(NodeSpec::new(NodeId(0), room)).unwrap();
    let (run, msg) = kiln.reserve(cpu(1500)).unwrap();
    assert_eq!(
        talk(&driver, &mut kiln, vec![msg]),
        vec![Event::Granted(run)]
    );
    let (d, p) = demand_of(1000);
    let (_, step) = driver.submit(d, p).unwrap();
    assert!(!started(&quiesce(&driver, step)));

    let mut out = Out::default();
    kiln.release(run, &mut out);
    assert_eq!(
        talk(&driver, &mut kiln, out.send),
        vec![Event::Released(run)]
    );
    assert!(started(&quiesce(&driver, driver.retry().unwrap())));

    let hints = driver.view_hints().unwrap();
    assert_eq!(hints.len(), 1, "{hints:?}");
    let mut out = Out::default();
    for (_, hint) in hints {
        kiln.hear(hint, &mut out);
    }
    let (late, msg) = kiln.reserve(cpu(1500)).unwrap();
    assert_eq!(
        talk(&driver, &mut kiln, vec![msg]),
        vec![Event::Refused(late, Why::Room)]
    );
    assert_eq!(kiln.view().unwrap().free().cpu_milli, 1000);
    assert_eq!(driver.view().unwrap().held.cpu_milli, 1000);
}
