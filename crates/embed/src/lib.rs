//! `mm-embed`：唯一允许碰 ONNX 的 crate（白皮书 §4.1）。
//!
//! 临时模型 Chinese-CLIP ViT-B/16（ADR-0010，MIT、512 维）；推理经 ort，
//! EP：Windows DirectML → CPU / macOS CoreML → CPU / Linux CPU。
//! 视觉输入是否定长 224×224 由清单字段决定（资产构建时检查并写入）。

pub mod download;
pub mod manifest;
pub mod preprocess;
pub mod quantize;
pub mod tokenizer;

use std::path::Path;
use std::sync::Mutex;

use mm_core::{DecodedImage, Embedder, ErrorCode};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

use crate::manifest::ModelManifest;
use crate::tokenizer::BertTokenizer;

pub struct ClipEmbedder {
    visual: Mutex<Session>,
    text: Mutex<Session>,
    visual_input_name: String,
    visual_output_name: String,
    text_input_names: Vec<String>,
    text_output_name: String,
    text_input_is_i32: bool,
    tokenizer: BertTokenizer,
    visual_fixed_square: bool,
    pub embedding_dim: usize,
}

const IMAGE_BATCH: usize = 8;

impl ClipEmbedder {
    pub fn load(model_dir: &Path, manifest: &ModelManifest) -> Result<Self, ErrorCode> {
        let visual_bytes =
            std::fs::read(model_dir.join("visual.onnx")).map_err(|_| ErrorCode::ModelMissing)?;
        let text_bytes =
            std::fs::read(model_dir.join("text.onnx")).map_err(|_| ErrorCode::ModelMissing)?;
        let tokenizer = BertTokenizer::from_vocab(
            &model_dir.join("vocab.txt"),
            manifest.context_length.unwrap_or(52),
        )?;

        let visual = load_session(&visual_bytes)?;
        let text = load_session(&text_bytes)?;

        let visual_input_name = visual
            .inputs()
            .first()
            .map(|i| i.name().to_string())
            .ok_or(ErrorCode::ModelMissing)?;
        let visual_output_name = visual
            .outputs()
            .first()
            .map(|o| o.name().to_string())
            .ok_or(ErrorCode::ModelMissing)?;
        let text_input_names: Vec<String> = visual_then_text_inputs(&text);
        let text_output_name = text
            .outputs()
            .first()
            .map(|o| o.name().to_string())
            .ok_or(ErrorCode::ModelMissing)?;
        let text_input_is_i32 = matches!(
            text.inputs().first().map(|i| i.dtype()),
            Some(ort::value::ValueType::Tensor {
                ty: ort::value::TensorElementType::Int32,
                ..
            })
        );

        Ok(Self {
            visual: Mutex::new(visual),
            text: Mutex::new(text),
            visual_input_name,
            visual_output_name,
            text_input_names,
            text_output_name,
            text_input_is_i32,
            tokenizer,
            visual_fixed_square: manifest.visual_fixed_square,
            embedding_dim: manifest.embedding_dim,
        })
    }

    /// 文本 → 归一化 f32（模型自身已归一化，这里再归一一次兜底）
    pub fn embed_text(&self, query: &str) -> Result<Vec<f32>, ErrorCode> {
        let enc = self.tokenizer.encode(query)?;
        let ctx = self.tokenizer.context_length;
        let mut session = self.text.lock().unwrap();

        let output = if self.text_input_is_i32 {
            let ids: Vec<i32> = enc.input_ids.iter().map(|&v| v as i32).collect();
            let mask: Vec<i32> = enc.attention_mask.iter().map(|&v| v as i32).collect();
            let types: Vec<i32> = enc.token_type_ids.iter().map(|&v| v as i32).collect();
            let inputs = ort::inputs! {
                self.text_input_names[0].as_str() => Tensor::from_array(([1i64, ctx as i64], ids)).map_err(|_| ErrorCode::SearchUnavailable)?,
                self.text_input_names[1].as_str() => Tensor::from_array(([1i64, ctx as i64], mask)).map_err(|_| ErrorCode::SearchUnavailable)?,
                self.text_input_names[2].as_str() => Tensor::from_array(([1i64, ctx as i64], types)).map_err(|_| ErrorCode::SearchUnavailable)?,
            };
            run_and_take_f32(&mut session, inputs, &self.text_output_name)?
        } else {
            let inputs = ort::inputs! {
                self.text_input_names[0].as_str() => Tensor::from_array(([1i64, ctx as i64], enc.input_ids.clone())).map_err(|_| ErrorCode::SearchUnavailable)?,
                self.text_input_names[1].as_str() => Tensor::from_array(([1i64, ctx as i64], enc.attention_mask.clone())).map_err(|_| ErrorCode::SearchUnavailable)?,
                self.text_input_names[2].as_str() => Tensor::from_array(([1i64, ctx as i64], enc.token_type_ids.clone())).map_err(|_| ErrorCode::SearchUnavailable)?,
            };
            run_and_take_f32(&mut session, inputs, &self.text_output_name)?
        };

        let mut v = output;
        quantize::normalize(&mut v)?;
        Ok(v)
    }

    /// 批量图像 → 归一化 f32 列表（定长输入按批推理，动态形状逐张）
    pub fn embed_images(&self, batch: &[DecodedImage]) -> Result<Vec<Vec<f32>>, ErrorCode> {
        let mut out = Vec::with_capacity(batch.len());
        if self.visual_fixed_square {
            for chunk in batch.chunks(IMAGE_BATCH) {
                let mut flat = Vec::new();
                for p in chunk {
                    flat.extend_from_slice(&preprocess::to_chw(p, true)?.0);
                }
                let n = chunk.len() as i64;
                let mut session = self.visual.lock().unwrap();
                let tensor = Tensor::from_array(([n, 3i64, 224, 224], flat))
                    .map_err(|_| ErrorCode::DecodeFailed)?;
                let inputs = ort::inputs! { self.visual_input_name.as_str() => tensor };
                let mut outputs_all =
                    run_and_take_f32(&mut session, inputs, &self.visual_output_name)?;
                // 一次 run 返回 [N, dim]；拆开
                for v in outputs_all.chunks_mut(self.embedding_dim) {
                    quantize::normalize(v)?;
                    out.push(v.to_vec());
                }
            }
        } else {
            for p in batch {
                let (chw, w, h) = preprocess::to_chw(p, false)?;
                let mut session = self.visual.lock().unwrap();
                let tensor = Tensor::from_array(([1i64, 3i64, w as i64, h as i64], chw))
                    .map_err(|_| ErrorCode::DecodeFailed)?;
                let inputs = ort::inputs! { self.visual_input_name.as_str() => tensor };
                let mut v = run_and_take_f32(&mut session, inputs, &self.visual_output_name)?;
                quantize::normalize(&mut v)?;
                out.push(v);
            }
        }
        Ok(out)
    }
}

fn visual_then_text_inputs(session: &Session) -> Vec<String> {
    session
        .inputs()
        .iter()
        .map(|i| i.name().to_string())
        .collect()
}

fn run_and_take_f32<'i, 'v, I>(
    session: &mut Session,
    inputs: I,
    output_name: &str,
) -> Result<Vec<f32>, ErrorCode>
where
    I: Into<ort::session::SessionInputs<'i, 'v>>,
    'v: 'i,
{
    let outputs = session
        .run(inputs)
        .map_err(|_| ErrorCode::SearchUnavailable)?;
    let (shape, data) = outputs[output_name]
        .try_extract_tensor::<f32>()
        .map_err(|_| ErrorCode::SearchUnavailable)?;
    let _ = shape;
    Ok(data.to_vec())
}

fn load_session(bytes: &[u8]) -> Result<Session, ErrorCode> {
    #[cfg(any(windows, target_os = "macos"))]
    use ort::ep;

    // Linux 无 EP shadowing：基础绑定需要 mut；Windows/macOS 被 shadow 消费，mut 闲置
    #[allow(unused_mut)]
    let mut builder = Session::builder()
        .map_err(|_| ErrorCode::ModelMissing)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|_| ErrorCode::ModelMissing)?;
    #[cfg(windows)]
    let mut builder = builder
        .with_execution_providers([ep::DirectML::default().build(), ep::CPU::default().build()])
        .map_err(|_| ErrorCode::ModelMissing)?;
    #[cfg(target_os = "macos")]
    let mut builder = builder
        .with_execution_providers([ep::CoreML::default().build(), ep::CPU::default().build()])
        .map_err(|_| ErrorCode::ModelMissing)?;
    builder
        .commit_from_memory(bytes)
        .map_err(|_| ErrorCode::ModelMissing)
}

impl Embedder for ClipEmbedder {
    fn embed_images(&self, batch: &[DecodedImage]) -> Result<Vec<Vec<f32>>, ErrorCode> {
        ClipEmbedder::embed_images(self, batch)
    }
    fn embed_text(&self, text: &str) -> Result<Vec<f32>, ErrorCode> {
        ClipEmbedder::embed_text(self, text)
    }
}
