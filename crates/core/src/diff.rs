//! Minimal edits between two versions of a text, so that an editor applying
//! them keeps its cursor, marks and comment anchors on unchanged text.

use crate::md::edit::Change;
use similar::{Algorithm, DiffTag, TextDiff};
use std::time::Duration;

/// Minimal replacements turning `old` into `new`, as byte ranges of `old`.
pub fn diff_changes(old: &str, new: &str) -> Vec<Change> {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .timeout(Duration::from_millis(300))
        .diff_lines(old, new);
    let mut offs_old = vec![0];
    for s in diff.iter_old_slices() {
        offs_old.push(offs_old.last().unwrap() + s.len());
    }
    let mut offs_new = vec![0];
    for s in diff.iter_new_slices() {
        offs_new.push(offs_new.last().unwrap() + s.len());
    }
    let mut changes = Vec::new();
    for op in diff.ops() {
        let (tag, o, n) = op.as_tag_tuple();
        if tag == DiffTag::Equal {
            continue;
        }
        let o = offs_old[o.start]..offs_old[o.end];
        let n = offs_new[n.start]..offs_new[n.end];
        if o.len() <= 8192 && n.len() <= 8192 && !o.is_empty() && !n.is_empty() {
            // Refine so that marks inside mostly-unchanged lines stay put.
            let sub = TextDiff::configure()
                .algorithm(Algorithm::Myers)
                .timeout(Duration::from_millis(50))
                .diff_chars(&old[o.clone()], &new[n.clone()]);
            let old_sl: Vec<usize> = sub.iter_old_slices().map(str::len).collect();
            let new_sl: Vec<usize> = sub.iter_new_slices().map(str::len).collect();
            let mut so = o.start;
            let mut sn = n.start;
            for sop in sub.ops() {
                let (t, a, b) = sop.as_tag_tuple();
                let a_len: usize = old_sl[a].iter().sum();
                let b_len: usize = new_sl[b].iter().sum();
                if t != DiffTag::Equal {
                    changes.push(Change::replace(so..so + a_len, &new[sn..sn + b_len]));
                }
                so += a_len;
                sn += b_len;
            }
        } else {
            changes.push(Change::replace(o, &new[n]));
        }
    }
    changes
}
