use cmd_lib::run_cmd;
use std::str::FromStr;

use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::api::core::v1::Node;
use k8s_openapi::api::core::v1::Pod;
use kube::api::Patch;
use kube::api::PatchParams;
use kube::api::{Api, ListParams, ResourceExt, WatchEvent};
use kube::Client;
use std::convert::From;

#[tokio::main]
async fn main() -> Result<(), kube::Error> {
    let key = std::env::var("KEY").unwrap_or_else(|_| "".into());
    let operator = std::env::var("OPERATOR").unwrap_or_else(|_| "gt".into());
    let threshold = std::env::var("THRESHOLD").unwrap_or_else(|_| "2".into());
    let threshold = threshold.parse().unwrap_or_else(|_| 2.0);

    let client = Client::try_default().await?;

    let nodes: Api<Node> = Api::all(client.clone());
    let lp = ListParams::default().timeout(250);

    let mut stream = nodes.watch(&lp, "0").await?.boxed();
    while let Some(status) = stream.try_next().await? {
        match status {
            WatchEvent::Added(o) | WatchEvent::Modified(o) => {
                let mut unschedulable: bool = false;
                if let Some(node_name) = &o.metadata.name {
                    println!("\n\nNode: {}", node_name);
                    if let Some(s) = o.status.as_ref() {
                        // PLEG problem
                        for node_condition in s.conditions.iter() {
                            if node_condition.type_ != "Ready" {
                                continue;
                            }
                            if node_condition.status == "True" {
                                continue;
                            }
                            if let Some(m) = node_condition.message.clone() {
                                if m.contains("PLEG") {
                                    println!("GOT PLEG");
                                    for add in &s.addresses {
                                        if add.type_ == "InternalIP" {
                                            let address = add.address.clone();
                                            println!("-------->\n------------->\n----------------->    {}\n------------->\n------->\n---->", address);
                                            run_cmd!(ssh -i ~/.ssh/id_key root@$address systemctl restart docker).expect("msg");
                                        }
                                    }
                                }
                            }
                        }
                        // resource problem
                        if key.is_empty() {
                            continue;
                        }
                        let mut max_value: f64 = 0.0;
                        let mut used_totaly: f64 = 0.0;
                        if let Some(max_value__) = &s.allocatable.get(&key) {
                            if let Ok(max_value_) =
                                kubectl_view_allocations::qty::Qty::from_str(&max_value__.0)
                            {
                                max_value = f64::from(&max_value_);
                                used_totaly = 0.0;
                                let ns_api: Api<Namespace> = Api::all(client.clone());
                                let nslp = ListParams::default().timeout(60);
                                for ns in ns_api.list(&nslp).await? {
                                    println!("  ns : {}", ns.name());

                                    let pods_api: Api<Pod> =
                                        Api::namespaced(client.clone(), &ns.name());
                                    let mut filter = "spec.nodeName=".to_owned();
                                    filter.push_str(&node_name);
                                    let podslp = ListParams::default().timeout(60).fields(&filter);
                                    match pods_api.list(&podslp).await {
                                        Ok(podslist) => {
                                            for pod in podslist {
                                                // println!("    pod {}", pod.name());
                                                if let Some(pod_spec) = pod.spec {
                                                    let ctr_vec = pod_spec.containers;
                                                    for ctr in ctr_vec {
                                                        if let Some(r) = &ctr.resources {
                                                            if let Some(value_used__) =
                                                                r.limits.get(&key)
                                                            {
                                                                if let Ok(value_used_) =
                                                        kubectl_view_allocations::qty::Qty::from_str(
                                                            &value_used__.0,
                                                        )
                                                    {
                                                        let value_used = f64::from(&value_used_);
                                                        used_totaly += value_used;
                                                    }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            println!("error {}", e)
                                        }
                                    }
                                }
                            }
                        }
                        println!(
                            "node({}) {} used({}) total({}) rate({})",
                            &key,
                            &node_name,
                            used_totaly,
                            max_value,
                            used_totaly / max_value
                        );
                        if ((operator == "gt") && (used_totaly / max_value > threshold))
                            || ((operator == "lt") && (used_totaly / max_value < threshold))
                        {
                            println!("YES!!! THRESHOLD!!! unschedulable should be true");
                            unschedulable = true;
                        } else {
                            println!("unschedulable shoule be false");
                            unschedulable = false;
                        }
                    }
                }
                if let Some(spec) = o.spec.clone() {
                    match spec.unschedulable {
                        Some(b) => {
                            if b != unschedulable {
                                let patch = serde_json::json!(
                                    {
                                        "spec": {
                                            "unschedulable": unschedulable,
                                        }
                                    }
                                );
                                let pp = PatchParams::default();
                                nodes.patch(&o.name(), &pp, &Patch::Merge(&patch)).await?;
                            }
                        }
                        None => {
                            let patch = serde_json::json!(
                                {
                                    "spec": {
                                        "unschedulable": unschedulable,
                                    }
                                }
                            );
                            let pp = PatchParams::default();
                            nodes.patch(&o.name(), &pp, &Patch::Merge(&patch)).await?;
                        }
                    }
                }
            }
            WatchEvent::Error(e) => println!("Error {}", e),
            _ => {}
        }
    }
    Ok(())
}
