//! Leadership through a `coordination.k8s.io/v1` Lease.
//!
//! A background thread runs a current-thread tokio runtime that reads the Lease every `retry` and renews it (as its
//! holder) or takes it over (once its holder has stopped renewing). Updates are optimistic: a `resourceVersion`
//! conflict means another pod got there first, and nothing is claimed. Expiry of another pod's lease is judged by
//! *this* pod's clock: it notes when the holder's `renewTime` last changed, as it saw it, and never compares the two
//! pods' clocks. `view()` stops claiming leadership the moment `renew_deadline` has passed since the last successful
//! renewal, before anyone else could take the Lease.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use k8s_openapi::api::coordination::v1::{Lease, LeaseSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{MicroTime, ObjectMeta};
use kube::api::{Api, PostParams};
use speedyspec_server_domain::{LeaderView, Leadership, LeaseTimings};

/// Which Lease, and who this pod is.
#[derive(Clone, Debug)]
pub struct LeaseSettings {
    pub name: String,
    pub namespace: String,
    pub pod: String,
    pub timings: LeaseTimings,
}

#[derive(Default)]
struct Shared {
    /// When this pod last renewed (or took) the Lease successfully.
    renewed: Option<Instant>,
    holder: Option<String>,
    transitions: u64,
    /// The holder and `renewTime` last seen, and when this pod saw them change.
    observed: Option<(Option<String>, Option<String>)>,
    observed_at: Option<Instant>,
    /// The holder's lease duration, from the Lease itself.
    holder_lease: Duration,
    /// Set by `step_down`: this pod no longer takes the Lease.
    stepped_down: bool,
    watchers: Vec<Sender<LeaderView>>,
    last_sent: Option<LeaderView>,
}

enum Command {
    StepDown(Sender<()>),
}

/// See the crate docs. Dropping it stops the renewing thread (without releasing the Lease; use `step_down`).
pub struct KubeLeadership {
    pod: String,
    timings: LeaseTimings,
    shared: Arc<Mutex<Shared>>,
    commands: tokio::sync::mpsc::UnboundedSender<Command>,
}

/// The service account's namespace, inside a pod.
pub fn pod_namespace() -> Option<String> {
    std::fs::read_to_string("/var/run/secrets/kubernetes.io/serviceaccount/namespace")
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

impl KubeLeadership {
    /// Starts with the in-cluster config (the pod's service account).
    pub fn in_cluster(settings: LeaseSettings) -> Result<KubeLeadership, String> {
        let config = kube::Config::incluster()
            .map_err(|err| format!("in-cluster Kubernetes config: {err}"))?;
        KubeLeadership::start(config, settings)
    }

    pub fn start(config: kube::Config, settings: LeaseSettings) -> Result<KubeLeadership, String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("starting the Lease runtime: {err}"))?;
        let client = runtime
            .block_on(async { kube::Client::try_from(config) })
            .map_err(|err| format!("Kubernetes client: {err}"))?;
        let shared: Arc<Mutex<Shared>> = Arc::default();
        let (commands, receiver) = tokio::sync::mpsc::unbounded_channel();
        let leadership = KubeLeadership {
            pod: settings.pod.clone(),
            timings: settings.timings,
            shared: shared.clone(),
            commands,
        };
        std::thread::Builder::new()
            .name("speedyspec-lease".to_string())
            .spawn(move || runtime.block_on(run(client, settings, shared, receiver)))
            .map_err(|err| format!("starting the Lease thread: {err}"))?;
        Ok(leadership)
    }
}

fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|p| p.into_inner())
}

fn view_of(pod: &str, timings: &LeaseTimings, shared: &Shared) -> LeaderView {
    let is_leader = shared.holder.as_deref() == Some(pod)
        && shared
            .renewed
            .is_some_and(|at| at.elapsed() < timings.renew_deadline);
    let holder_live = shared
        .observed_at
        .is_some_and(|at| at.elapsed() < shared.holder_lease.max(timings.lease));
    let leader = if is_leader {
        Some(pod.to_string())
    } else {
        shared
            .holder
            .clone()
            .filter(|holder| holder != pod && holder_live)
    };
    LeaderView {
        is_leader,
        epoch: shared.transitions,
        leader,
    }
}

fn now() -> MicroTime {
    MicroTime(k8s_openapi::jiff::Timestamp::now())
}

fn whole_seconds(duration: Duration) -> i32 {
    duration.as_secs_f64().ceil().max(1.0) as i32
}

async fn run(
    client: kube::Client,
    settings: LeaseSettings,
    shared: Arc<Mutex<Shared>>,
    mut commands: tokio::sync::mpsc::UnboundedReceiver<Command>,
) {
    let api: Api<Lease> = Api::namespaced(client, &settings.namespace);
    loop {
        tick(&api, &settings, &shared).await;
        notify(&settings, &shared);
        tokio::select! {
            () = tokio::time::sleep(settings.timings.retry) => {}
            command = commands.recv() => match command {
                Some(Command::StepDown(done)) => {
                    step_down(&api, &settings, &shared).await;
                    notify(&settings, &shared);
                    let _ = done.send(());
                }
                None => return,
            }
        }
    }
}

async fn tick(api: &Api<Lease>, settings: &LeaseSettings, shared: &Mutex<Shared>) {
    let pod = settings.pod.as_str();
    let attempt = Instant::now();
    let found = match api.get_opt(&settings.name).await {
        Ok(found) => found,
        Err(err) => {
            tracing::warn!(event = "lease_unreadable", error = %err);
            return;
        }
    };
    let stepped_down = lock(shared).stepped_down;
    let Some(lease) = found else {
        if stepped_down {
            return;
        }
        let lease = Lease {
            metadata: ObjectMeta {
                name: Some(settings.name.clone()),
                ..Default::default()
            },
            spec: Some(LeaseSpec {
                holder_identity: Some(pod.to_string()),
                lease_duration_seconds: Some(whole_seconds(settings.timings.lease)),
                acquire_time: Some(now()),
                renew_time: Some(now()),
                lease_transitions: Some(0),
                ..Default::default()
            }),
        };
        match api.create(&PostParams::default(), &lease).await {
            Ok(_) => {
                let mut shared = lock(shared);
                shared.renewed = Some(attempt);
                shared.holder = Some(pod.to_string());
                shared.transitions = 0;
                shared.observed_at = Some(attempt);
                tracing::info!(event = "lease_created", pod = %pod);
            }
            Err(err) => tracing::info!(event = "lease_create_lost", error = %err),
        }
        return;
    };

    let spec = lease.spec.clone().unwrap_or_default();
    let holder = spec.holder_identity.clone().filter(|h| !h.is_empty());
    let key = (
        holder.clone(),
        spec.renew_time.as_ref().map(|t| t.0.to_string()),
    );
    let transitions = spec.lease_transitions.unwrap_or(0).max(0) as u64;
    let expired = {
        let mut shared = lock(shared);
        if shared.observed.as_ref() != Some(&key) {
            shared.observed = Some(key);
            shared.observed_at = Some(attempt);
        }
        shared.holder = holder.clone();
        shared.transitions = transitions;
        shared.holder_lease =
            Duration::from_secs(spec.lease_duration_seconds.unwrap_or(0).max(0) as u64);
        let lease_for = shared.holder_lease.max(settings.timings.lease);
        shared
            .observed_at
            .is_some_and(|at| at.elapsed() >= lease_for)
    };

    if holder.as_deref() == Some(pod) {
        let mut renewal = lease.clone();
        let spec = renewal.spec.get_or_insert_with(Default::default);
        spec.renew_time = Some(now());
        spec.lease_duration_seconds = Some(whole_seconds(settings.timings.lease));
        match api
            .replace(&settings.name, &PostParams::default(), &renewal)
            .await
        {
            Ok(_) => lock(shared).renewed = Some(attempt),
            Err(err) => tracing::warn!(event = "lease_renew_failed", error = %err),
        }
        return;
    }
    lock(shared).renewed = None;
    if stepped_down || !(holder.is_none() || expired) {
        return;
    }
    let mut taking = lease.clone();
    let spec = taking.spec.get_or_insert_with(Default::default);
    spec.holder_identity = Some(pod.to_string());
    spec.lease_duration_seconds = Some(whole_seconds(settings.timings.lease));
    spec.acquire_time = Some(now());
    spec.renew_time = Some(now());
    spec.lease_transitions = Some((transitions + 1) as i32);
    match api
        .replace(&settings.name, &PostParams::default(), &taking)
        .await
    {
        Ok(_) => {
            let mut shared = lock(shared);
            shared.renewed = Some(attempt);
            shared.holder = Some(pod.to_string());
            shared.transitions = transitions + 1;
            shared.observed_at = Some(attempt);
            tracing::info!(event = "lease_acquired", pod = %pod, epoch = transitions + 1, previous = ?holder);
        }
        // A conflict: another pod took it, or its holder renewed after all. Nothing is claimed.
        Err(err) => tracing::info!(event = "lease_take_lost", error = %err),
    }
}

async fn step_down(api: &Api<Lease>, settings: &LeaseSettings, shared: &Mutex<Shared>) {
    lock(shared).stepped_down = true;
    let held = lock(shared).holder.as_deref() == Some(settings.pod.as_str());
    lock(shared).renewed = None;
    if !held {
        return;
    }
    match api.get_opt(&settings.name).await {
        Ok(Some(mut lease)) => {
            let spec = lease.spec.get_or_insert_with(Default::default);
            if spec.holder_identity.as_deref() != Some(settings.pod.as_str()) {
                return;
            }
            spec.holder_identity = None;
            spec.renew_time = None;
            match api
                .replace(&settings.name, &PostParams::default(), &lease)
                .await
            {
                Ok(_) => {
                    lock(shared).holder = None;
                    tracing::info!(event = "lease_released", pod = %settings.pod);
                }
                Err(err) => tracing::warn!(event = "lease_release_failed", error = %err),
            }
        }
        Ok(None) => {}
        Err(err) => tracing::warn!(event = "lease_release_failed", error = %err),
    }
}

fn notify(settings: &LeaseSettings, shared: &Mutex<Shared>) {
    let mut shared = lock(shared);
    let view = view_of(&settings.pod, &settings.timings, &shared);
    if shared.last_sent.as_ref() != Some(&view) {
        shared.watchers.retain(|tx| tx.send(view.clone()).is_ok());
        shared.last_sent = Some(view);
    }
}

impl Leadership for KubeLeadership {
    fn view(&self) -> LeaderView {
        view_of(&self.pod, &self.timings, &lock(&self.shared))
    }

    fn changed(&self) -> Receiver<LeaderView> {
        let (tx, rx) = channel();
        lock(&self.shared).watchers.push(tx);
        rx
    }

    fn step_down(&self) {
        let (done, wait) = channel();
        if self.commands.send(Command::StepDown(done)).is_ok() {
            let _ = wait.recv_timeout(self.timings.renew_deadline);
        }
        lock(&self.shared).renewed = None;
    }
}
