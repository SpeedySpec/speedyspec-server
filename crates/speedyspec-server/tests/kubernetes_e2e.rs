//! End to end on a real, throwaway cluster: the k3s that `scripts/k3s.sh up` starts, with three replicas sharing
//! one volume and electing their leader through the Lease.
//!
//! Every test is ignored by default and takes its kubeconfig from `SPEEDYSPEC_E2E_KUBECONFIG` only; it fails rather
//! than fall back to your own cluster. Run:
//!
//! ```text
//! scripts/k3s.sh up
//! SPEEDYSPEC_E2E_KUBECONFIG=$(scripts/k3s.sh kubeconfig) \
//!   cargo test -p speedyspec-server --features test-hooks --test kubernetes_e2e -- --ignored --test-threads=1
//! scripts/k3s.sh down
//! ```
//!
//! Each pod has a sidecar (the same image, in immutable mode) serving the release files on 127.0.0.1:8081, so pods
//! download over loopback, which the release source allows without TLS.
#![cfg(feature = "test-hooks")]

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use std::time::{Duration, Instant};

use support::release_host::ReleaseHost;
use support::versions::next;

const K3S: &str = "speedyspec-server-k3s";
const IMAGE: &str = "speedyspec-server:e2e";

fn kubeconfig() -> String {
    std::env::var("SPEEDYSPEC_E2E_KUBECONFIG").expect(
        "set SPEEDYSPEC_E2E_KUBECONFIG to the kubeconfig `scripts/k3s.sh up` wrote; this test never uses your own \
         cluster",
    )
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs a command; returns (exit code, stdout, stderr).
fn run(program: &str, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(program)
        .args(args)
        .env("KUBECONFIG", kubeconfig())
        .output()
        .unwrap_or_else(|err| panic!("{program}: {err}"));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn ok(program: &str, args: &[&str]) -> String {
    let (code, out, err) = run(program, args);
    assert_eq!(code, 0, "{program} {args:?}: {err}{out}");
    out
}

fn wait_for(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    done()
}

/// Builds the image (with test hooks) once, and imports it into the k3s node.
fn image() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let root = repo();
        ok(
            "docker",
            &[
                "build",
                "-t",
                IMAGE,
                "--build-arg",
                "FEATURES=test-hooks",
                root.to_str().unwrap(),
            ],
        );
        let status = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "docker save {IMAGE} | docker exec -i {K3S} ctr -n k8s.io images import -"
            ))
            .status()
            .unwrap();
        assert!(status.success(), "importing the image into k3s");
    });
}

/// The image's own Linux binary, copied out, and its target triple.
fn linux_binary(dir: &Path) -> (PathBuf, String) {
    let name = format!("speedyspec-e2e-{}", std::process::id());
    ok("docker", &["create", "--name", &name, IMAGE]);
    let path = dir.join("speedyspec-server");
    ok(
        "docker",
        &[
            "cp",
            &format!("{name}:/speedyspec-server"),
            path.to_str().unwrap(),
        ],
    );
    ok("docker", &["rm", &name]);
    let arch = ok("docker", &["exec", K3S, "uname", "-m"])
        .trim()
        .to_string();
    (path, format!("{arch}-unknown-linux-musl"))
}

/// One namespace with the deployment; deleted when dropped.
struct Deployment {
    ns: String,
    _files: tempfile::TempDir,
}

impl Drop for Deployment {
    fn drop(&mut self) {
        let _ = run(
            "kubectl",
            &["delete", "namespace", &self.ns, "--wait=false"],
        );
    }
}

fn deploy(ns: &str) -> Deployment {
    image();
    let files = tempfile::tempdir().unwrap();
    let host = ReleaseHost::start();
    host.publish_static("admin", "1.0.0");
    let (binary, target) = linux_binary(files.path());
    host.publish_server_for(next(), &binary, &target);
    let releases = files.path().join("releases");
    host.write_to(&releases.join("files"), "http://127.0.0.1:8081");
    std::fs::write(
        releases.join("speedyspec-release.json"),
        r#"{"app":"releases","version":"1.0.0","kind":"static","static":{"root":"files"}}"#,
    )
    .unwrap();
    let node_dir = format!("/srv/{ns}");
    let _ = run("docker", &["exec", K3S, "rm", "-rf", &node_dir]);
    ok("docker", &["exec", K3S, "mkdir", "-p", "/srv"]);
    ok(
        "docker",
        &[
            "cp",
            releases.to_str().unwrap(),
            &format!("{K3S}:{node_dir}"),
        ],
    );

    let _ = run(
        "kubectl",
        &["delete", "namespace", ns, "--wait=true", "--timeout=120s"],
    );
    ok("kubectl", &["create", "namespace", ns]);
    for manifest in ["serviceaccount", "role", "rolebinding", "service"] {
        let path = repo().join(format!("deploy/kubernetes/{manifest}.yaml"));
        ok(
            "kubectl",
            &["-n", ns, "apply", "-f", path.to_str().unwrap()],
        );
    }
    let server = format!(
        r#"mode = "managed"
data_dir = "/var/lib/speedyspec"
run_dir = "/run/speedyspec"

[[listener]]
address = "0.0.0.0:8080"

[app.server]
kind = "server"
source = "http://127.0.0.1:8081/server/index.json"
keys = ["{key}"]

[app.admin]
kind = "static"
source = "http://127.0.0.1:8081/admin/index.json"
keys = ["{key}"]
update = "manual"

[[mount]]
path = "/"
app = "admin"

[handoff]
trial_period = "5s"
ready_timeout = "30s"
drain_timeout = "5s"

[cluster]
leadership = "kubernetes"
probe = "0.0.0.0:9090"
poll_interval = "1s"
"#,
        key = host.public_key()
    );
    let sidecar = r#"mode = "immutable"
run_dir = "/tmp"

[[listener]]
address = "127.0.0.1:8081"

[app.server]
kind = "server"

[app.releases]
kind = "static"
path = "/releases"

[[mount]]
path = "/"
app = "releases"
"#;
    std::fs::write(files.path().join("server.toml"), server).unwrap();
    std::fs::write(files.path().join("releases.toml"), sidecar).unwrap();
    ok(
        "kubectl",
        &[
            "-n",
            ns,
            "create",
            "configmap",
            "speedyspec-config",
            &format!(
                "--from-file=server.toml={}",
                files.path().join("server.toml").display()
            ),
            &format!(
                "--from-file=releases.toml={}",
                files.path().join("releases.toml").display()
            ),
        ],
    );
    let manifests = format!(
        r#"apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: speedyspec-data
spec:
  accessModes: ["ReadWriteOnce"]
  resources:
    requests:
      storage: 1Gi
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: speedyspec-server
spec:
  replicas: 3
  selector:
    matchLabels:
      app: speedyspec-server
  template:
    metadata:
      labels:
        app: speedyspec-server
    spec:
      serviceAccountName: speedyspec-server
      terminationGracePeriodSeconds: 10
      containers:
        - name: server
          image: {IMAGE}
          imagePullPolicy: Never
          args: ["run", "--config", "/etc/speedyspec/server.toml"]
          env:
            - name: POD_NAME
              valueFrom:
                fieldRef:
                  fieldPath: metadata.name
            - name: SPEEDYSPEC_TEST_VERSION_FROM_DIR
              value: "1"
          readinessProbe:
            httpGet:
              path: /readyz
              port: 9090
            periodSeconds: 2
          volumeMounts:
            - name: config
              mountPath: /etc/speedyspec
            - name: data
              mountPath: /var/lib/speedyspec
        - name: releases
          image: {IMAGE}
          imagePullPolicy: Never
          args: ["run", "--config", "/etc/speedyspec/releases.toml"]
          volumeMounts:
            - name: config
              mountPath: /etc/speedyspec
            - name: releases
              mountPath: /releases
      volumes:
        - name: config
          configMap:
            name: speedyspec-config
        - name: data
          persistentVolumeClaim:
            claimName: speedyspec-data
        - name: releases
          hostPath:
            path: {node_dir}
            type: Directory
"#
    );
    let path = files.path().join("deployment.yaml");
    std::fs::write(&path, manifests).unwrap();
    ok(
        "kubectl",
        &["-n", ns, "apply", "-f", path.to_str().unwrap()],
    );
    let (code, out, err) = run(
        "kubectl",
        &[
            "-n",
            ns,
            "rollout",
            "status",
            "deployment/speedyspec-server",
            "--timeout=300s",
        ],
    );
    assert_eq!(
        code,
        0,
        "the deployment never became ready: {out}{err}\n{}",
        describe(ns)
    );
    Deployment {
        ns: ns.to_string(),
        _files: files,
    }
}

/// Pod states and the server containers' last log lines, for failure messages.
fn describe(ns: &str) -> String {
    let mut out = run("kubectl", &["-n", ns, "get", "pods", "-o", "wide"]).1;
    for pod in pods(ns) {
        out.push_str(&format!("--- {pod}\n"));
        out.push_str(
            &run(
                "kubectl",
                &["-n", ns, "logs", &pod, "-c", "server", "--tail=30"],
            )
            .1,
        );
    }
    out
}

fn pods(ns: &str) -> Vec<String> {
    run(
        "kubectl",
        &[
            "-n",
            ns,
            "get",
            "pods",
            "-l",
            "app=speedyspec-server",
            "-o",
            "jsonpath={.items[*].metadata.name}",
        ],
    )
    .1
    .split_whitespace()
    .map(str::to_string)
    .collect()
}

/// `speedyspec-server <args>` inside a pod's server container.
fn exec(ns: &str, pod: &str, args: &[&str]) -> (i32, String, String) {
    let mut all = vec![
        "-n",
        ns,
        "exec",
        pod,
        "-c",
        "server",
        "--",
        "/speedyspec-server",
    ];
    all.extend_from_slice(args);
    all.extend(["--config", "/etc/speedyspec/server.toml"]);
    run("kubectl", &all)
}

fn exec_ok(ns: &str, pod: &str, args: &[&str]) -> String {
    let (code, out, err) = exec(ns, pod, args);
    assert_eq!(code, 0, "{args:?} on {pod}: {err}{out}\n{}", describe(ns));
    out
}

/// The leader, as the first pod sees it.
fn leader(ns: &str) -> Option<String> {
    let pods = pods(ns);
    let out = exec(ns, pods.first()?, &["status"]).1;
    out.lines()
        .find_map(|l| l.strip_prefix("cluster\t"))
        .and_then(|rest| rest.split('\t').nth(2))
        .filter(|l| *l != "-")
        .map(str::to_string)
}

/// `GET path` on one pod's port 8080, through the API server's pod proxy.
fn get_from(ns: &str, pod: &str, path: &str) -> String {
    let path = path.trim_start_matches('/');
    run(
        "kubectl",
        &[
            "get",
            "--raw",
            &format!("/api/v1/namespaces/{ns}/pods/{pod}:8080/proxy/{path}"),
        ],
    )
    .1
}

fn lease_holder(ns: &str) -> String {
    run(
        "kubectl",
        &[
            "-n",
            ns,
            "get",
            "lease",
            "speedyspec-server",
            "-o",
            "jsonpath={.spec.holderIdentity}",
        ],
    )
    .1
    .trim()
    .to_string()
}

#[test]
#[ignore = "needs the throwaway k3s from scripts/k3s.sh and SPEEDYSPEC_E2E_KUBECONFIG"]
fn admin_switch_reaches_all_replicas() {
    let deployment = deploy("speedyspec-e2e-switch");
    let ns = deployment.ns.as_str();
    assert!(
        wait_for(Duration::from_secs(60), || leader(ns).is_some()),
        "no leader: {}",
        describe(ns)
    );
    let leader = leader(ns).unwrap();
    assert_eq!(lease_holder(ns), leader, "the Lease names the leader");
    exec_ok(ns, &leader, &["release", "install", "admin", "1.0.0"]);
    exec_ok(ns, &leader, &["release", "switch", "admin@1.0.0"]);
    for pod in pods(ns) {
        assert!(
            wait_for(Duration::from_secs(30), || get_from(ns, &pod, "/")
                == "admin-1.0.0"),
            "{pod} never served admin 1.0.0\n{}",
            describe(ns)
        );
    }
    let follower = pods(ns).into_iter().find(|p| *p != leader).unwrap();
    let (code, _, err) = exec(ns, &follower, &["release", "pin", "admin", "1.0.0"]);
    assert_eq!(code, 1);
    assert!(
        err.contains(&format!("run release commands on {leader}")),
        "{err}"
    );
}

#[test]
#[ignore = "needs the throwaway k3s from scripts/k3s.sh and SPEEDYSPEC_E2E_KUBECONFIG"]
fn server_upgrade_rolls_all_replicas() {
    let deployment = deploy("speedyspec-e2e-upgrade");
    let ns = deployment.ns.as_str();
    assert!(
        wait_for(Duration::from_secs(60), || leader(ns).is_some()),
        "no leader: {}",
        describe(ns)
    );
    let leader = leader(ns).unwrap();
    exec_ok(ns, &leader, &["release", "install", "server", next()]);
    let out = exec_ok(
        ns,
        &leader,
        &["release", "switch", &format!("server@{}", next())],
    );
    assert!(
        out.contains(&format!("rolling out server {} across 3 pods", next())),
        "{out}"
    );
    let mut holders = Vec::new();
    let done = wait_for(Duration::from_secs(300), || {
        holders.push(lease_holder(ns));
        pods(ns).iter().all(|pod| {
            exec(ns, pod, &["status"]).1.lines().any(|l| {
                l.starts_with("generation\t") && l.ends_with(&format!("\t{}\tconfirmed", next()))
            })
        })
    });
    assert!(
        done,
        "the rollout never reached every pod\n{}",
        describe(ns)
    );
    holders.retain(|h| !h.is_empty());
    assert!(
        holders.iter().all(|h| *h == leader),
        "the Lease's holder changed during the rollout: {holders:?}"
    );
}
