//! A server upgrade rolling across the pods, one at a time: a pure decision from the rollout, who is live, and what
//! each pod reports. The leader applies each decision (`ReleaseManager::advance_rollout`).

use semver::Version;
use speedyspec_server_domain::{MemberReport, Rollout};

/// What the leader does next in a rollout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RolloutStep {
    /// It is this pod's turn to switch its server binary.
    Turn(String),
    /// The pod whose turn it is has not finished its trial yet.
    Wait,
    /// Every live pod runs the target, confirmed.
    Done,
    /// A pod's trial of the target failed: the rollout reverts.
    Failed { pod: String, reason: String },
    /// Every pod that had upgraded runs the previous version again.
    Reverted,
}

/// The upgrade order: the pods by name, with the leader last, so it keeps coordinating until the end.
pub fn leader_last(live: &[String], leader: &str) -> Vec<String> {
    let mut order: Vec<String> = live.iter().filter(|pod| *pod != leader).cloned().collect();
    order.sort();
    if live.iter().any(|pod| pod == leader) {
        order.push(leader.to_string());
    }
    order
}

/// Whether `pod` runs `version`, confirmed, with no trial running.
pub(crate) fn finished(members: &[MemberReport], pod: &str, version: &Version) -> bool {
    members.iter().any(|report| {
        report.pod == pod
            && report.server.running.as_ref() == Some(version)
            && report.server.confirmed.as_ref() == Some(version)
            && report.server.trial.is_none()
    })
}

/// The next step of `rollout`, given the live pods and every pod's report. Pods that are not live are skipped and
/// not counted as done; live pods missing from `order` (late joiners) go at its end.
pub fn next_step(rollout: &Rollout, live: &[String], members: &[MemberReport]) -> RolloutStep {
    let is_live = |pod: &String| live.contains(pod);
    if rollout.reverting {
        let back = &rollout.previous;
        if let Some(turn) = rollout.turn.as_ref().filter(|t| is_live(t))
            && rollout.done.contains(turn)
            && !finished(members, turn, back)
        {
            return RolloutStep::Wait;
        }
        return match rollout
            .done
            .iter()
            .rev()
            .find(|pod| is_live(pod) && !finished(members, pod, back))
        {
            Some(pod) => RolloutStep::Turn(pod.clone()),
            None => RolloutStep::Reverted,
        };
    }
    let mut order = rollout.order.clone();
    let mut joiners: Vec<String> = live
        .iter()
        .filter(|pod| !order.contains(pod))
        .cloned()
        .collect();
    joiners.sort();
    order.extend(joiners);
    for pod in order.iter().filter(|pod| is_live(pod)) {
        // Only a failure recorded during this rollout counts (an earlier rollout of the same version may have
        // failed, and its record stays in the pod's member file).
        if let Some(report) = members.iter().find(|r| r.pod == *pod)
            && let Some(reason) = report.server.failed.get(&rollout.target)
            && (report.server.failed_in.get(&rollout.target) == Some(&rollout.id)
                || (rollout.id == 0 && !report.server.failed_in.contains_key(&rollout.target)))
        {
            return RolloutStep::Failed {
                pod: pod.clone(),
                reason: reason.clone(),
            };
        }
    }
    let target = &rollout.target;
    if let Some(turn) = rollout.turn.as_ref().filter(|t| is_live(t))
        && !finished(members, turn, target)
    {
        return RolloutStep::Wait;
    }
    match order
        .iter()
        .find(|pod| is_live(pod) && !rollout.done.contains(pod) && !finished(members, pod, target))
    {
        Some(pod) => RolloutStep::Turn(pod.clone()),
        None => RolloutStep::Done,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use speedyspec_server_domain::PodServer;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn rollout(order: &[&str]) -> Rollout {
        Rollout {
            id: 0,
            target: v("0.2.0"),
            previous: v("0.1.0"),
            group: None,
            order: order.iter().map(|p| p.to_string()).collect(),
            done: Vec::new(),
            turn: None,
            reverting: false,
            failure: None,
        }
    }

    /// A pod running `version`, confirmed, or on trial of it.
    fn pod(name: &str, version: &str, trial: bool) -> MemberReport {
        let mut server = PodServer {
            running: Some(v(version)),
            ..Default::default()
        };
        if trial {
            server.trial = Some(v(version));
            server.confirmed = Some(v("0.1.0"));
        } else {
            server.confirmed = Some(v(version));
        }
        MemberReport {
            pod: name.to_string(),
            server,
            ..Default::default()
        }
    }

    fn live(pods: &[&str]) -> Vec<String> {
        pods.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn turns_in_name_order_leader_last() {
        assert_eq!(
            leader_last(&live(&["c", "a", "b"]), "a"),
            live(&["b", "c", "a"])
        );
        let plan = rollout(&["b", "c", "a"]);
        let members = [
            pod("a", "0.1.0", false),
            pod("b", "0.1.0", false),
            pod("c", "0.1.0", false),
        ];
        assert_eq!(
            next_step(&plan, &live(&["a", "b", "c"]), &members),
            RolloutStep::Turn("b".to_string())
        );
    }

    #[test]
    fn waits_during_a_trial() {
        let mut plan = rollout(&["b", "c", "a"]);
        plan.turn = Some("b".to_string());
        let members = [
            pod("a", "0.1.0", false),
            pod("b", "0.2.0", true),
            pod("c", "0.1.0", false),
        ];
        assert_eq!(
            next_step(&plan, &live(&["a", "b", "c"]), &members),
            RolloutStep::Wait
        );
        let members = [
            pod("a", "0.1.0", false),
            pod("b", "0.2.0", false),
            pod("c", "0.1.0", false),
        ];
        assert_eq!(
            next_step(&plan, &live(&["a", "b", "c"]), &members),
            RolloutStep::Turn("c".to_string()),
            "b confirmed, so c is next"
        );
    }

    #[test]
    fn done_when_all_confirmed() {
        let mut plan = rollout(&["b", "c", "a"]);
        plan.turn = Some("a".to_string());
        plan.done = live(&["b", "c"]);
        let members = [
            pod("a", "0.2.0", false),
            pod("b", "0.2.0", false),
            pod("c", "0.2.0", false),
        ];
        assert_eq!(
            next_step(&plan, &live(&["a", "b", "c"]), &members),
            RolloutStep::Done
        );
    }

    #[test]
    fn failed_trial_starts_revert_in_reverse_order() {
        let mut plan = rollout(&["b", "c", "d", "a"]);
        plan.turn = Some("d".to_string());
        plan.done = live(&["b", "c"]);
        let mut d = pod("d", "0.1.0", false);
        d.server
            .failed
            .insert(v("0.2.0"), "exited during its trial".to_string());
        let members = [
            pod("a", "0.1.0", false),
            pod("b", "0.2.0", false),
            pod("c", "0.2.0", false),
            d,
        ];
        let all = live(&["a", "b", "c", "d"]);
        assert_eq!(
            next_step(&plan, &all, &members),
            RolloutStep::Failed {
                pod: "d".to_string(),
                reason: "exited during its trial".to_string()
            }
        );
        plan.reverting = true;
        plan.turn = None;
        assert_eq!(
            next_step(&plan, &all, &members),
            RolloutStep::Turn("c".to_string()),
            "the last upgraded goes back first"
        );
        plan.turn = Some("c".to_string());
        let members = [
            pod("a", "0.1.0", false),
            pod("b", "0.2.0", false),
            pod("c", "0.1.0", false),
            pod("d", "0.1.0", false),
        ];
        assert_eq!(
            next_step(&plan, &all, &members),
            RolloutStep::Turn("b".to_string())
        );
        let members = [
            pod("a", "0.1.0", false),
            pod("b", "0.1.0", false),
            pod("c", "0.1.0", false),
            pod("d", "0.1.0", false),
        ];
        plan.turn = Some("b".to_string());
        assert_eq!(next_step(&plan, &all, &members), RolloutStep::Reverted);
    }

    #[test]
    fn stale_pod_skipped() {
        let mut plan = rollout(&["b", "c", "a"]);
        plan.turn = Some("b".to_string());
        let members = [
            pod("a", "0.1.0", false),
            pod("b", "0.1.0", false),
            pod("c", "0.1.0", false),
        ];
        assert_eq!(
            next_step(&plan, &live(&["a", "c"]), &members),
            RolloutStep::Turn("c".to_string()),
            "b went stale during its turn"
        );
        plan.turn = Some("a".to_string());
        plan.done = live(&["c"]);
        let members = [
            pod("a", "0.2.0", false),
            pod("b", "0.1.0", false),
            pod("c", "0.2.0", false),
        ];
        assert_eq!(
            next_step(&plan, &live(&["a", "c"]), &members),
            RolloutStep::Done,
            "b is not waited for"
        );
    }

    #[test]
    fn late_joiner_upgrades_last() {
        let mut plan = rollout(&["b", "a"]);
        plan.turn = Some("a".to_string());
        plan.done = live(&["b"]);
        let members = [
            pod("a", "0.2.0", false),
            pod("b", "0.2.0", false),
            pod("e", "0.1.0", false),
        ];
        assert_eq!(
            next_step(&plan, &live(&["a", "b", "e"]), &members),
            RolloutStep::Turn("e".to_string())
        );
        let members = [
            pod("a", "0.2.0", false),
            pod("b", "0.2.0", false),
            pod("e", "0.2.0", false),
        ];
        plan.turn = Some("e".to_string());
        assert_eq!(
            next_step(&plan, &live(&["a", "b", "e"]), &members),
            RolloutStep::Done
        );
    }

    /// Final review I7: a failure recorded in an earlier rollout of the same version does not fail this one.
    #[test]
    fn old_failure_does_not_fail_a_new_rollout() {
        let mut plan = rollout(&["b", "a"]);
        plan.id = 9;
        let mut b = pod("b", "0.1.0", false);
        b.server
            .failed
            .insert(v("0.2.0"), "an earlier rollout".to_string());
        b.server.failed_in.insert(v("0.2.0"), 4);
        let members = [pod("a", "0.1.0", false), b.clone()];
        assert_eq!(
            next_step(&plan, &live(&["a", "b"]), &members),
            RolloutStep::Turn("b".to_string())
        );
        let mut b = b;
        b.server.failed_in.insert(v("0.2.0"), 9);
        let members = [pod("a", "0.1.0", false), b];
        assert!(matches!(
            next_step(&plan, &live(&["a", "b"]), &members),
            RolloutStep::Failed { .. }
        ));
    }
}
