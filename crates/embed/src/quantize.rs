//! f32 ↔ INT8 量化（ADR-0002：sqlite-vec INT8 向量，白皮书 §3.2）。
//! 归一化后的向量落在 [-1,1]，×127 保留符号与精度。

use mm_core::ErrorCode;

/// 归一化 f32 向量 → INT8（×127 舍入）
pub fn quantize(v: &[f32]) -> Vec<i8> {
    v.iter()
        .map(|&x| {
            let scaled = (x * 127.0).round();
            scaled.clamp(-128.0, 127.0) as i8
        })
        .collect()
}

/// INT8 → f32（÷127；用于本地近似计算/调试，检索走 sqlite-vec）
pub fn dequantize(v: &[i8]) -> Vec<f32> {
    v.iter().map(|&x| f32::from(x) / 127.0).collect()
}

/// L2 归一化（零向量原样返回）
pub fn normalize(v: &mut [f32]) -> Result<(), ErrorCode> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return Err(ErrorCode::DecodeFailed);
    }
    for x in v.iter_mut() {
        *x /= norm;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (na * nb)
    }

    #[test]
    fn quantize_dequantize_preserves_direction() {
        // 随机但确定的向量
        let mut v: Vec<f32> = (0..512)
            .map(|i| (i as f32 * 0.37).sin() * 3.1 + (i as f32 * 0.11).cos())
            .collect();
        normalize(&mut v).unwrap();
        let q = quantize(&v);
        let back = dequantize(&q);
        assert!(cosine(&v, &back) >= 0.99, "量化前后余弦相似度应 ≥0.99");
        assert_eq!(q.len(), 512);
        assert!(q.iter().all(|&x| (-127..=127).contains(&x)));
    }

    #[test]
    fn quantize_clamps_extremes() {
        let q = quantize(&[2.0, -2.0, 0.0]);
        assert_eq!(q, vec![127, -128, 0]);
    }

    #[test]
    fn normalize_rejects_zero_vector() {
        let mut v = vec![0.0f32; 4];
        assert_eq!(normalize(&mut v), Err(ErrorCode::DecodeFailed));
        let mut w = vec![3.0, 4.0];
        normalize(&mut w).unwrap();
        assert!((w[0] - 0.6).abs() < 1e-6 && (w[1] - 0.8).abs() < 1e-6);
    }
}
