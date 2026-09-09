//! `mm-embed`：唯一允许碰 ONNX 的 crate（白皮书 §4.1）。
//!
//! 临时模型 Chinese-CLIP ViT-B/16（ADR-0010，MIT、512 维）；推理经 ort。
//! EP 运行时可配置（spec 0008 / ADR-0014）：CPU（默认）/ DirectML / CUDA，
//! 所选 EP 初始化失败自动降级 CPU 并带出原因；macOS 维持 CoreML → CPU 编译期现状。
//! Windows ort 为 load-dynamic：app 启动最早处须经 [`init_runtime_dylib`] 选定
//! onnxruntime 变体（进程级一次性，切换后端重启生效）。
//! 视觉输入是否定长 224×224 由清单字段决定（资产构建时检查并写入）。

pub mod download;
pub mod manifest;
pub mod preprocess;
pub mod quantize;
pub mod runtime;
pub mod tokenizer;

use std::path::Path;
use std::sync::Mutex;

use mm_core::{DecodedImage, ErrorCode, Indexer};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

use crate::manifest::ModelManifest;
use crate::tokenizer::BertTokenizer;

/// 推理执行提供者（spec 0008）。`parse` 对未知值与平台不支持项返回 None（调用方回落默认）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EpKind {
    #[default]
    Cpu,
    DirectML,
    Cuda,
}

impl EpKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "cpu" => Some(Self::Cpu),
            "directml" if cfg!(windows) => Some(Self::DirectML),
            "cuda" if cfg!(windows) => Some(Self::Cuda),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::DirectML => "directml",
            Self::Cuda => "cuda",
        }
    }

    /// EP 注册链（后者兜底）。macOS 的「默认」维持既有 CoreML→CPU 行为（不变相降级）。
    fn chain(self) -> &'static [Self] {
        #[cfg(windows)]
        match self {
            Self::Cpu => &[Self::Cpu],
            Self::DirectML => &[Self::DirectML, Self::Cpu],
            Self::Cuda => &[Self::Cuda, Self::DirectML, Self::Cpu],
        }
        #[cfg(target_os = "macos")]
        {
            let _ = self;
            &[Self::Cpu] // macOS 侧由 build_session 特判注册 CoreML
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = self;
            &[Self::Cpu]
        }
    }
}

/// 加载 onnxruntime 动态库变体并提交 ort 全局环境（**进程级仅一次**）。
/// 必须在任何 session 构建之前调用（app `run()` 最早处）；失败由调用方回退
/// 自带变体重试。非 Windows 为编译期链接，直接成功。
pub fn init_runtime_dylib(dll: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let builder = ort::init_from(dll)
            .map_err(|e| format!("加载 onnxruntime 变体失败（{}）：{e}", dll.display()))?;
        if !builder.with_name("miaomory").commit() {
            return Err("ort 环境已被重复初始化（进程级只允许一次）".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = dll;
        Ok(())
    }
}

pub struct ClipEmbedder {
    /// 索引身份（ADR-0013）：对应 index_meta.index_id
    index_id: i64,
    slug: &'static str,
    display: &'static str,
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
    /// `index_id` 来自 index_meta 注册表（内置 CLIP 索引由调用方传入）；
    /// `ep` 为所选推理后端（spec 0008）。返回（实例，降级原因——None = 按所选 EP 生效）。
    pub fn load(
        model_dir: &Path,
        manifest: &ModelManifest,
        index_id: i64,
        ep: EpKind,
    ) -> Result<(Self, Option<String>), ErrorCode> {
        // 文件名来自清单（如 visual.int8.onnx），不硬编码（量化/非量化命名不同）
        let read_model_file = |keyword: &str| -> Result<Vec<u8>, ErrorCode> {
            let name = manifest
                .files
                .iter()
                .map(|f| &f.name)
                .find(|n| n.contains(keyword))
                .ok_or(ErrorCode::ModelMissing)?;
            std::fs::read(model_dir.join(name)).map_err(|_| ErrorCode::ModelMissing)
        };
        let visual_bytes = read_model_file("visual")?;
        let text_bytes = read_model_file("text")?;
        let vocab_name = manifest
            .files
            .iter()
            .map(|f| &f.name)
            .find(|n| n.contains("vocab"))
            .ok_or(ErrorCode::ModelMissing)?;
        let tokenizer = BertTokenizer::from_vocab(
            &model_dir.join(vocab_name),
            manifest.context_length.unwrap_or(52),
        )?;

        let (visual, degraded_visual) = load_session(&visual_bytes, ep)?;
        let (text, degraded_text) = load_session(&text_bytes, ep)?;
        let degraded = degraded_visual.or(degraded_text);

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

        Ok((
            Self {
            index_id,
            slug: "chinese-clip-vit-b16-int8",
            display: "Chinese-CLIP ViT-B/16（int8）",
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
        },
        degraded,
        ))
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

/// 按所选 EP 构建 session（spec 0008）。先按 EP 注册链尝试；失败则收敛纯 CPU
/// 重建并返回降级原因（所选 EP 不可用，常见：CUDA 驱动过旧/缺运行时组件）。
fn load_session(bytes: &[u8], ep: EpKind) -> Result<(Session, Option<String>), ErrorCode> {
    #[cfg(any(windows, target_os = "macos"))]
    use ort::ep;

    #[cfg(windows)]
    fn build(bytes: &[u8], chain: &[EpKind]) -> Result<Session, ErrorCode> {
        let build_ep = |k: EpKind| match k {
            EpKind::Cpu => ep::CPU::default().build(),
            EpKind::DirectML => ep::DirectML::default().build(),
            EpKind::Cuda => ep::CUDA::default().build(),
        };
        let providers: Vec<_> = chain.iter().map(|k| build_ep(*k)).collect();
        Session::builder()
            .map_err(|_| ErrorCode::ModelMissing)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|_| ErrorCode::ModelMissing)?
            .with_execution_providers(providers)
            .map_err(|_| ErrorCode::ModelMissing)?
            .commit_from_memory(bytes)
            .map_err(|_| ErrorCode::ModelMissing)
    }
    #[cfg(target_os = "macos")]
    fn build(bytes: &[u8], _chain: &[EpKind]) -> Result<Session, ErrorCode> {
        // macOS 维持编译期 CoreML → CPU 现状（spec 0008 非目标：CoreML 可选化）
        Session::builder()
            .map_err(|_| ErrorCode::ModelMissing)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|_| ErrorCode::ModelMissing)?
            .with_execution_providers([ep::CoreML::default().build(), ep::CPU::default().build()])
            .map_err(|_| ErrorCode::ModelMissing)?
            .commit_from_memory(bytes)
            .map_err(|_| ErrorCode::ModelMissing)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    fn build(bytes: &[u8], _chain: &[EpKind]) -> Result<Session, ErrorCode> {
        // Linux 纯 CPU（无 EP shadowing）
        Session::builder()
            .map_err(|_| ErrorCode::ModelMissing)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|_| ErrorCode::ModelMissing)?
            .commit_from_memory(bytes)
            .map_err(|_| ErrorCode::ModelMissing)
    }

    match build(bytes, ep.chain()) {
        Ok(session) => Ok((session, None)),
        // 降级重建：纯 CPU 链（macOS 恒 CoreML→CPU，不会走到这里）
        Err(_) => {
            let fallback: &[EpKind] = &[EpKind::Cpu];
            let session = build(bytes, fallback)?;
            Ok((
                session,
                Some(format!(
                    "{} 初始化失败（驱动/运行时不可用），本次以 CPU 继续",
                    ep.as_str()
                )),
            ))
        }
    }
}

impl Indexer for ClipEmbedder {
    fn index_id(&self) -> i64 {
        self.index_id
    }
    fn slug(&self) -> &str {
        self.slug
    }
    fn display(&self) -> &str {
        self.display
    }
    fn dim(&self) -> u32 {
        self.embedding_dim as u32
    }
    fn supports_text(&self) -> bool {
        true
    }
    fn embed_images(&self, batch: &[DecodedImage]) -> Result<Vec<Vec<f32>>, ErrorCode> {
        ClipEmbedder::embed_images(self, batch)
    }
    fn embed_text(&self, text: &str) -> Result<Vec<f32>, ErrorCode> {
        ClipEmbedder::embed_text(self, text)
    }
}
