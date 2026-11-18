# Running several pods on one data directory

In managed mode the server can run as several replicas that share one `data_dir` volume (on Kubernetes, a
ReadWriteMany volume). One pod **leads**: it alone downloads, verifies, switches, prunes and writes `state.json`. The
others **follow**: they serve whatever the leader makes live. A single host (`[cluster]` absent, or
`leadership = "single"`) is a cluster of one that always leads, and behaves exactly as it did before clusters existed.

## Who leads

Leadership is a `coordination.k8s.io/v1` Lease (`cluster.leadership = "kubernetes"`), named `cluster.lease_name`
(default `speedyspec-server`) in `cluster.lease_namespace` (default: the pod's service account namespace).

- The Lease's holder is the **pod's name** (`cluster.pod`, default `$POD_NAME`). The pod's active generation renews
  it every `retry_period` (2 s). After a socket handoff inside that pod, the new generation renews under the same
  name, so leadership does not move during the pod's own upgrade.
- Another pod takes the Lease once its holder has stopped renewing for `lease_duration` (15 s). That is judged by
  the taking pod's own clock: it notes when the holder's `renewTime` last changed, as it saw it. No two pods' clocks
  are ever compared.
- A leader that has not renewed for `renew_deadline` (10 s) stops writing **at once**, before anyone else could take
  the Lease (`renew_deadline < lease_duration` is checked at start).
- The Lease's `leaseTransitions` is the **epoch**. It grows every time the leader changes.
- The pod needs a Role allowing `create` on Leases and `get` and `update` on that one Lease
  (`deploy/kubernetes/role.yaml`).

## What each pod writes

| Who | Writes |
|---|---|
| The leader | `state.json`, the release store (`releases/`), and its own member file |
| Every pod | only `<data_dir>/cluster/members/<pod>.json` |

Every `state.json` write carries the writing leader's epoch. A pod ignores a `state.json` whose epoch is older than
the Lease's current epoch, or than one it has already seen (`stale_epoch_ignored`). That is how a deposed leader's
late write is kept out. A new leader inherits the previous leader's state and re-stamps it with its own epoch at once
(`took_over`). It also rewrites a `state.json` that a late write replaced (`stale_epoch_repaired`).

`flock` is never taken on the shared volume (it is unreliable on network file systems). Files are written by
replacing them: a temp file, fsync, rename, then fsync the directory. A reader that finds a file it cannot parse (a
torn read) keeps its last good copy and logs `member_unreadable` once.

## Member files and liveness

Each pod rewrites its member file every 2 s. The file holds:

- a heartbeat counter;
- the `state.json` revision it serves;
- the staged revision it has prepared, or why it could not;
- every release it serves or holds prepared;
- whether it is ready;
- its own server binary state: the version it runs, its confirmed version, its trial, and its failed versions.

A pod counts as live while its heartbeat counter keeps changing. It counts as stale once the counter has not changed
for 10 s, as the reading pod saw it (`member_stale`). Again, no clock is compared.

## Switching apps together (two steps)

`release switch admin@2.0.0` on the leader:

1. **Stage** the set in `state.json` (`staged`), leaving the active set as it is.
2. **Prepare** it on every pod. The leader prepares it itself; followers see it within `poll_interval` (2 s) and
   report `prepared` or `prepare_failed`.
3. **Wait** until every live pod has reported on this staged revision, or `cluster.prepare_timeout` (60 s) passes. A
   pod that goes stale stops being waited for at once. A report made before the staged revision never counts.
4. **Activate.** If at least one pod prepared the set, it becomes active (`activated`, listing the prepared and failed
   pods), and every pod switches within one poll. If none prepared it, the switch is abandoned and the version marked
   failed with every pod's reason (`switch_abandoned`).

A pod that cannot prepare the active set keeps serving its old set and answers `/readyz` with 503, so Kubernetes
stops sending it traffic until a set it can prepare is active. Pruning never removes a release that any live pod
serves or holds prepared.

If the leader dies with a set staged, the next leader re-checks every staged release in the store, then finishes the
switch from where it stood. If one can no longer be verified, it abandons the switch.

## Upgrading the server, pod by pod

`release switch server@0.2.0` on the leader writes a **rollout** and returns at once:

```
rolling out server 0.2.0 across 3 pods (web-1, web-2, web-0), one at a time
```

- The order is the live pods by name, with the leader last, so it keeps coordinating until the end. Pods that join
  later go at the end.
- On its turn, a pod's generation asks its own keeper for the switch. The keeper runs the single-host socket handoff,
  trial included.
- The leader gives the next pod its turn once the current pod reports the target confirmed (`rollout_step`).
- A pod whose trial fails (its keeper falls back, and its member file records the failure) stops the rollout
  (`rollout_failed`). Every pod already upgraded goes back to the previous version, one at a time, most recent first.
  The version is then marked failed with that pod's reason (`rollout_reverted`).
- When every live pod runs the target, confirmed, `state.json`'s confirmed server becomes the target, and any apps
  switched in the same group become active.
- A pod that goes stale during its turn is skipped. It is not counted as upgraded.

Each pod's fall-back binary is its own confirmed version, recorded in its member file. When a pod starts as the
leader, it keeps its running executable as a server release, so every pod has a binary to revert to. Followers never
write to the store.

## Commands

Reads (`status`, `release list`, `release check`) work on any pod. Commands that change releases work only on the
leader. A follower refuses them, naming it:

```
speedyspec-server: this pod is not the leader; run release commands on web-0
```

`reload` works on every pod: it re-reads that pod's own config.

`status` gains two kinds of line in a cluster:

```
server	0.1.0	managed
generation	78344	0.1.0	confirmed
keeper	0.1.0
cluster	web-1	follower	web-0
member	web-0	live	5	ready	0.1.0
member	web-1	live	5	ready	0.1.0
member	web-2	live	5	ready	0.1.0
admin	static	1.0.0	1.0.0	-	-
```

`cluster` is this pod, its role and the leader. Each `member` line is a pod: live or stale, the `state.json` revision
it serves, ready or not, and the server version it runs.

## Probes

With `cluster.probe = "0.0.0.0:9090"` each pod serves `/livez` (200 while the generation runs) and `/readyz` on that
address. `/readyz` answers 200 once the generation is ready and serves the active set, and 503 with the reason
otherwise. `deploy/kubernetes/deployment.yaml` wires both.

## Testing

- **`tests/cluster.rs`** runs three keepers on one data directory with a one-host lease file
  (`leadership = "file"`, only in `test-hooks` builds). It covers the two-step switch, a pod that cannot prepare, the
  leader killed mid-switch, a deposed leader's late write, follower refusals, and the rolling upgrade and its revert.
- **`tests/kubernetes_e2e.rs`** runs three replicas on a throwaway k3s (`scripts/k3s.sh`) with the Lease and a shared
  volume. It covers an admin switch and a server rollout. It is ignored by default and needs
  `SPEEDYSPEC_E2E_KUBECONFIG`.
