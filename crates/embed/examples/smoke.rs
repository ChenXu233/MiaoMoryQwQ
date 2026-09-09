//! 模型冒烟测试：加载本地模型目录，编码一张测试图 + 中文查询，输出相似度。
//! 用法：cargo run -p mm-embed --example smoke -- <模型目录>
//! （本地已下载模型后可用；不进 CI——需要模型文件）

use mm_core::{DecodedImage, Indexer};
use mm_embed::manifest;

fn main() {
    let dir = std::env::args().nth(1).expect("用法: smoke <模型目录>");
    let dir = std::path::Path::new(&dir);
    let manifest = manifest::manifest();
    let missing = manifest.missing_files(dir);
    assert!(missing.is_empty(), "缺文件: {missing:?}");

    let (embedder, degraded) =
        mm_embed::ClipEmbedder::load(dir, &manifest, 1, Default::default()).expect("加载模型失败");
    if let Some(reason) = degraded {
        println!("EP 降级: {reason}");
    }

    // 8x8 渐变测试图
    let mut rgb = Vec::new();
    for y in 0..8u32 {
        for x in 0..8u32 {
            rgb.extend_from_slice(&[(x * 31) as u8, (y * 31) as u8, 128]);
        }
    }
    let img = DecodedImage {
        width: 8,
        height: 8,
        rgb,
    };
    let vecs = Indexer::embed_images(&embedder, &[img.clone(), img]).expect("图像嵌入失败");
    println!("image embedding dims: {}", vecs[0].len());

    let q = Indexer::embed_text(&embedder, "海边的日落").expect("文本嵌入失败");
    println!("text embedding dims: {}", q.len());

    let dot: f32 = q.iter().zip(&vecs[0]).map(|(a, b)| a * b).sum();
    println!("cross similarity (random image): {dot:.4}");
    println!("SMOKE OK");
}
