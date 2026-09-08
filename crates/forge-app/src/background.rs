//! Background evaluation worker, autosave and export jobs.
//!
//! Threading model (NFR-PER-01: the UI thread never blocks on geometry):
//! - The **UI thread** owns the `Document` and all egui state.
//! - The **evaluation worker** owns the `Evaluator` cache. The UI sends a
//!   document clone; the worker computes which features changed (data
//!   fingerprints + DAG descendant propagation), evaluates them with
//!   `rayon` acceleration inside the kernel, and posts the
//!   [`Evaluation`] back.
//! - The **tokio runtime** handles file I/O jobs (exports) via
//!   `spawn_blocking`.

use forge_core::FeatureId;
use forge_model::{Document, Evaluation, Evaluator};
use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, Sender};

/// Request to the evaluation worker.
pub enum EvalRequest {
    /// Evaluate this document state.
    Evaluate(Document),
}

/// Worker response.
pub enum EvalResponse {
    /// Evaluation finished.
    Done(Evaluation),
}

/// Handle to the background evaluation worker.
pub struct EvalWorker {
    pub tx: Sender<EvalRequest>,
    pub rx: Receiver<EvalResponse>,
}

impl EvalWorker {
    /// Spawn the worker thread.
    pub fn spawn() -> Self {
        let (tx_req, rx_req) = std::sync::mpsc::channel::<EvalRequest>();
        let (tx_res, rx_res) = std::sync::mpsc::channel::<EvalResponse>();

        std::thread::Builder::new()
            .name("forge-eval".into())
            .spawn(move || {
                let mut evaluator = Evaluator::default();
                // Feature data fingerprints of the last evaluated state.
                let mut last_fingerprint: BTreeMap<FeatureId, String> = BTreeMap::new();

                while let Ok(req) = rx_req.recv() {
                    match req {
                        EvalRequest::Evaluate(mut doc) => {
                            // Compute the dirty set from fingerprints.
                            let dirty = compute_dirty(&doc, &mut last_fingerprint);
                            // Clear all dirty flags, then set the computed
                            // ones (the worker owns evaluation truth).
                            for id in doc.tree.order().to_vec() {
                                let clean = !dirty.contains(&id);
                                if let Some(node) = doc.tree.get_mut(id) {
                                    node.dirty = clean;
                                }
                            }
                            let ev = evaluator.evaluate(&mut doc);
                            let _ = tx_res.send(EvalResponse::Done(ev));
                        }
                    }
                }
            })
            .expect("eval worker thread");

        Self {
            tx: tx_req,
            rx: rx_res,
        }
    }
}

/// Features whose data changed since the last fingerprint snapshot, plus
/// all their DAG descendants.
fn compute_dirty(
    doc: &Document,
    last_fingerprint: &mut BTreeMap<FeatureId, String>,
) -> Vec<FeatureId> {
    let mut changed: Vec<FeatureId> = Vec::new();
    for id in doc.tree.order().to_vec() {
        let Some(node) = doc.tree.get(id) else {
            continue;
        };
        let fp = fingerprint(&node.feature, node.suppressed);
        let dirty = match last_fingerprint.get(&id) {
            Some(prev) => *prev != fp,
            None => true,
        };
        if dirty {
            changed.push(id);
            last_fingerprint.insert(id, fp);
        } else {
            // Refresh the fingerprint (suppression is part of it).
            last_fingerprint.insert(id, fp);
        }
    }

    // Propagate to descendants (transitive children).
    let mut result: Vec<FeatureId> = changed.clone();
    let mut queue = changed;
    while let Some(id) = queue.pop() {
        if let Some(node) = doc.tree.get(id) {
            for child in node.children.iter().copied().collect::<Vec<_>>() {
                if !result.contains(&child) {
                    result.push(child);
                    queue.push(child);
                }
            }
        }
    }
    result
}

fn fingerprint(feature: &forge_model::Feature, suppressed: bool) -> String {
    // Debug formatting is not stable across Rust versions, but it only has
    // to be consistent within a single process run.
    format!("{suppressed}|{feature:?}")
}

/// A completed export job notification.
pub struct ExportDone {
    /// What was exported.
    pub what: String,
    /// Destination path.
    pub path: std::path::PathBuf,
    /// Result.
    pub result: Result<(), String>,
}

/// A completed import job notification (I-01): the repaired mesh is added
/// to the feature tree on the UI thread.
pub struct ImportDone {
    /// Source file path.
    pub path: std::path::PathBuf,
    /// The repaired, welded mesh (or the error text).
    pub result: Result<forge_geometry::TriMesh, String>,
}
