//! 混合检索融合：Reciprocal Rank Fusion（白皮书 §4.6，k=60）。
//! 纯函数，零 IO——排序正确性是检索体验的根基，单测全覆盖。

use crate::AssetId;

/// 单条融合结果
#[derive(Debug, Clone, PartialEq)]
pub struct FusedHit {
    pub asset_id: AssetId,
    pub score: f64,
    /// 排序理由：both（双流）/ semantic（语义）/ text（文件名）
    pub matched: Matched,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matched {
    Both,
    Semantic,
    Text,
}

impl Matched {
    pub fn slug(&self) -> &'static str {
        match self {
            Matched::Both => "both",
            Matched::Semantic => "semantic",
            Matched::Text => "text",
        }
    }
}

/// RRF：score = Σ 1/(k + rank)；rank 从 1 起。同流重复 id 取最优排名。
/// 输入列表已按各自相关性降序排列。输出按 score 降序，同分按 id 升序（确定性）。
pub fn rrf_fuse(semantic: &[AssetId], text: &[AssetId], k: f64) -> Vec<FusedHit> {
    rrf_fuse_multi(&[semantic], text, k)
}

/// 多流 RRF（ADR-0013）：语义流可来自多套索引（每套一路），文本流一路。
/// 语义流计数入 matched.semantic，文本流计入 text；both = 两类皆命中。
pub fn rrf_fuse_multi(semantic_lists: &[&[AssetId]], text: &[AssetId], k: f64) -> Vec<FusedHit> {
    let k = if k <= 0.0 { 60.0 } else { k };
    let mut scores: std::collections::BTreeMap<AssetId, (f64, bool, bool)> =
        std::collections::BTreeMap::new();

    let mut all: Vec<(bool, &[AssetId])> = Vec::with_capacity(semantic_lists.len() + 1);
    for list in semantic_lists {
        all.push((true, list));
    }
    all.push((false, text));

    for (is_semantic, list) in all {
        let mut seen_in_stream = std::collections::HashSet::new();
        for (idx, id) in list.iter().enumerate() {
            if !seen_in_stream.insert(*id) {
                continue; // 同流去重：保留最优（最先出现）排名
            }
            let contribution = 1.0 / (k + idx as f64 + 1.0);
            let e = scores.entry(*id).or_insert((0.0, false, false));
            e.0 += contribution;
            if is_semantic {
                e.1 = true;
            } else {
                e.2 = true;
            }
        }
    }

    let mut hits: Vec<FusedHit> = scores
        .into_iter()
        .map(|(asset_id, (score, sem, txt))| FusedHit {
            asset_id,
            score,
            matched: match (sem, txt) {
                (true, true) => Matched::Both,
                (true, false) => Matched::Semantic,
                _ => Matched::Text,
            },
        })
        .collect();
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.asset_id.cmp(&b.asset_id))
    });
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: f64 = 60.0;

    #[test]
    fn both_stream_hit_ranks_first() {
        let hits = rrf_fuse(&[1, 2, 3], &[2, 4], K);
        assert_eq!(hits[0].asset_id, 2);
        assert_eq!(hits[0].matched, Matched::Both);
        // 双流分数 = 两个单流分数之和
        assert!(hits[0].score > hits[1].score);
        // 单流命中标记正确
        assert!(hits
            .iter()
            .any(|h| h.asset_id == 1 && h.matched == Matched::Semantic));
        assert!(hits
            .iter()
            .any(|h| h.asset_id == 4 && h.matched == Matched::Text));
    }

    #[test]
    fn empty_stream_falls_back_to_other() {
        let hits = rrf_fuse(&[], &[5, 6], K);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].asset_id, 5);
        assert_eq!(hits[0].matched, Matched::Text);

        let hits2 = rrf_fuse(&[7], &[], K);
        assert_eq!(hits2.len(), 1);
        assert_eq!(hits2[0].matched, Matched::Semantic);
    }

    #[test]
    fn duplicates_within_stream_take_best_rank() {
        let hits = rrf_fuse(&[9, 9, 8], &[], K);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].asset_id, 9); // 首次出现即最优排名
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn ordering_is_deterministic_and_desc() {
        let a = rrf_fuse(&[1, 2, 3, 4], &[3, 4, 5], K);
        let b = rrf_fuse(&[1, 2, 3, 4], &[3, 4, 5], K);
        assert_eq!(a, b);
        assert!(a.windows(2).all(|w| w[0].score >= w[1].score));
        // 双流命中的 id=3（两路各 1/61+1/62）应超过单流 rank1 的 id=1（1/61）
        assert_eq!(a[0].asset_id, 3);
        assert_eq!(a[0].matched, Matched::Both);
    }
}
