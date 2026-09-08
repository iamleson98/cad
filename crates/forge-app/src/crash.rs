//! Crash reporter (PR-01): a panic hook that writes a local crash report
//! plus a document snapshot before the default panic behavior runs.
//!
//! Design:
//! - **No telemetry.** Reports stay on the user's disk (temp dir). No
//!   network calls, opt-in or otherwise — anything more is a deliberate
//!   future decision, never silent.
//! - **Freshest possible recovery.** The in-memory snapshot is refreshed
//!   every time the app requests an evaluation (i.e. after every
//!   mutation), so a crash loses at most the current in-flight frame —
//!   strictly fresher than the 2-minute autosave. At startup the app
//!   prefers the newest of {crash snapshot, autosave}.
//! - **Never panics in the panic path.** All writes are best-effort and
//!   lock poisoning is recovered, not unwrapped.

use forge_model::Document;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

/// Latest document snapshot (RON), refreshed after every mutation.
#[cfg(not(target_arch = "wasm32"))]
static SNAPSHOT: Mutex<Option<String>> = Mutex::new(None);

/// Directory for crash reports and document snapshots.
#[cfg(not(target_arch = "wasm32"))]
pub fn crash_dir() -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push("forgecad_crash");
    dir
}

/// Refresh the in-memory document snapshot. Cheap: the document is
/// parametric data only (no meshes); errors are silently ignored — a
/// missing snapshot degrades the crash report, it never breaks the app.
#[cfg(not(target_arch = "wasm32"))]
pub fn snapshot_document(doc: &Document) {
    if let Ok(ron) = ron::ser::to_string_pretty(doc, ron::ser::PrettyConfig::default()) {
        *SNAPSHOT.lock().unwrap_or_else(|e| e.into_inner()) = Some(ron);
    }
}

/// The newest crash document snapshot, if one exists.
#[cfg(not(target_arch = "wasm32"))]
pub fn latest_snapshot() -> Option<PathBuf> {
    let dir = std::fs::read_dir(crash_dir()).ok()?;
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("forgecad") {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH);
        if best.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
            best = Some((mtime, path));
        }
    }
    best.map(|(_, path)| path)
}

/// Install the crash-reporting panic hook (call once at startup, before
/// any window exists). The previous hook is chained afterwards so the
/// default behavior (message on stderr, unwind/abort) is preserved.
#[cfg(not(target_arch = "wasm32"))]
pub fn install_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_report(info);
        previous(info);
    }));
}

/// Write the crash report + document snapshot. Best-effort throughout.
#[cfg(not(target_arch = "wasm32"))]
fn write_report(info: &std::panic::PanicHookInfo<'_>) {
    let dir = crash_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stamp = format!("forgecad-crash-{ts}");

    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".into());
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "<unknown>".into());
    let backtrace = std::backtrace::Backtrace::force_capture();

    let report = format!(
        "ForgeCAD crash report\n\
         =====================\n\
         version: {}\n\
         time (unix): {ts}\n\
         panic: {payload}\n\
         at: {location}\n\
         \n\
         backtrace:\n{backtrace}\n\
         \n\
         A document snapshot (if any) sits next to this file as\n\
         {stamp}.forgecad and is picked up automatically on the next\n\
         launch. Reports stay local; ForgeCAD sends nothing anywhere.\n",
        env!("CARGO_PKG_VERSION")
    );
    let report_path = dir.join(format!("{stamp}.txt"));
    let snapshot_path = dir.join(format!("{stamp}.forgecad"));
    let _ = std::fs::write(&report_path, report);
    if let Some(ron) = SNAPSHOT.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = std::fs::write(&snapshot_path, ron);
        eprintln!(
            "ForgeCAD crash report: {} (+ document snapshot {})",
            report_path.display(),
            snapshot_path.display()
        );
    } else {
        eprintln!("ForgeCAD crash report: {}", report_path.display());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn snapshot_round_trips_through_migrations() {
        // The crash snapshot must be loadable by the normal loader
        // (migrations included) — it is the recovery path.
        let mut doc = Document::new("crashy");
        doc.add_feature(forge_model::Feature::Primitive(
            forge_model::PrimitiveParams {
                kind: forge_model::PrimitiveKind::Box,
                center: forge_core::Point3::origin(),
                dims: forge_core::Vector3::new(10.0, 10.0, 10.0),
            },
        ))
        .unwrap();
        snapshot_document(&doc);
        let ron = SNAPSHOT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .expect("snapshot stored");
        // Decode through the public path so version checks apply.
        let loaded: Document = ron::from_str(&ron).expect("valid ron");
        assert_eq!(loaded.name, "crashy");
        assert_eq!(loaded.tree.len(), 1);
        assert_eq!(loaded.format_version, forge_core::NATIVE_FORMAT_VERSION);
        // Clear for other tests.
        *SNAPSHOT.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    #[test]
    fn crash_dir_is_under_temp() {
        let dir = crash_dir();
        assert!(dir.starts_with(std::env::temp_dir()));
        assert!(dir.ends_with("forgecad_crash"));
    }
}

// ---------------------------------------------------------------------------
// wasm32 stubs (W-10): no disk, no stderr — eframe installs a browser
// panic handler that reports to the console. The in-memory snapshot /
// report machinery is native-only.
// ---------------------------------------------------------------------------
#[cfg(target_arch = "wasm32")]
pub fn install_hook() {}

#[cfg(target_arch = "wasm32")]
pub fn snapshot_document(_doc: &Document) {}

#[cfg(target_arch = "wasm32")]
pub fn latest_snapshot() -> Option<std::path::PathBuf> {
    None
}
