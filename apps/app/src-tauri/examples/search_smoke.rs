//! 端到端语义搜索冒烟：init runtime → 加载模型 → 文本编码 → KNN → 打印命中文件名。
//! 用法：cargo run -p miaomory-app --example search_smoke -- <模型目录> <db路径> <dll路径> [query]
//! （本地已有模型/索引后可用；不进 CI——需要模型文件与运行时 dll）

use mm_core::Indexer;
use mm_embed::{manifest, EpKind};
use mm_store::Store;

fn main() {
    let mut args = std::env::args().skip(1);
    let model_dir = std::path::PathBuf::from(
        args.next()
            .expect("用法: search_smoke <模型目录> <db> <dll> [query]"),
    );
    let db = args.next().expect("缺少 db 路径");
    let dll = args.next().expect("缺少 dll 路径");
    let query = args.next().unwrap_or_else(|| "花".into());

    mm_embed::init_runtime_dylib(std::path::Path::new(&dll)).expect("ort 运行时初始化失败");
    let m = manifest::manifest();
    let (ix, degraded) =
        mm_embed::ClipEmbedder::load(&model_dir, &m, 1, EpKind::Cpu).expect("加载模型失败");
    if let Some(r) = degraded {
        println!("EP 降级: {r}");
    }
    let qvec = Indexer::embed_text(&ix, &query).expect("文本编码失败");
    println!("query dims: {}", qvec.len());

    let store = Store::open(std::path::Path::new(&db)).expect("打开索引库失败");
    let hits = store.knn_search(1, &qvec, 10).expect("KNN 检索失败");
    println!("『{query}』KNN hits: {}", hits.len());
    for (asset_id, dist) in hits.iter().take(10) {
        if let Ok(Some(row)) = store.get_asset(*asset_id) {
            let name = row.storage_key.rsplit(['\\', '/']).next().unwrap_or("");
            println!("  {name}  (distance={dist:.4})");
        }
    }
    println!("SEARCH SMOKE OK");
}
