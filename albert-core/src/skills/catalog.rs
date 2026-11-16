//! Metadata catalog and ranked skill discovery.
use serde_json::{json, Value};

use super::{SkillMeta, SkillStore};
impl SkillStore {
    /// Every selectable skill's name and complete trigger description, on every
    /// turn. Bodies stay out until `skill_apply`, except for standing instructions.
    pub fn catalog(&self) -> String {
        let inner = self.inner.lock().unwrap();
        // An `always: true` skill is not offered, it is IN FORCE: its body goes in
        // verbatim every turn, so obeying it costs no tool call and it cannot lapse
        // between turns. It is also never paginated away — a standing instruction
        // that disappears once the catalog grows would be worse than none.
        let mut out = String::new();
        for skill in inner.catalog.iter() {
            let Some(body) = &skill.standing else {
                continue;
            };
            out.push_str(&format!(
                "STANDING INSTRUCTIONS — \"{}\", in force for EVERY reply, not optional and not \
                 expiring between turns. They shape HOW you answer; your persona (who you are, \
                 your voice) still governs. Where the two collide, keep the voice and follow the \
                 shape.\n\n{body}\n\n---\n\n",
                skill.name,
            ));
        }
        // Only the selectable skills are listed or counted — an in-force one has
        // already been applied.
        let listable: Vec<&SkillMeta> = inner
            .catalog
            .iter()
            .filter(|s| s.standing.is_none())
            .collect();
        let n = listable.len();
        if n == 0 {
            out.push_str("Skills: (none installed).");
        } else {
            out.push_str(&format!(
                "Skills available ({n} installed; apply a matching skill with skill_apply; \
                 skill_search to search, skill_list to browse by page):\n"
            ));
            for skill in listable {
                out.push_str(&format!("- {}: {}\n", skill.name, skill.when));
            }
        }
        out
    }
    /// One page of the catalog (name + when-to-use), 1-indexed, `PAGE` per page.
    pub(super) fn list_json(&self, page: usize) -> Value {
        let inner = self.inner.lock().unwrap();
        let size = inner.page;
        let total = inner.catalog.len();
        let pages = total.div_ceil(size).max(1);
        let page = page.clamp(1, pages);
        let items: Vec<Value> = inner
            .catalog
            .iter()
            .skip((page - 1) * size)
            .take(size)
            .map(|s| json!({ "name": s.name, "when": s.when }))
            .collect();
        json!({ "skills": items, "page": page, "pages": pages, "total": total })
    }
    /// Rank the catalog against `query` (keywords). A term in a skill's name scores 2,
    /// in its description 1; skills with any hit are returned best-first, up to `limit`.
    /// An empty query just returns the first `limit` by name (a browse fallback).
    pub(super) fn search_json(&self, query: &str, limit: usize) -> Value {
        let inner = self.inner.lock().unwrap();
        let q = query.to_lowercase();
        let terms: Vec<&str> = q.split_whitespace().collect();

        let mut scored: Vec<(i32, &SkillMeta)> = if terms.is_empty() {
            inner.catalog.iter().map(|s| (0, s)).collect()
        } else {
            inner
                .catalog
                .iter()
                .filter_map(|s| {
                    let name = s.name.to_lowercase();
                    let when = s.when.to_lowercase();
                    let score: i32 = terms
                        .iter()
                        .map(|t| (name.contains(t) as i32) * 2 + (when.contains(t) as i32))
                        .sum();
                    (score > 0).then_some((score, s))
                })
                .collect()
        };
        // Best score first; ties by name for a stable order.
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        let total_matched = scored.len();
        let items: Vec<Value> = scored
            .iter()
            .take(limit)
            .map(|(_, s)| json!({ "name": s.name, "when": s.when }))
            .collect();
        json!({
            "query": query,
            "total_matched": total_matched,
            "returned": items.len(),
            "skills": items,
        })
    }
}
