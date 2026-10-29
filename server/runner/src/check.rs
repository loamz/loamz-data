//! Reads a config back, as it will be handed to the runtime, and refuses it unless every promise is in it. This is
//! the gate before any code runs: a config that lost a limit on the way (a bug, an edit) never starts a sandbox.

use serde_json::Value;

/// Returns every problem found, so one run of the check says everything that is wrong.
///
/// # Errors
/// The list of missing or weakened promises.
pub fn verify(config: &Value) -> Result<(), Vec<String>> {
    let mut bad = Vec::new();
    let mut need = |ok: bool, why: &str| {
        if !ok {
            bad.push(why.to_owned());
        }
    };
    let limit = |v: &Value| v.as_u64().is_some_and(|n| n > 0);

    need(
        limit(&config["linux"]["resources"]["memory"]["limit"]),
        "no memory limit",
    );
    need(
        limit(&config["linux"]["resources"]["pids"]["limit"]),
        "no process limit",
    );
    need(
        limit(&config["linux"]["resources"]["cpu"]["quota"]),
        "no cpu limit",
    );
    need(
        config["root"]["readonly"] == true,
        "the root filesystem is writable",
    );
    need(
        config["process"]["noNewPrivileges"] == true,
        "new privileges are allowed",
    );
    need(
        config["process"]["capabilities"]["bounding"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "capabilities are not dropped",
    );
    need(
        config["process"]["user"]["uid"]
            .as_u64()
            .is_some_and(|u| u != 0),
        "runs as root",
    );

    let namespaces = config["linux"]["namespaces"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let net = namespaces.iter().find(|n| n["type"] == "network");
    need(
        net.is_some_and(|n| n.get("path").is_none()),
        "the sandbox is not in an empty network of its own",
    );
    need(
        namespaces.iter().any(|n| n["type"] == "pid"),
        "no process namespace",
    );

    let mounts = config["mounts"].as_array().cloned().unwrap_or_default();
    let has = |m: &Value, opt: &str| {
        m["options"]
            .as_array()
            .is_some_and(|o| o.iter().any(|x| x == opt))
    };
    let writable_binds: Vec<_> = mounts
        .iter()
        .filter(|m| m["type"] == "bind" && !has(m, "ro"))
        .collect();
    need(
        writable_binds.len() == 1 && writable_binds[0]["destination"] == "/workspace",
        "there must be exactly one writable folder, /workspace",
    );
    for m in mounts
        .iter()
        .filter(|m| m["type"] == "bind" && m["destination"] != "/workspace")
    {
        need(has(m, "ro"), "an input is not read-only");
    }

    if bad.is_empty() { Ok(()) } else { Err(bad) }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::semicolon_if_nothing_returned)]
mod tests {
    use super::*;
    use crate::spec::{Sandbox, oci_config};

    fn good() -> Value {
        oci_config(&Sandbox {
            rootfs: "/r".into(),
            inputs: vec![("in".into(), "/d".into())],
            workspace: "/w".into(),
            args: vec!["true".into()],
            memory_bytes: 1 << 28,
            pids_max: 32,
            cpu_millis: 500,
        })
        .unwrap()
    }

    #[test]
    fn what_the_builder_makes_passes() {
        assert_eq!(verify(&good()), Ok(()));
    }

    /// Breaks one promise in a good config and says the check must notice.
    fn caught(what: &str, break_it: impl FnOnce(&mut Value)) {
        let mut c = good();
        break_it(&mut c);
        assert!(verify(&c).is_err(), "{what} should have been caught");
    }

    #[test]
    fn each_lost_promise_is_caught() {
        use serde_json::json;
        caught("memory", |c| {
            c["linux"]["resources"]["memory"]["limit"] = json!(0)
        });
        caught("process limit", |c| {
            c["linux"]["resources"]
                .as_object_mut()
                .unwrap()
                .remove("pids");
        });
        caught("writable root", |c| c["root"]["readonly"] = json!(false));
        caught("root user", |c| c["process"]["user"]["uid"] = json!(0));
        caught("host network", |c| {
            c["linux"]["namespaces"][1]["path"] = json!("/proc/1/ns/net")
        });
        caught("writable input", |c| {
            c["mounts"][3]["options"] = json!(["rbind", "rw"])
        });
        caught("capabilities", |c| {
            c["process"]["capabilities"]["bounding"] = json!(["CAP_SYS_ADMIN"])
        });
    }
}
