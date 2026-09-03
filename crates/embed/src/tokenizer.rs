//! BERT 中文分词（Chinese-CLIP 文本侧）：vocab.txt → WordPiece，输出
//! input_ids / attention_mask / token_type_ids，手动 pad/truncate 到 context_length。
//! 注意：裸 WordPiece 模型无 PostProcessor，[CLS]=101 / [SEP]=102 手动添加。

use std::path::Path;

use mm_core::ErrorCode;
use tokenizers::{models::wordpiece::WordPiece, Tokenizer};

pub const CLS_ID: i64 = 101;
pub const SEP_ID: i64 = 102;

pub struct BertTokenizer {
    tokenizer: Tokenizer,
    pub context_length: usize,
    pad_id: i64,
}

/// 一条编码结果（已 pad 到 context_length）
pub struct Encoded {
    pub input_ids: Vec<i64>,
    pub attention_mask: Vec<i64>,
    pub token_type_ids: Vec<i64>,
}

impl BertTokenizer {
    pub fn from_vocab(vocab_path: &Path, context_length: usize) -> Result<Self, ErrorCode> {
        let vocab_str = vocab_path
            .to_str()
            .ok_or(ErrorCode::ModelMissing)?
            .to_string();
        let word_piece = WordPiece::from_file(&vocab_str)
            .unk_token("[UNK]".to_string())
            .build()
            .map_err(|_| ErrorCode::ModelMissing)?;
        let mut tokenizer = Tokenizer::new(word_piece);
        // 裸 WordPiece 无预切分：按空白切词，逐词 WordPiece
        tokenizer.with_pre_tokenizer(Some(tokenizers::pre_tokenizers::whitespace::Whitespace));
        // [PAD] 在 bert-base-chinese 词表中的 id 为 0
        let pad_id = tokenizer
            .token_to_id("[PAD]")
            .ok_or(ErrorCode::ModelMissing)? as i64;
        Ok(Self {
            tokenizer,
            context_length,
            pad_id,
        })
    }

    /// [CLS] text [SEP] 编码 + pad；超长截断（保留 CLS/SEP）
    pub fn encode(&self, text: &str) -> Result<Encoded, ErrorCode> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(ErrorCode::SearchUnavailable);
        }
        // bert-base-chinese 约定 do_lower_case=true
        let enc = self
            .tokenizer
            .encode(trimmed.to_lowercase().as_str(), false)
            .map_err(|_| ErrorCode::SearchUnavailable)?;

        let ctx = self.context_length;
        let max_content = ctx.saturating_sub(2);
        let mut ids: Vec<i64> = enc
            .get_ids()
            .iter()
            .take(max_content)
            .map(|&v| v as i64)
            .collect();
        ids.insert(0, CLS_ID);
        ids.push(SEP_ID);

        let mut input_ids = vec![self.pad_id; ctx];
        let mut attention_mask = vec![0i64; ctx];
        let token_type_ids = vec![0i64; ctx];
        input_ids[..ids.len()].copy_from_slice(&ids);
        attention_mask[..ids.len()].fill(1);
        // token_type_ids 单句全 0（含 pad 位置，与 cn_clip 参考实现一致）
        Ok(Encoded {
            input_ids,
            attention_mask,
            token_type_ids,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 词表由仓库测试资产提供（见 crates/embed/assets/vocab.txt）
    fn tokenizer() -> BertTokenizer {
        let vocab = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/vocab.txt");
        BertTokenizer::from_vocab(&vocab, 52).unwrap()
    }

    #[test]
    fn encodes_with_special_tokens_and_padding() {
        let t = tokenizer();
        let enc = t.encode("一只猫").unwrap();
        assert_eq!(enc.input_ids.len(), 52);
        assert_eq!(enc.attention_mask.len(), 52);
        // [CLS]=101, [SEP]=102（bert-base-chinese 词表固定值）
        assert_eq!(enc.input_ids[0], CLS_ID);
        let seq_len = enc.attention_mask.iter().sum::<i64>() as usize;
        assert_eq!(enc.input_ids[seq_len - 1], SEP_ID);
        assert!(enc.input_ids[seq_len..].iter().all(|&v| v == 0));
        assert!(enc.token_type_ids.iter().all(|&v| v == 0));
    }

    #[test]
    fn truncates_long_text() {
        let t = tokenizer();
        // 词粒度文本（WordPiece 对超长无空格串会整词转 [UNK]）
        let long = "猫 狗 ".repeat(100);
        let enc = t.encode(&long).unwrap();
        assert_eq!(enc.input_ids.len(), 52);
        assert_eq!(enc.attention_mask.iter().sum::<i64>(), 52);
        assert_eq!(*enc.input_ids.last().unwrap(), SEP_ID);
    }

    #[test]
    fn rejects_blank_input() {
        let t = tokenizer();
        assert!(matches!(t.encode("   "), Err(ErrorCode::SearchUnavailable)));
    }
}
