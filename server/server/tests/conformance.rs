//! The family's token conventions (loamz-deploy `spec/`): every case in the shared vector file must give the outcome it names.
//! The file is a copy of `spec/vectors/token/loamz-data.json`; regenerate and copy it when the spec changes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use loamz_data_server::kernel::auth::{AuthError, Kind, Verifier};
use serde_json::Value;

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_verifier_gives_every_outcome_the_shared_vectors_name() {
    let doc: Value = serde_json::from_str(include_str!("vectors/token-loamz-data.json")).unwrap();
    assert_eq!(doc["audience"], "loamz-data");
    let verifier = Verifier::new(doc["issuer"].as_str().unwrap());
    verifier.set_keys(&doc["jwks"].to_string()).unwrap();
    let cases = doc["cases"].as_array().unwrap();
    assert!(cases.len() >= 10, "the vector file has its cases");
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let expect = &case["expect"];
        let got = verifier.verify(case["token"].as_str().unwrap());
        if expect["ok"] == true {
            let who = got.unwrap_or_else(|e| panic!("{name}: expected a caller, got {e:?}"));
            assert_eq!(
                who.subject,
                expect["subject"].as_str().unwrap(),
                "{name}: subject"
            );
            let kind = if expect["kind"] == "service" {
                Kind::Service
            } else {
                Kind::Person
            };
            assert_eq!(who.kind, kind, "{name}: kind");
            assert_eq!(
                who.client,
                expect["client"].as_str().unwrap(),
                "{name}: client"
            );
            assert_eq!(
                who.orgs.iter().map(|o| o.alias.clone()).collect::<Vec<_>>(),
                strings(&expect["orgs"]),
                "{name}: orgs"
            );
            assert_eq!(who.groups, strings(&expect["groups"]), "{name}: groups");
            assert_eq!(
                who.actor.as_deref(),
                expect["actor"].as_str(),
                "{name}: actor"
            );
        } else {
            let want = if expect["error"] == "unknown_key" {
                AuthError::UnknownKey
            } else {
                AuthError::Invalid
            };
            assert_eq!(got.unwrap_err(), want, "{name}");
        }
    }
}
